//! `.mfd` -> `mapping::Project` conversion.
//!
//! The importer never fails on unsupported constructs: it converts what it
//! can and records a warning per skipped piece, because a partial import
//! the user finishes by hand still beats redrawing the mapping.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FormatOptions, Graph, NamedSource, Node, NodeId, Project, Scope,
};

use crate::MfdError;

pub struct Imported {
    pub project: Project,
    pub warnings: Vec<String>,
}

/// Which family of MapForce component a schema component came from --
/// decides how a document-level (empty-path) connection behaves.
#[derive(Clone, Copy, PartialEq)]
enum ComponentFormat {
    Xml,
    Json,
    Csv,
    Db,
}

/// One schema (source or target) component's extracted facts.
struct SchemaComponent {
    name: String,
    format: ComponentFormat,
    schema: SchemaNode,
    input_instance: Option<String>,
    output_instance: Option<String>,
    options: FormatOptions,
    is_source: bool,
    is_variable: bool,
    /// Input key of a variable component's compute-when control entry.
    compute_when_key: Option<u32>,
    /// Port key -> absolute entry path (segments below the schema root).
    ports: BTreeMap<u32, Vec<String>>,
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
}

/// What an iteration connection resolves to once `filter`/`group-by`
/// components on the way are unwrapped.
struct IterationFeed {
    /// Output key of the underlying source entry (or whatever else feeds
    /// the chain -- callers check it against the source ports).
    source_key: u32,
    /// Path below `source_key` selected by transparent intermediate schema
    /// components crossed on the way to the target iteration.
    source_suffix: Vec<String>,
    /// The filter's boolean expression key, if a filter was crossed.
    filter_expr: Option<u32>,
    /// The group-by's key expression key, if a group-by was crossed.
    group_key: Option<u32>,
    /// A sort key expression and direction crossed by the sequence.
    sort_expr: Option<u32>,
    sort_descending: bool,
    /// A connected first-items count, or an absent count meaning the
    /// function's default of one item.
    take_expr: Option<u32>,
    take_default_one: bool,
    /// A transparent variable projects the connected source group as a
    /// constructed value, so matching scalar descendants must be copied.
    projects_whole_group: bool,
    /// Scalar descendant inputs used to construct an intermediate group,
    /// keyed by their path relative to that group's output.
    projections: BTreeMap<Vec<String>, u32>,
}

/// One function component's extracted facts.
type ValueMapData = (Vec<(String, String)>, Option<String>);

struct FnComponent {
    library: String,
    name: String,
    kind: u32,
    /// Input pins in `pos` order; `None` for declared-but-keyless pins.
    inputs: Vec<Option<u32>>,
    /// Output pin keys in `pos` order.
    outputs: Vec<u32>,
    constant: Option<(String, String)>,
    valuemap: Option<ValueMapData>,
    sort_descending: Option<bool>,
}

struct IntermediateFeed {
    feed: u32,
    suffix: Vec<String>,
    control: Option<u32>,
    projections: BTreeMap<Vec<String>, u32>,
}

pub fn import(path: &Path) -> Result<Imported, MfdError> {
    let text = std::fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let mapping_el = doc.root_element();
    if mapping_el.tag_name().name() != "mapping" {
        return Err(MfdError::NotMfd("root element is not <mapping>"));
    }
    let wrapper = mapping_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "component")
        .ok_or(MfdError::NotMfd("no wrapper component"))?;
    let structure = wrapper
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "structure")
        .ok_or(MfdError::NotMfd("wrapper has no structure"))?;

    let mut warnings = Vec::new();
    let mut schema_components = Vec::new();
    let mut fn_components = Vec::new();
    let mut skipped_libraries: Vec<String> = Vec::new();

    if let Some(children) = structure
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "children")
    {
        for component in children
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "component")
        {
            let library = component.attribute("library").unwrap_or_default();
            let name = component.attribute("name").unwrap_or_default().to_string();
            match library {
                "xml" => match read_schema_component(&component, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => warnings.push(format!("skipped xml component `{name}`")),
                },
                "json" => match read_json_component(&component, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => warnings.push(format!("skipped json component `{name}`")),
                },
                "text" => {
                    let text_el = component
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "data")
                        .and_then(|d| {
                            d.children()
                                .find(|n| n.is_element() && n.tag_name().name() == "text")
                        });
                    let flavor = text_el.and_then(|t| t.attribute("type")).unwrap_or("");
                    if flavor == "csv" {
                        match read_csv_component(&component, &mut warnings) {
                            Some(sc) => schema_components.push(sc),
                            None => warnings.push(format!("skipped csv component `{name}`")),
                        }
                    } else {
                        let label = if flavor.is_empty() {
                            "text".to_string()
                        } else {
                            format!("text/{flavor}")
                        };
                        note_skipped_library(&mut skipped_libraries, &label);
                        warnings.push(format!(
                            "skipped component `{name}`: text flavor `{flavor}` is \
                             not supported yet (only csv text components import)"
                        ));
                    }
                }
                "db" => match read_db_component(&component, &mapping_el, path, &mut warnings) {
                    Some(sc) => schema_components.push(sc),
                    None => note_skipped_library(&mut skipped_libraries, "db"),
                },
                "core" | "lang" => fn_components.push(read_fn_component(&component)),
                other => {
                    note_skipped_library(&mut skipped_libraries, other);
                    warnings.push(format!(
                        "skipped component `{name}`: unsupported library `{other}` \
                         (only xml/json/csv/db and core/lang function components import)"
                    ));
                }
            }
        }
    }

    // Edges: to-key -> from-key (every input pin has at most one feed).
    let mut edge_from: BTreeMap<u32, u32> = BTreeMap::new();
    if let Some(graph_el) = structure
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "graph")
    {
        for vertex in graph_el.descendants().filter(|n| n.has_tag_name("vertex")) {
            let Some(from) = parse_u32(vertex.attribute("vertexkey")) else {
                continue;
            };
            for edge in vertex.descendants().filter(|n| n.has_tag_name("edge")) {
                if let Some(to) = parse_u32(edge.attribute("vertexkey")) {
                    edge_from.insert(to, from);
                }
            }
        }
    }
    // Older designs store the same directed edges as flat from/to pairs.
    if let Some(connections) = structure
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "connections")
        .or_else(|| {
            wrapper
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "connections")
        })
    {
        for edge in connections
            .children()
            .filter(|node| node.is_element() && node.tag_name().name() == "edge")
        {
            if let (Some(from), Some(to)) = (
                parse_u32(edge.attribute("from")),
                parse_u32(edge.attribute("to")),
            ) {
                edge_from.insert(to, from);
            }
        }
    }

    let sources: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| !c.is_variable && c.is_source)
        .collect();
    let targets: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| !c.is_variable && !c.is_source)
        .collect();
    let intermediates: Vec<&SchemaComponent> =
        schema_components.iter().filter(|c| c.is_variable).collect();
    let unsupported = |side: &str| {
        MfdError::Unsupported(if skipped_libraries.is_empty() {
            format!("no importable {side} component (xml/json/csv/db) found in this design")
        } else {
            format!(
                "no importable {side} component (xml/json/csv/db) found; this design \
                 uses {} components, which ferrule cannot import yet",
                skipped_libraries.join("/")
            )
        })
    };
    let primary = sources.first().ok_or_else(|| unsupported("source"))?;
    let target = targets.first().ok_or_else(|| unsupported("target"))?;
    if targets.len() > 1 {
        warnings.push(format!(
            "multiple target components; only `{}` imported",
            target.name
        ));
    }

    let mut builder = GraphBuilder {
        graph: Graph::default(),
        next_id: 0,
        fn_nodes: BTreeMap::new(),
        source_fields: BTreeMap::new(),
        edge_from: &edge_from,
        sources: &sources,
        intermediates: &intermediates,
        fn_components: &fn_components,
        fn_by_output: BTreeMap::new(),
        framed: std::collections::BTreeSet::new(),
        warnings: Vec::new(),
    };
    for (i, fc) in fn_components.iter().enumerate() {
        for &out in &fc.outputs {
            builder.fn_by_output.insert(out, i);
        }
    }
    // Scopes and bindings from the target's connected ports.
    let mut scope_builder = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    let mut iterations = Vec::new();
    let mut bindings = Vec::new();
    for (&inpkey, target_path) in &target.ports {
        let Some(&from) = edge_from.get(&inpkey) else {
            continue;
        };
        let node_kind = schema_node_at(&target.schema, target_path);
        match node_kind {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {
                // Iteration connection (or filtered iteration). An empty
                // path is a document-level connection: for row/array-shaped
                // targets (a CSV block, a repeating JSON root) it iterates
                // the root scope; for document-shaped targets the root runs
                // exactly once anyway, so it carries no information.
                if target_path.is_empty() {
                    let row_shaped =
                        matches!(target.format, ComponentFormat::Csv | ComponentFormat::Db)
                            || (target.format == ComponentFormat::Json && node.repeating);
                    if row_shaped {
                        iterations.push((target_path.clone(), from));
                    }
                    continue;
                }
                if !node.repeating {
                    builder.warnings.push(format!(
                        "connection into non-repeating group `{}` ignored",
                        target_path.join("/")
                    ));
                    continue;
                }
                iterations.push((target_path.clone(), from));
            }
            Some(_) => bindings.push((target_path.clone(), from)),
            None => builder.warnings.push(format!(
                "target port path `{}` not found in schema",
                target_path.join("/")
            )),
        }
    }
    // Iterations first (outer before inner), so anchors exist for bindings.
    iterations.sort_by_key(|(path, _)| path.len());
    // SourceField paths are relative to the enclosing iteration frames, so
    // the builder must know which repeating levels the scopes will iterate
    // before any function component materializes a SourceField.
    for (_, from) in &iterations {
        let feed = builder.resolve_iteration_feed(*from);
        if let Some(abs) = builder.iteration_source_path(&feed) {
            builder.note_framed_prefixes(&abs);
        }
    }
    // Materialize aggregates first so computed sequence functions are built
    // under their per-item collection frame rather than as outer expressions.
    for (i, fc) in fn_components.iter().enumerate() {
        if fc.kind == 5 && aggregate_op(&fc.name).is_some() {
            builder.fn_node(i);
        }
    }
    // Materialize every remaining value-producing function up front
    // (filters and group-bys are handled at the scope stage instead).
    // Outputless core components are annotations such as comments.
    for (i, fc) in fn_components.iter().enumerate() {
        if !(fc.outputs.is_empty()
            || is_filter_component(fc)
            || is_input_component(fc)
            || is_sort_component(fc)
            || is_first_items_component(fc)
            || fc.name == "group-by"
            || fc.kind == 5 && aggregate_op(&fc.name).is_some())
        {
            builder.fn_node(i);
        }
    }
    let connected_bindings: BTreeSet<Vec<String>> =
        bindings.iter().map(|(path, _)| path.clone()).collect();
    for (target_path, from) in iterations {
        let feed = builder.resolve_iteration_feed(from);
        let Some(source_abs) = builder.iteration_source_path(&feed) else {
            builder.warnings.push(format!(
                "iteration into `{}` comes from an unsupported feed; skipped",
                target_path.join("/")
            ));
            continue;
        };
        let filter_node = feed.filter_expr.and_then(|key| builder.value_node(key));
        let group_node = feed.group_key.and_then(|key| builder.value_node(key));
        let sort_node = feed.sort_expr.and_then(|key| builder.value_node(key));
        let take_node = feed
            .take_expr
            .and_then(|key| builder.value_node(key))
            .or_else(|| {
                feed.take_default_one.then(|| {
                    builder.alloc(Node::Const {
                        value: Value::Int(1),
                    })
                })
            });
        scope_builder.add_iteration(
            &target_path,
            &source_abs,
            IterationNodes {
                filter: filter_node,
                group_by: group_node,
                sort_by: sort_node,
                sort_descending: feed.sort_descending,
                take: take_node,
            },
        );
        if feed.projects_whole_group
            && let (Some(source_group), Some(target_group)) = (
                schema_node_at(&primary.schema, &source_abs),
                schema_node_at(&target.schema, &target_path),
            )
        {
            let mut relative_paths = Vec::new();
            collect_matching_scalar_paths(
                source_group,
                target_group,
                &mut Vec::new(),
                &mut relative_paths,
            );
            for relative in relative_paths {
                let mut target_leaf = target_path.clone();
                target_leaf.extend(relative.iter().cloned());
                if connected_bindings.contains(&target_leaf)
                    || feed.projections.contains_key(&relative)
                {
                    continue;
                }
                let mut source_leaf = source_abs.clone();
                source_leaf.extend(relative);
                if let Some(node) = builder.primary_source_field(&source_leaf) {
                    scope_builder.add_binding(&target_leaf, node);
                }
            }
        }
        let mut projection_paths = Vec::new();
        if let Some(target_group) = schema_node_at(&target.schema, &target_path) {
            collect_matching_scalar_paths(
                target_group,
                target_group,
                &mut Vec::new(),
                &mut projection_paths,
            );
        }
        for relative in projection_paths {
            let Some(value_feed) = feed.projections.get(&relative) else {
                continue;
            };
            let mut target_leaf = target_path.clone();
            target_leaf.extend(relative.iter().cloned());
            if connected_bindings.contains(&target_leaf)
                || !schema_node_at(&target.schema, &target_leaf)
                    .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
            {
                continue;
            }
            if let Some(node) = builder.value_node(*value_feed) {
                scope_builder.add_binding(&target_leaf, node);
            }
        }
    }
    for (target_path, from) in bindings {
        let Some(node) = builder.value_node(from) else {
            builder.warnings.push(format!(
                "binding for `{}` comes from an unsupported feed; skipped",
                target_path.join("/")
            ));
            continue;
        };
        scope_builder.add_binding(&target_path, node);
    }

    let mut extra_sources = Vec::new();
    for extra in sources.iter().skip(1) {
        builder.warnings.push(format!(
            "extra source `{}` imported as a named source; cross-source \
             connections usually need manual lookup/scope fixes",
            extra.name
        ));
        extra_sources.push(NamedSource {
            name: extra.name.clone(),
            path: extra.input_instance.clone().unwrap_or_default(),
            schema: extra.schema.clone(),
            options: extra.options.clone(),
        });
    }

    warnings.extend(builder.warnings);
    Ok(Imported {
        project: Project {
            source: primary.schema.clone(),
            target: target.schema.clone(),
            source_path: primary.input_instance.clone(),
            target_path: target
                .output_instance
                .clone()
                .or_else(|| target.input_instance.clone()),
            source_options: primary.options.clone(),
            target_options: target.options.clone(),
            extra_sources,
            graph: builder.graph,
            root: scope_builder.root,
        },
        warnings,
    })
}

fn parse_u32(attr: Option<&str>) -> Option<u32> {
    attr.and_then(|a| a.parse().ok())
}

fn entry_key_sets(root: &roxmltree::Node) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for entry in root.descendants().filter(|node| node.has_tag_name("entry")) {
        if let Some(key) = parse_u32(entry.attribute("inpkey")) {
            inputs.insert(key);
        }
        if let Some(key) = parse_u32(entry.attribute("outkey")) {
            outputs.insert(key);
        }
    }
    (inputs, outputs)
}

/// Reads an xml schema component: entry tree, ports, and the schema itself
/// (from the referenced XSD when it resolves, else derived from entries).
fn read_schema_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let document = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "document");
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Prefer the payload below the synthetic `document` entry wherever it
    // appears. Variable components can put compute-when or parent-context
    // entries before/around it instead of using the ordinary
    // FileInstance/document wrapper.
    let document_entry = root_el
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"));
    let mut entry = document_entry
        .and_then(|document| document.children().find(|node| node.has_tag_name("entry")))
        .or_else(|| root_el.children().find(|node| node.has_tag_name("entry")))?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let (input_keys, output_keys) = entry_key_sets(&root_el);
    // The root entry's own port is a document-level connection.
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_entry_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
        warnings,
    );
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // Schema: prefer the referenced XSD (types + repeating info), picking
    // the top-level element the design says the document uses -- an XSD
    // can declare several document roots ("{ns}Local" strips to "Local").
    let instance_root: Vec<String> = document
        .and_then(|d| d.attribute("instanceroot"))
        .map(instance_root_segments)
        .unwrap_or_default();
    let schema = document
        .and_then(|d| d.attribute("schema"))
        .and_then(|rel| {
            let xsd_path = mfd_path.parent().unwrap_or(Path::new(".")).join(rel);
            match format_xml::xsd::import_root(&xsd_path, instance_root.first().map(String::as_str))
            {
                Ok(schema) => {
                    if instance_root.len() <= 1 {
                        Some(schema)
                    } else {
                        match schema_node_at(&schema, &instance_root[1..]).cloned() {
                            Some(nested) => Some(nested),
                            None => {
                                warnings.push(format!(
                                    "component `{name}`: instance root `{}` does not exist in \
                                     schema `{rel}`; falling back to the entry tree",
                                    instance_root.join("/")
                                ));
                                None
                            }
                        }
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({e}); \
                         falling back to the entry tree (no types, no repeating info)"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| entry_tree_schema(&entry));
    normalize_xml_text_ports(&schema, &mut ports);

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Xml,
        schema,
        input_instance: document
            .and_then(|d| d.attribute("inputinstance"))
            .map(str::to_string),
        output_instance: document
            .and_then(|d| d.attribute("outputinstance"))
            .map(str::to_string),
        options: FormatOptions::default(),
        is_source,
        is_variable: data.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("variable")
        }),
        compute_when_key: root_el
            .descendants()
            .find(|node| {
                node.has_tag_name("entry") && node.attribute("name") == Some("compute-when")
            })
            .and_then(|entry| parse_u32(entry.attribute("inpkey"))),
        ports,
        input_keys,
        output_keys,
    })
}

fn instance_root_segments(root: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_namespace = false;
    for (index, character) in root.char_indices() {
        match character {
            '{' => in_namespace = true,
            '}' => in_namespace = false,
            '/' if !in_namespace => {
                segments.push(&root[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(&root[start..]);
    segments
        .into_iter()
        .filter_map(|segment| segment.rsplit('}').next())
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

/// MapForce puts a simple-content value on its parent element's port. The
/// ferrule schema stores that scalar as a `#text` child, so normalize the
/// port path once at component import and let the graph/scope builders use
/// ordinary scalar paths afterward.
fn normalize_xml_text_ports(schema: &SchemaNode, ports: &mut BTreeMap<u32, Vec<String>>) {
    for path in ports.values_mut() {
        if let Some(text) = schema_node_at(schema, path).and_then(SchemaNode::text_child) {
            path.push(text.name.clone());
        }
    }
}

fn note_skipped_library(skipped: &mut Vec<String>, label: &str) {
    if !skipped.iter().any(|l| l == label) {
        skipped.push(label.to_string());
    }
}

/// Records an entry's own port keys under `path`.
fn record_entry_keys(
    entry: &roxmltree::Node,
    path: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
) {
    if let Some(key) = parse_u32(entry.attribute("outkey")) {
        *out_count += 1;
        ports.insert(key, path.to_vec());
    }
    if let Some(key) = parse_u32(entry.attribute("inpkey")) {
        *in_count += 1;
        ports.insert(key, path.to_vec());
    }
}

/// Reads a json component: schema from the referenced JSON Schema file
/// (entry tree as fallback), ports normalized so only `json-property`
/// entries contribute path segments -- which lines the paths up with the
/// property/array shapes `json_schema::import` produces.
fn read_json_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let json_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "json");
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Strip the synthetic FileInstance/document levels down to the JSON
    // document root wrapper (an entry conventionally named `root`).
    let mut entry = root_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

    if json_el.is_some_and(|j| j.attribute("jsonlines") == Some("1")) {
        warnings.push(format!(
            "component `{name}` uses JSON Lines; ferrule reads/writes it as \
             regular JSON, so instances need converting"
        ));
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_json_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
        warnings,
    );
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // Schema: prefer the referenced JSON Schema (types + repeating info).
    let schema = json_el
        .and_then(|j| j.attribute("schema"))
        .and_then(|rel| {
            let schema_path = mfd_path.parent().unwrap_or(Path::new(".")).join(rel);
            match format_json::json_schema::import(&schema_path) {
                Ok(schema) => Some(schema),
                Err(e) => {
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({e}); \
                         falling back to the entry tree"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| {
            if json_el.and_then(|j| j.attribute("schema")).is_none() {
                warnings.push(format!(
                    "component `{name}` has no schema reference; deriving the \
                     schema from the entry tree"
                ));
            }
            json_entry_value_schema(&name, &entry)
        });

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Json,
        schema,
        input_instance: json_el
            .and_then(|j| j.attribute("inputinstance"))
            .map(str::to_string),
        output_instance: json_el
            .and_then(|j| j.attribute("outputinstance"))
            .map(str::to_string),
        options: FormatOptions::default(),
        is_source,
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
    })
}

/// Walks a json component's entry tree. Only `json-property` entries push
/// a path segment; structural entries (`object`, `array`, `item`, type
/// leaves) are transparent, so a port lands on the path of the property
/// that contains it -- matching the [`SchemaNode`] tree, where a repeating
/// property carries its array's shape directly.
fn collect_json_ports(
    entry: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
    warnings: &mut Vec<String>,
) {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        match child.attribute("type") {
            Some("json-property") => {
                path.push(child.attribute("name").unwrap_or_default().to_string());
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
                path.pop();
            }
            Some("json-propertyname") | Some("json-subtype") => {
                if child.attribute("outkey").is_some()
                    || child.attribute("inpkey").is_some()
                    || child
                        .descendants()
                        .any(|d| d.attribute("outkey").is_some() || d.attribute("inpkey").is_some())
                {
                    warnings.push(format!(
                        "dynamic `{}` entries are not supported; connections \
                         under `{}` skipped",
                        child.attribute("type").unwrap_or_default(),
                        path.join("/")
                    ));
                }
            }
            _ => {
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports(&child, path, ports, out_count, in_count, warnings);
            }
        }
    }
}

/// Fallback JSON schema straight from the entry tree: `json-property`
/// children become fields, an enclosing `array` marks the property
/// repeating, type-leaf names give scalar types.
fn json_entry_value_schema(name: &str, entry: &roxmltree::Node) -> SchemaNode {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        if child.attribute("type").is_some() && child.attribute("type") != Some("json-item") {
            continue;
        }
        match child.attribute("name") {
            Some("object") => {
                let children = child
                    .children()
                    .filter(|n| {
                        n.is_element()
                            && n.tag_name().name() == "entry"
                            && n.attribute("type") == Some("json-property")
                    })
                    .map(|p| json_entry_value_schema(p.attribute("name").unwrap_or_default(), &p))
                    .collect();
                return SchemaNode::group(name, children);
            }
            Some("array") => {
                return json_entry_value_schema(name, &child).repeating();
            }
            Some("item") => {
                // An array's item wrapper: its child describes the value.
                return json_entry_value_schema(name, &child);
            }
            Some("number") => return SchemaNode::scalar(name, ir::ScalarType::Float),
            Some("integer") => return SchemaNode::scalar(name, ir::ScalarType::Int),
            Some("boolean") => return SchemaNode::scalar(name, ir::ScalarType::Bool),
            Some("string") | Some("null") => {
                return SchemaNode::scalar(name, ir::ScalarType::String);
            }
            _ => continue,
        }
    }
    SchemaNode::scalar(name, ir::ScalarType::String)
}

/// Reads a csv text component: flat schema and delimiter/header options
/// from the inline `<settings>`; the block entry's own port is the row
/// iteration (path `[]`), field entries map to `[field]`.
fn read_csv_component(
    component: &roxmltree::Node,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let text_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "text")?;
    let settings = text_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "settings");
    let names_el = settings.and_then(|s| {
        s.children()
            .find(|n| n.is_element() && n.tag_name().name() == "names")
    });

    // The entry tree is FileInstance > document > block(fields).
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;
    let mut block = root_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    while matches!(
        block.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        block = block
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

    let fields: Vec<SchemaNode> = names_el
        .map(|names| {
            names
                .children()
                .filter(|n| n.is_element() && n.tag_name().name().starts_with("field"))
                .map(|f| {
                    let field_name = f.attribute("name").unwrap_or_default();
                    let ty = match f.attribute("type") {
                        Some("number") | Some("decimal") | Some("double") | Some("float") => {
                            ir::ScalarType::Float
                        }
                        Some("integer") | Some("int") => ir::ScalarType::Int,
                        Some("boolean") => ir::ScalarType::Bool,
                        _ => ir::ScalarType::String,
                    };
                    SchemaNode::scalar(field_name, ty)
                })
                .collect()
        })
        .unwrap_or_default();
    if fields.is_empty() {
        warnings.push(format!(
            "csv component `{name}` declares no fields; skipped"
        ));
        return None;
    }
    let root_name = names_el
        .and_then(|n| n.attribute("root"))
        .filter(|r| !r.is_empty())
        .unwrap_or(&name);
    let schema = SchemaNode::group(root_name, fields);

    let mut options = FormatOptions::default();
    if let Some(settings) = settings {
        if let Some(separator) = settings.attribute("separator") {
            let mut chars = separator.chars();
            options.delimiter = chars.next();
            if chars.next().is_some() {
                warnings.push(format!(
                    "csv component `{name}`: multi-character separator \
                     `{separator}` truncated to its first character"
                ));
            }
        }
        options.has_header_row = Some(settings.attribute("firstrownames") == Some("true"));
        if let Some(quote) = settings.attribute("quote")
            && !quote.is_empty()
            && quote != "\""
        {
            warnings.push(format!(
                "csv component `{name}`: quote character `{quote}` is not \
                 supported (ferrule always quotes with `\"`)"
            ));
        }
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    record_entry_keys(&block, &[], &mut ports, &mut out_count, &mut in_count);
    for field_entry in block
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        let field_name = field_entry.attribute("name").unwrap_or_default();
        record_entry_keys(
            &field_entry,
            &[field_name.to_string()],
            &mut ports,
            &mut out_count,
            &mut in_count,
        );
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Csv,
        schema,
        input_instance: text_el.attribute("inputinstance").map(str::to_string),
        output_instance: text_el.attribute("outputinstance").map(str::to_string),
        options,
        is_source,
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
    })
}

fn collect_entry_ports(
    entry: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
    warnings: &mut Vec<String>,
) {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        let name = child.attribute("name").unwrap_or_default();
        if name == "element()" || child.attribute("type") == Some("xml-type") {
            if name == "element()" {
                warnings.push(
                    "generic element() entries are not supported; subtree skipped".to_string(),
                );
                continue;
            }
            // xml-type entries are transparent type wrappers: descend
            // without extending the path.
            collect_entry_ports(&child, path, ports, out_count, in_count, warnings);
            continue;
        }
        path.push(name.to_string());
        if let Some(key) = parse_u32(child.attribute("outkey")) {
            *out_count += 1;
            ports.insert(key, path.clone());
        }
        if let Some(key) = parse_u32(child.attribute("inpkey")) {
            *in_count += 1;
            ports.insert(key, path.clone());
        }
        collect_entry_ports(&child, path, ports, out_count, in_count, warnings);
        path.pop();
    }
}

/// Fallback schema straight from the entry tree: groups where there are
/// children, string scalars at the leaves (attribute entries flagged as
/// such), no repeating flags.
fn entry_tree_schema(entry: &roxmltree::Node) -> SchemaNode {
    let name = entry.attribute("name").unwrap_or("root");
    if entry.attribute("type") == Some("attribute") {
        return SchemaNode::scalar(name, ir::ScalarType::String).attribute();
    }
    let children: Vec<SchemaNode> = entry
        .children()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "entry"
                && n.attribute("name") != Some("element()")
        })
        .map(|c| entry_tree_schema(&c))
        .collect();
    if children.is_empty() {
        SchemaNode::scalar(name, ir::ScalarType::String)
    } else {
        SchemaNode::group(name, children)
    }
}

/// Reads a single-table database component: the table entry's own port is
/// the row iteration (path `[]`, like a csv block), column entries map to
/// `[column]`, and the schema comes from introspecting the referenced
/// SQLite file when it exists (untyped column names otherwise). Components
/// with several tables, nested (foreign-key) tables, or SQL statements
/// are skipped with a warning -- ferrule's db adapter is whole-table.
fn read_db_component(
    component: &roxmltree::Node,
    mapping_el: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Descend through the synthetic FileInstance/document levels to the
    // level whose entries are the tables.
    let mut container = root_el;
    loop {
        let mut entries = container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "entry");
        let (first, second) = (entries.next(), entries.next());
        match (first, second) {
            (Some(entry), None)
                if matches!(
                    entry.attribute("name"),
                    Some("FileInstance") | Some("document")
                ) =>
            {
                container = entry;
            }
            _ => break,
        }
    }
    let tables: Vec<roxmltree::Node> = container
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
        .collect();
    let single_plain_table = tables.len() == 1
        && tables[0].attribute("type") == Some("table")
        && !tables[0]
            .descendants()
            .skip(1)
            .any(|n| n.attribute("type") == Some("table"));
    if !single_plain_table {
        warnings.push(format!(
            "skipped database component `{name}`: only single-table components \
             import (nested tables and SQL statements need manual conversion)"
        ));
        return None;
    }
    let table = tables[0];
    let table_name = table.attribute("name").unwrap_or_default().to_string();

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    record_entry_keys(&table, &[], &mut ports, &mut out_count, &mut in_count);
    for column in table
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        let column_name = column.attribute("name").unwrap_or_default();
        record_entry_keys(
            &column,
            &[column_name.to_string()],
            &mut ports,
            &mut out_count,
            &mut in_count,
        );
    }
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // The connection string lives in the mapping's datasource registry,
    // linked from the component by name.
    let connection = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "database")
        .and_then(|db| db.attribute("ref"))
        .and_then(|r| {
            mapping_el
                .descendants()
                .find(|n| n.has_tag_name("database_connection") && n.attribute("name") == Some(r))
        })
        .and_then(|c| c.attribute("ConnectionString"))
        .map(str::to_string);
    if connection.is_none() {
        warnings.push(format!(
            "database component `{name}` has no resolvable connection; the \
             project needs an instance path filled in"
        ));
    }

    // Schema: introspect the SQLite file when it is reachable (types),
    // else fall back to the column entries (untyped).
    let schema = connection
        .as_deref()
        .and_then(|conn| {
            let db_path = mfd_path.parent().unwrap_or(Path::new(".")).join(conn);
            if !db_path.exists() {
                warnings.push(format!(
                    "component `{name}`: database `{conn}` not found next to the \
                     design; falling back to untyped columns"
                ));
                return None;
            }
            match format_db::introspect(&db_path, &table_name) {
                Ok(schema) => Some(schema),
                Err(e) => {
                    warnings.push(format!(
                        "component `{name}`: could not introspect `{conn}` ({e}); \
                         falling back to untyped columns"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| {
            let columns = table
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "entry")
                .map(|c| {
                    SchemaNode::scalar(
                        c.attribute("name").unwrap_or_default(),
                        ir::ScalarType::String,
                    )
                })
                .collect();
            // Tables are repeating groups by format-db convention.
            SchemaNode::group(&table_name, columns).repeating()
        });

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Db,
        schema,
        input_instance: connection.clone(),
        output_instance: connection,
        options: FormatOptions::default(),
        is_source,
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys: BTreeSet::new(),
        output_keys: BTreeSet::new(),
    })
}

fn read_fn_component(component: &roxmltree::Node) -> FnComponent {
    let library = component
        .attribute("library")
        .unwrap_or_default()
        .to_string();
    let name = component.attribute("name").unwrap_or_default().to_string();
    let kind = parse_u32(component.attribute("kind")).unwrap_or(0);
    let pins = |tag: &str| -> Vec<Option<u32>> {
        component
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .map(|pins| {
                pins.children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "datapoint")
                    .map(|d| parse_u32(d.attribute("key")))
                    .collect()
            })
            .unwrap_or_default()
    };
    let inputs = pins("sources");
    let outputs = pins("targets").into_iter().flatten().collect();

    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data");
    let constant = data
        .and_then(|d| {
            d.children()
                .find(|n| n.is_element() && n.tag_name().name() == "constant")
        })
        .map(|c| {
            (
                c.attribute("value").unwrap_or_default().to_string(),
                c.attribute("datatype").unwrap_or_default().to_string(),
            )
        });
    let valuemap = data
        .and_then(|d| {
            d.children()
                .find(|n| n.is_element() && n.tag_name().name() == "valuemap")
        })
        .map(|vm| {
            let table = vm
                .descendants()
                .filter(|n| n.has_tag_name("entry"))
                .map(|e| {
                    (
                        e.attribute("from").unwrap_or_default().to_string(),
                        e.attribute("to").unwrap_or_default().to_string(),
                    )
                })
                .collect();
            let default = vm
                .descendants()
                .find(|n| n.has_tag_name("result"))
                .and_then(|r| r.attribute("defaultValue"))
                .map(str::to_string)
                .filter(|_| vm.attribute("defaultValueMode") == Some("custom"));
            (table, default)
        });
    let sort_descending = data
        .and_then(|data| data.descendants().find(|node| node.has_tag_name("sort")))
        .map(|sort| {
            sort.descendants()
                .find(|node| node.has_tag_name("key"))
                .is_some_and(|key| key.attribute("direction") == Some("descending"))
        });

    FnComponent {
        library,
        name,
        kind,
        inputs,
        outputs,
        constant,
        valuemap,
        sort_descending,
    }
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        node = node.child(segment)?;
    }
    Some(node)
}

fn collect_matching_scalar_paths(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    match (&source.kind, &target.kind) {
        (SchemaKind::Scalar { .. }, SchemaKind::Scalar { .. }) => paths.push(path.clone()),
        (SchemaKind::Group { .. }, SchemaKind::Group { children }) => {
            for target_child in children {
                if target_child.repeating {
                    continue;
                }
                let Some(source_child) = source.child(&target_child.name) else {
                    continue;
                };
                path.push(target_child.name.clone());
                collect_matching_scalar_paths(source_child, target_child, path, paths);
                path.pop();
            }
        }
        _ => {}
    }
}

fn is_filter_component(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 3
}

fn is_input_component(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 6
}

fn is_sort_component(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 30 && component.sort_descending.is_some()
}

fn is_first_items_component(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "first-items"
}

struct GraphBuilder<'a> {
    graph: Graph,
    next_id: NodeId,
    fn_nodes: BTreeMap<usize, NodeId>,
    source_fields: BTreeMap<(Option<Vec<String>>, Vec<String>), NodeId>,
    edge_from: &'a BTreeMap<u32, u32>,
    sources: &'a [&'a SchemaComponent],
    intermediates: &'a [&'a SchemaComponent],
    fn_components: &'a [FnComponent],
    fn_by_output: BTreeMap<u32, usize>,
    /// Absolute source paths ending at a repeating node that some scope's
    /// iteration crosses -- i.e. levels that get their own context frame
    /// at run time. SourceField paths are cut after the innermost framed
    /// ancestor; repeating levels no scope iterates stay in the path (the
    /// engine reads their first item).
    framed: std::collections::BTreeSet<Vec<String>>,
    warnings: Vec<String>,
}

impl GraphBuilder<'_> {
    fn alloc(&mut self, node: Node) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.graph.nodes.insert(id, node);
        id
    }

    fn const_null(&mut self) -> NodeId {
        self.alloc(Node::Const { value: Value::Null })
    }

    fn source_field(&mut self, frame: Option<Vec<String>>, path: Vec<String>) -> NodeId {
        let key = (frame.clone(), path.clone());
        let id = *self.source_fields.entry(key).or_insert_with_key(|_| {
            let id = self.next_id;
            self.next_id += 1;
            id
        });
        self.graph
            .nodes
            .entry(id)
            .or_insert(Node::SourceField { path, frame });
        id
    }

    fn primary_source_field(&mut self, abs: &[String]) -> Option<NodeId> {
        let schema = &self.sources.first()?.schema;
        let path = self.suffix_after_framed(schema, abs);
        let frame = self.frame_for_field(schema, abs);
        Some(self.source_field(frame, path))
    }

    /// Marks every repeating level along an iterated absolute source path
    /// as getting a run-time context frame.
    fn note_framed_prefixes(&mut self, abs: &[String]) {
        let Some(source) = self.sources.first() else {
            return;
        };
        let mut node = &source.schema;
        for (i, segment) in abs.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating {
                self.framed.insert(abs[..=i].to_vec());
            }
            node = child;
        }
    }

    /// Path segments after the innermost framed (scope-iterated) repeating
    /// ancestor -- what a `SourceField` must hold so it resolves against
    /// the enclosing scopes' iteration frames.
    fn suffix_after_framed(&self, schema: &SchemaNode, abs: &[String]) -> Vec<String> {
        let mut node = schema;
        let mut suffix_start = 0;
        for (i, segment) in abs.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating && self.framed.contains(&abs[..=i]) {
                suffix_start = i + 1;
            }
            node = child;
        }
        abs[suffix_start..].to_vec()
    }

    fn frame_for_field(&self, schema: &SchemaNode, abs: &[String]) -> Option<Vec<String>> {
        let mut node = schema;
        let mut frame = None;
        for (i, segment) in abs.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating && self.framed.contains(&abs[..=i]) {
                frame = Some(abs[..=i].to_vec());
            }
            node = child;
        }
        frame
    }

    /// Resolves one output of a variable schema component to the connected
    /// input that supplies it plus the output's path below that input. An
    /// Connected descendant inputs are returned as scalar projections so a
    /// constructed group can become ordinary target bindings.
    fn intermediate_feed(&self, output_key: u32) -> Option<IntermediateFeed> {
        for component in self.intermediates {
            if !component.output_keys.contains(&output_key) {
                continue;
            }
            let output_path = component.ports.get(&output_key)?;
            let (input_key, input_path) = component
                .ports
                .iter()
                .filter(|(key, path)| {
                    component.input_keys.contains(key)
                        && self.edge_from.contains_key(key)
                        && output_path.starts_with(path)
                })
                .max_by_key(|(_, path)| path.len())?;
            let feed = *self.edge_from.get(input_key)?;
            let control = component
                .compute_when_key
                .and_then(|key| self.edge_from.get(&key).copied());
            let projections = component
                .ports
                .iter()
                .filter_map(|(key, path)| {
                    if component.input_keys.contains(key)
                        && path.len() > output_path.len()
                        && path.starts_with(output_path)
                    {
                        self.edge_from
                            .get(key)
                            .map(|feed| (path[output_path.len()..].to_vec(), *feed))
                    } else {
                        None
                    }
                })
                .collect();
            return Some(IntermediateFeed {
                feed,
                suffix: output_path[input_path.len()..].to_vec(),
                control,
                projections,
            });
        }
        None
    }

    /// The ferrule node producing the value at output-port `key`, creating
    /// SourceField/function nodes on demand. `None` for unsupported feeds.
    fn value_node(&mut self, key: u32) -> Option<NodeId> {
        // A source schema entry?
        for (idx, source) in self.sources.iter().enumerate() {
            if let Some(abs) = source.ports.get(&key).cloned() {
                if idx == 0 {
                    return self.primary_source_field(&abs);
                }
                // Extra sources are addressed by name from the outermost
                // context frame.
                let mut path = vec![self.sources[idx].name.clone()];
                path.extend(abs);
                return Some(self.source_field(None, path));
            }
        }
        // A transparent output of a variable schema component?
        if let Some(intermediate) = self.intermediate_feed(key) {
            if intermediate.suffix.is_empty() {
                return self.value_node(intermediate.feed);
            }
            let mut abs = self.sequence_source_path(intermediate.feed)?;
            abs.extend(intermediate.suffix);
            return self.primary_source_field(&abs);
        }
        // A function output?
        let idx = *self.fn_by_output.get(&key)?;
        if is_filter_component(&self.fn_components[idx]) {
            // A filter feeding a value position is pass-through of its
            // node input for our purposes; treat the value as whatever
            // feeds the filter's first input.
            let feed = self.input_feed(idx, 0)?;
            return self.value_node(feed);
        }
        if is_input_component(&self.fn_components[idx]) {
            return match self.input_feed(idx, 0) {
                Some(feed) => self.value_node(feed),
                None => Some(self.const_null()),
            };
        }
        match self.fn_components[idx].name.as_str() {
            // A group-by's key output is the key expression itself
            // (re-evaluated in the group's context it reads the group's
            // shared key); its groups output passes the nodes through.
            "group-by" => {
                let pos = if self.fn_components[idx].outputs.get(1) == Some(&key) {
                    1
                } else {
                    0
                };
                let feed = self.input_feed(idx, pos)?;
                self.value_node(feed)
            }
            _ => Some(self.fn_node(idx)),
        }
    }

    /// Materializes function component `idx` as a mapping node.
    fn fn_node(&mut self, idx: usize) -> NodeId {
        if let Some(&id) = self.fn_nodes.get(&idx) {
            return id;
        }
        // Reserve the id first so cycles cannot recurse forever.
        let id = self.next_id;
        self.next_id += 1;
        self.fn_nodes.insert(idx, id);

        // Aggregates take a sequence connection, not scalar arguments, so
        // they must not materialize their feeds as SourceFields.
        let name = self.fn_components[idx].name.clone();
        if let Some(op) = aggregate_op(&name).filter(|_| self.fn_components[idx].kind == 5) {
            let node = match self.aggregate_node(op, idx) {
                Some(node) => node,
                None => {
                    self.warnings.push(format!(
                        "aggregate `{name}` has an unresolvable sequence input; \
                         imported as a plain call and will fail at run time until \
                         replaced"
                    ));
                    let args = (0..self.fn_components[idx].inputs.len().max(1))
                        .map(|_| self.const_null())
                        .collect();
                    Node::Call {
                        function: name,
                        args,
                    }
                }
            };
            self.graph.nodes.insert(id, node);
            return id;
        }
        if name == "position" && self.fn_components[idx].kind == 5 {
            let collection = self.position_collection(idx);
            self.graph.nodes.insert(id, Node::Position { collection });
            return id;
        }
        let fc = &self.fn_components[idx];

        let mut input_ids = Vec::with_capacity(fc.inputs.len());
        for input in fc.inputs.clone() {
            let feed = input.and_then(|k| self.edge_from.get(&k).copied());
            let node = feed.and_then(|from| self.value_node(from));
            input_ids.push(node);
        }
        let input_or_null = |builder: &mut Self, i: usize| {
            input_ids
                .get(i)
                .copied()
                .flatten()
                .unwrap_or_else(|| builder.const_null())
        };

        let node = match (fc.name.as_str(), fc.kind) {
            ("constant", _) => {
                let (value, datatype) = fc.constant.clone().unwrap_or_default();
                Node::Const {
                    value: parse_constant(&value, &datatype),
                }
            }
            ("if-else", _) => Node::If {
                condition: input_or_null(self, 0),
                then: input_or_null(self, 1),
                else_: input_or_null(self, 2),
            },
            ("value-map", _) => {
                let (table, default) = fc.valuemap.clone().unwrap_or_default();
                Node::ValueMap {
                    input: input_or_null(self, 0),
                    table: table
                        .into_iter()
                        .map(|(f, t)| (Value::String(f), Value::String(t)))
                        .collect(),
                    default: default.map(Value::String),
                }
            }
            (name, _) => {
                let function = match map_function_name(name) {
                    Some(mapped) => mapped.to_string(),
                    None => {
                        self.warnings.push(format!(
                            "function `{name}` has no ferrule equivalent; imported \
                             as-is and will fail at run time until replaced"
                        ));
                        name.to_string()
                    }
                };
                let args = (0..fc.inputs.len().max(1))
                    .map(|i| input_or_null(self, i))
                    .collect();
                Node::Call { function, args }
            }
        };
        self.graph.nodes.insert(id, node);
        id
    }

    /// Converts an aggregate function component into a [`Node::Aggregate`].
    /// The connected inputs split into source-entry feeds (sequence and,
    /// optionally, an explicit parent-context before it) and scalar feeds
    /// (join's separator, item-at's position). `None` when no input
    /// resolves to a source entry.
    fn aggregate_node(&mut self, op: AggregateOp, idx: usize) -> Option<Node> {
        let fc = &self.fn_components[idx];
        let source_schema = self.sources.first()?.schema.clone();
        let sequence_feed = self.input_feed(idx, 1).or_else(|| {
            (fc.inputs.len() == 1)
                .then(|| self.input_feed(idx, 0))
                .flatten()
        })?;

        let (collection_abs, value, expression) =
            if let Some(path) = self.sequence_source_path(sequence_feed) {
                let (collection, value) = split_at_innermost_repeating(&source_schema, &path);
                (collection, value, None)
            } else {
                let mut dependencies = self.sequence_dependency_paths(sequence_feed);
                if let Some(context) = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_source_path(feed))
                {
                    dependencies.push(context);
                }
                let collection = compatible_collection(&source_schema, &dependencies)?;
                let expression = self.value_node_in_collection(sequence_feed, &collection)?;
                (collection, Vec::new(), Some(expression))
            };

        let collection = match collection_abs.split_last() {
            Some((last, prefix)) => {
                let mut relative = self.suffix_after_framed(&source_schema, prefix);
                relative.push(last.clone());
                relative
            }
            None => Vec::new(),
        };
        let arg = self
            .input_feed(idx, 2)
            .and_then(|feed| self.value_node(feed));
        Some(Node::Aggregate {
            function: op,
            collection,
            value,
            expression,
            arg,
        })
    }

    /// Source leaves used by a computed sequence expression. Aggregating
    /// that expression iterates the deepest collection shared by the leaves;
    /// outer leaves broadcast through the engine's normal context fallback.
    fn sequence_dependency_paths(&self, feed: u32) -> Vec<Vec<String>> {
        fn visit(
            builder: &GraphBuilder<'_>,
            feed: u32,
            visited: &mut std::collections::BTreeSet<u32>,
            paths: &mut Vec<Vec<String>>,
        ) {
            if !visited.insert(feed) {
                return;
            }
            if let Some(path) = builder
                .sources
                .first()
                .and_then(|source| source.ports.get(&feed))
            {
                paths.push(path.clone());
                return;
            }
            let Some(&idx) = builder.fn_by_output.get(&feed) else {
                return;
            };
            let component = &builder.fn_components[idx];
            if aggregate_op(&component.name).is_some() && component.kind == 5 {
                return;
            }
            for key in component.inputs.iter().flatten() {
                if let Some(&input_feed) = builder.edge_from.get(key) {
                    visit(builder, input_feed, visited, paths);
                }
            }
        }

        let mut paths = Vec::new();
        visit(
            self,
            feed,
            &mut std::collections::BTreeSet::new(),
            &mut paths,
        );
        paths
    }

    fn position_collection(&self, idx: usize) -> Vec<String> {
        let Some(source) = self.sources.first() else {
            return Vec::new();
        };
        let Some(path) = self
            .input_feed(idx, 0)
            .and_then(|feed| self.sequence_source_path(feed))
        else {
            return Vec::new();
        };
        let collection_abs = split_at_innermost_repeating(&source.schema, &path).0;
        match collection_abs.split_last() {
            Some((last, prefix)) => {
                let mut relative = self.suffix_after_framed(&source.schema, prefix);
                relative.push(last.clone());
                relative
            }
            None => Vec::new(),
        }
    }

    /// The feed of pin `pos` on function component `idx`, if connected.
    fn input_feed(&self, idx: usize, pos: usize) -> Option<u32> {
        self.fn_components[idx]
            .inputs
            .get(pos)
            .copied()
            .flatten()
            .and_then(|k| self.edge_from.get(&k).copied())
    }

    /// Materializes an expression with `collection` treated as an iteration
    /// frame, then restores the scope-derived frame set for other nodes.
    fn value_node_in_collection(&mut self, key: u32, collection: &[String]) -> Option<NodeId> {
        let inserted = !collection.is_empty() && self.framed.insert(collection.to_vec());
        let node = self.value_node(key);
        if inserted {
            self.framed.remove(collection);
        }
        node
    }

    /// Follows an iteration feed through `filter` and `group-by`
    /// components back to the underlying source entry, collecting the
    /// filter's boolean expression and the group-by's key expression on
    /// the way.
    fn resolve_iteration_feed(&self, from: u32) -> IterationFeed {
        self.resolve_iteration_feed_inner(from, 0)
    }

    fn resolve_iteration_feed_inner(&self, mut from: u32, depth: usize) -> IterationFeed {
        let mut filter_expr = None;
        let mut group_key = None;
        let mut sort_expr = None;
        let mut sort_descending = false;
        let mut take_expr = None;
        let mut take_default_one = false;
        let mut projects_whole_group = false;
        let mut projections = BTreeMap::new();
        let mut source_suffix = Vec::new();
        // Chains are short; the bound only guards against odd cycles.
        for _ in 0..12 {
            if let Some(intermediate) = self.intermediate_feed(from) {
                projects_whole_group |= intermediate.suffix.is_empty();
                projections.extend(intermediate.projections);
                if let Some(control) = intermediate.control
                    && depth < 12
                {
                    let control = self.resolve_iteration_feed_inner(control, depth + 1);
                    filter_expr = filter_expr.or(control.filter_expr);
                    group_key = group_key.or(control.group_key);
                    if sort_expr.is_none() && control.sort_expr.is_some() {
                        sort_expr = control.sort_expr;
                        sort_descending = control.sort_descending;
                    }
                    take_expr = take_expr.or(control.take_expr);
                    take_default_one |= control.take_default_one;
                }
                let mut suffix = intermediate.suffix;
                suffix.extend(source_suffix);
                source_suffix = suffix;
                from = intermediate.feed;
                continue;
            }
            let Some(&idx) = self.fn_by_output.get(&from) else {
                break;
            };
            let fc = &self.fn_components[idx];
            if is_filter_component(fc) {
                let Some(node_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                filter_expr = filter_expr.or_else(|| self.input_feed(idx, 1));
                from = node_feed;
            } else if is_sort_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                if sort_expr.is_none() {
                    sort_expr = self.input_feed(idx, 1);
                    sort_descending = fc.sort_descending.unwrap_or(false);
                }
                from = nodes_feed;
            } else if is_first_items_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                // A variable driven by group-by uses first-items to select
                // the first member inside each group. Grouped scope frames
                // already expose that member to scalar bindings, so an
                // outer item limit would incorrectly truncate the groups.
                if group_key.is_none() && take_expr.is_none() && !take_default_one {
                    take_expr = self.input_feed(idx, 1);
                    take_default_one = take_expr.is_none();
                }
                from = nodes_feed;
            } else {
                match fc.name.as_str() {
                    "group-by" if fc.outputs.first() == Some(&from) => {
                        let Some(nodes_feed) = self.input_feed(idx, 0) else {
                            break;
                        };
                        group_key = group_key.or_else(|| self.input_feed(idx, 1));
                        from = nodes_feed;
                    }
                    _ => break,
                }
            }
        }
        IterationFeed {
            source_key: from,
            source_suffix,
            filter_expr,
            group_key,
            sort_expr,
            sort_descending,
            take_expr,
            take_default_one,
            projects_whole_group,
            projections,
        }
    }

    /// Follows filter/group-by pass-throughs to the primary-source entry a
    /// sequence connection ultimately reads, for aggregates.
    fn sequence_source_path(&self, mut feed: u32) -> Option<Vec<String>> {
        let mut suffix = Vec::new();
        for _ in 0..12 {
            if let Some(abs) = self.sources.first()?.ports.get(&feed) {
                let mut path = abs.clone();
                path.extend(suffix);
                return Some(path);
            }
            if let Some(intermediate) = self.intermediate_feed(feed) {
                let mut intermediate_suffix = intermediate.suffix;
                intermediate_suffix.extend(suffix);
                suffix = intermediate_suffix;
                feed = intermediate.feed;
                continue;
            }
            let &idx = self.fn_by_output.get(&feed)?;
            let fc = &self.fn_components[idx];
            if is_filter_component(fc) || is_sort_component(fc) || is_first_items_component(fc) {
                feed = self.input_feed(idx, 0)?;
            } else {
                match fc.name.as_str() {
                    "group-by" if fc.outputs.first() == Some(&feed) => {
                        feed = self.input_feed(idx, 0)?;
                    }
                    _ => return None,
                }
            }
        }
        None
    }

    fn iteration_source_path(&self, feed: &IterationFeed) -> Option<Vec<String>> {
        let mut path = self.source_abs_path(feed.source_key)?;
        path.extend(feed.source_suffix.iter().cloned());
        Some(path)
    }

    /// The absolute source path behind output-port `key` on the primary
    /// source, if that is what it is.
    fn source_abs_path(&self, key: u32) -> Option<Vec<String>> {
        self.sources.first()?.ports.get(&key).cloned()
    }
}

fn parse_constant(value: &str, datatype: &str) -> Value {
    match datatype {
        "integer" | "int" | "long" => value.parse().map(Value::Int).unwrap_or(Value::Null),
        "decimal" | "double" | "float" => value.parse().map(Value::Float).unwrap_or(Value::Null),
        "boolean" => value.parse().map(Value::Bool).unwrap_or(Value::Null),
        _ => Value::String(value.to_string()),
    }
}

fn aggregate_op(name: &str) -> Option<AggregateOp> {
    Some(match name {
        "count" => AggregateOp::Count,
        "sum" => AggregateOp::Sum,
        "avg" => AggregateOp::Avg,
        "min" => AggregateOp::Min,
        "max" => AggregateOp::Max,
        "string-join" => AggregateOp::Join,
        "item-at" => AggregateOp::ItemAt,
        _ => return None,
    })
}

/// Splits an absolute source path at its innermost repeating node: the
/// collection is everything up to and including it, the value the rest.
/// With no repeating node the collection is empty -- flat-rows sources
/// (csv/db) hold their repetition outside the schema.
fn split_at_innermost_repeating(schema: &SchemaNode, abs: &[String]) -> (Vec<String>, Vec<String>) {
    let mut node = schema;
    let mut cut = None;
    for (i, segment) in abs.iter().enumerate() {
        let Some(child) = node.child(segment) else {
            break;
        };
        if child.repeating {
            cut = Some(i);
        }
        node = child;
    }
    match cut {
        Some(i) => (abs[..=i].to_vec(), abs[i + 1..].to_vec()),
        None => (Vec::new(), abs.to_vec()),
    }
}

/// Picks the deepest repeated collection used by a computed expression,
/// provided every other dependency belongs to that collection or one of its
/// enclosing contexts. Empty collections represent flat row sources.
fn compatible_collection(schema: &SchemaNode, paths: &[Vec<String>]) -> Option<Vec<String>> {
    if paths.is_empty() {
        return None;
    }
    let collections: Vec<Vec<String>> = paths
        .iter()
        .map(|path| split_at_innermost_repeating(schema, path).0)
        .collect();
    let deepest = collections.iter().max_by_key(|path| path.len())?.clone();
    collections
        .iter()
        .all(|path| deepest.starts_with(path))
        .then_some(deepest)
}

fn map_function_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "concat" => "concat",
        "add" => "add",
        "subtract" => "subtract",
        "multiply" => "multiply",
        "divide" => "divide",
        "equal" => "equal",
        "not-equal" => "not_equal",
        "greater" => "greater_than",
        "less" => "less_than",
        "greater-equal" | "greater-or-equal" | "equal-or-greater" => "greater_or_equal",
        "less-equal" | "less-or-equal" | "equal-or-less" => "less_or_equal",
        "logical-and" => "and",
        "logical-or" => "or",
        "logical-not" => "not",
        "string-length" => "length",
        "contains" => "contains",
        "starts-with" => "starts_with",
        "upper-case" => "upper",
        "lower-case" => "lower",
        "trim" => "trim",
        "left-trim" => "left_trim",
        "right-trim" => "right_trim",
        "pad-string-left" => "pad_string_left",
        "pad-string-right" => "pad_string_right",
        "substring" => "substring",
        "substring-before" => "substring_before",
        "substring-after" => "substring_after",
        "exists" => "exists",
        "round" | "round-precision" => "round",
        "date-from-datetime" => "date_from_datetime",
        _ => return None,
    })
}

/// Builds the scope tree from iteration and binding connections. `anchors`
/// remembers, per scope chain, the absolute source path its iteration
/// starts from, so nested iterations can be expressed relative to it.
struct ScopeBuilder {
    root: Scope,
    anchors: BTreeMap<Vec<String>, Vec<String>>,
}

struct IterationNodes {
    filter: Option<NodeId>,
    group_by: Option<NodeId>,
    sort_by: Option<NodeId>,
    sort_descending: bool,
    take: Option<NodeId>,
}

impl ScopeBuilder {
    fn ensure_scope(&mut self, chain: &[String]) -> &mut Scope {
        let mut scope = &mut self.root;
        for element in chain {
            let idx = match scope
                .children
                .iter()
                .position(|c| c.target_field == *element)
            {
                Some(idx) => idx,
                None => {
                    scope.children.push(Scope {
                        target_field: element.clone(),
                        ..Scope::default()
                    });
                    scope.children.len() - 1
                }
            };
            scope = &mut scope.children[idx];
        }
        scope
    }

    /// The nearest enclosing anchor for a chain, if any iteration exists
    /// above it.
    fn enclosing_anchor(&self, chain: &[String]) -> Vec<String> {
        for len in (0..chain.len()).rev() {
            if let Some(anchor) = self.anchors.get(&chain[..len]) {
                return anchor.clone();
            }
        }
        Vec::new()
    }

    fn add_iteration(
        &mut self,
        target_path: &[String],
        source_abs: &[String],
        nodes: IterationNodes,
    ) {
        let anchor = self.enclosing_anchor(target_path);
        let relative: Vec<String> = if source_abs.starts_with(&anchor) {
            source_abs[anchor.len()..].to_vec()
        } else {
            source_abs.to_vec()
        };
        self.anchors
            .insert(target_path.to_vec(), source_abs.to_vec());
        let scope = self.ensure_scope(target_path);
        scope.source = Some(relative);
        scope.filter = nodes.filter;
        scope.group_by = nodes.group_by;
        scope.sort_by = nodes.sort_by;
        scope.sort_descending = nodes.sort_descending;
        scope.take = nodes.take;
    }

    fn add_binding(&mut self, target_path: &[String], node: NodeId) {
        let (field, chain) = target_path.split_last().expect("leaf path is never empty");
        let scope = self.ensure_scope(chain);
        scope.bindings.push(Binding {
            target_field: field.clone(),
            node,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::instance_root_segments;

    #[test]
    fn instance_root_paths_do_not_split_namespace_uris() {
        assert_eq!(
            instance_root_segments(
                "{http://example.com/people}People/{http://example.com/people}Person"
            ),
            ["People", "Person"]
        );
        assert_eq!(
            instance_root_segments("{}People/{}Person"),
            ["People", "Person"]
        );
    }
}
