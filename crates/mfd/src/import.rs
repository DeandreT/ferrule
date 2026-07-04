//! `.mfd` -> `mapping::Project` conversion.
//!
//! The importer never fails on unsupported constructs: it converts what it
//! can and records a warning per skipped piece, because a partial import
//! the user finishes by hand still beats redrawing the mapping.

use std::collections::BTreeMap;
use std::path::Path;

use ir::{SchemaKind, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, NamedSource, Node, NodeId, Project, Scope};

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
    /// Port key -> absolute entry path (segments below the schema root).
    ports: BTreeMap<u32, Vec<String>>,
}

/// One function component's extracted facts.
type ValueMapData = (Vec<(String, String)>, Option<String>);

struct FnComponent {
    name: String,
    kind: u32,
    /// Input pins in `pos` order; `None` for declared-but-keyless pins.
    inputs: Vec<Option<u32>>,
    /// Output pin keys in `pos` order.
    outputs: Vec<u32>,
    constant: Option<(String, String)>,
    valuemap: Option<ValueMapData>,
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
                "db" => {
                    let connection = component
                        .descendants()
                        .find(|n| n.has_tag_name("database_connection"))
                        .and_then(|c| c.attribute("ConnectionString"))
                        .map(|c| format!(" ({c})"))
                        .unwrap_or_default();
                    note_skipped_library(&mut skipped_libraries, "db");
                    warnings.push(format!(
                        "skipped database component `{name}`{connection}: ferrule \
                         cannot import db components yet"
                    ));
                }
                "core" | "lang" => fn_components.push(read_fn_component(&component)),
                other => {
                    note_skipped_library(&mut skipped_libraries, other);
                    warnings.push(format!(
                        "skipped component `{name}`: unsupported library `{other}` \
                         (only xml/json/csv and core/lang function components import)"
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

    let sources: Vec<&SchemaComponent> = schema_components.iter().filter(|c| c.is_source).collect();
    let targets: Vec<&SchemaComponent> =
        schema_components.iter().filter(|c| !c.is_source).collect();
    let unsupported = |side: &str| {
        MfdError::Unsupported(if skipped_libraries.is_empty() {
            format!("no importable {side} component (xml/json/csv) found in this design")
        } else {
            format!(
                "no importable {side} component (xml/json/csv) found; this design \
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
                    let row_shaped = target.format == ComponentFormat::Csv
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
        let (source_key, _) = builder.resolve_iteration_feed(*from);
        if let Some(abs) = builder.source_abs_path(source_key) {
            builder.note_framed_prefixes(&abs);
        }
    }
    // Materialize every function component up front (filters are handled
    // at the scope stage instead).
    for (i, fc) in fn_components.iter().enumerate() {
        if fc.name != "filter" {
            builder.fn_node(i);
        }
    }
    for (target_path, from) in iterations {
        let (source_key, filter_expr) = builder.resolve_iteration_feed(from);
        let Some(source_abs) = builder.source_abs_path(source_key) else {
            builder.warnings.push(format!(
                "iteration into `{}` comes from an unsupported feed; skipped",
                target_path.join("/")
            ));
            continue;
        };
        let filter_node = filter_expr.and_then(|key| builder.value_node(key));
        scope_builder.add_iteration(&target_path, &source_abs, filter_node);
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

    // Strip the synthetic FileInstance/document entry levels.
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

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
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
    let instance_root = document
        .and_then(|d| d.attribute("instanceroot"))
        .and_then(|r| r.rsplit('}').next())
        .filter(|r| !r.is_empty());
    let schema = document
        .and_then(|d| d.attribute("schema"))
        .and_then(|rel| {
            let xsd_path = mfd_path.parent().unwrap_or(Path::new(".")).join(rel);
            match format_xml::xsd::import_root(&xsd_path, instance_root) {
                Ok(schema) => Some(schema),
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
        ports,
    })
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
        ports,
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
        ports,
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

fn read_fn_component(component: &roxmltree::Node) -> FnComponent {
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

    FnComponent {
        name,
        kind,
        inputs,
        outputs,
        constant,
        valuemap,
    }
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut node = schema;
    for segment in path {
        node = node.child(segment)?;
    }
    Some(node)
}

struct GraphBuilder<'a> {
    graph: Graph,
    next_id: NodeId,
    fn_nodes: BTreeMap<usize, NodeId>,
    source_fields: BTreeMap<Vec<String>, NodeId>,
    edge_from: &'a BTreeMap<u32, u32>,
    sources: &'a [&'a SchemaComponent],
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

    /// The ferrule node producing the value at output-port `key`, creating
    /// SourceField/function nodes on demand. `None` for unsupported feeds.
    fn value_node(&mut self, key: u32) -> Option<NodeId> {
        // A source schema entry?
        for (idx, source) in self.sources.iter().enumerate() {
            if let Some(abs) = source.ports.get(&key) {
                let mut path = self.suffix_after_framed(&source.schema, abs);
                if idx > 0 {
                    // Extra sources are addressed by name from the
                    // outermost context frame.
                    let mut prefixed = vec![self.sources[idx].name.clone()];
                    prefixed.extend(source.ports[&key].iter().cloned());
                    path = prefixed;
                }
                let id = *self
                    .source_fields
                    .entry(path.clone())
                    .or_insert_with_key(|_| {
                        let id = self.next_id;
                        self.next_id += 1;
                        id
                    });
                self.graph
                    .nodes
                    .entry(id)
                    .or_insert(Node::SourceField { path });
                return Some(id);
            }
        }
        // A function output?
        let idx = *self.fn_by_output.get(&key)?;
        if self.fn_components[idx].name == "filter" {
            // A filter feeding a value position is pass-through of its
            // node input for our purposes; treat the value as whatever
            // feeds the filter's first input.
            let feed = self.fn_components[idx]
                .inputs
                .first()
                .copied()
                .flatten()
                .and_then(|k| self.edge_from.get(&k).copied())?;
            return self.value_node(feed);
        }
        Some(self.fn_node(idx))
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

    /// Follows an iteration feed through at most one `filter` component:
    /// returns the source-entry output key plus the filter's boolean
    /// expression key, if any.
    fn resolve_iteration_feed(&self, from: u32) -> (u32, Option<u32>) {
        if let Some(&idx) = self.fn_by_output.get(&from)
            && self.fn_components[idx].name == "filter"
        {
            let fc = &self.fn_components[idx];
            let node_feed = fc
                .inputs
                .first()
                .copied()
                .flatten()
                .and_then(|k| self.edge_from.get(&k).copied());
            let bool_feed = fc
                .inputs
                .get(1)
                .copied()
                .flatten()
                .and_then(|k| self.edge_from.get(&k).copied());
            if let Some(node_feed) = node_feed {
                return (node_feed, bool_feed);
            }
        }
        (from, None)
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
        "greater-equal" | "greater-or-equal" => "greater_or_equal",
        "less-equal" | "less-or-equal" => "less_or_equal",
        "logical-and" => "and",
        "logical-or" => "or",
        "logical-not" => "not",
        "string-length" => "length",
        "contains" => "contains",
        "starts-with" => "starts_with",
        "upper-case" => "upper",
        "lower-case" => "lower",
        "trim" => "trim",
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
        filter: Option<NodeId>,
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
        scope.filter = filter;
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
