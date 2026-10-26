//! Explicit per-message X12 body import. Acquisition, filename timing and
//! physical publication are separate contracts; ordinary import is unchanged.
use super::{ImportOptions, function, graph, schema};
use crate::MfdError;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Binding, Graph, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
};

mod expression;
mod scalar_metadata;
mod target;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X12DocumentBodyFormat {
    Json,
    Xml,
}

pub struct X12DocumentBody {
    pub name: String,
    pub format: X12DocumentBodyFormat,
    pub is_default_output: bool,
    pub output_preview: Option<String>,
    /// Execution source is one projected message, not the physical Envelope.
    pub body: Project,
    pub filename: Project,
    /// Nonempty source value whose occurrences drive this output document.
    pub driver_path: Vec<String>,
}

impl X12DocumentBody {
    /// Retain the independently imported filename as a named output after the
    /// eager JSON body. The separate body and filename projects are unchanged.
    pub fn eager_json_project(&self, filename_target: &str) -> Result<Project, MfdError> {
        if self.format != X12DocumentBodyFormat::Json {
            return Err(refuse("eager JSON documents require a JSON body"));
        }
        self.with_filename_target(filename_target)
    }

    fn with_filename_target(&self, filename_target: &str) -> Result<Project, MfdError> {
        if filename_target.is_empty()
            || self.body.source != self.filename.source
            || !self.body.extra_targets.is_empty()
            || !self.body.extra_sources.is_empty()
            || !self.filename.extra_sources.is_empty()
            || !self.filename.extra_targets.is_empty()
            || !self.filename.failure_rules.is_empty()
        {
            return Err(refuse(
                "eager JSON documents require one body and its same-source filename",
            ));
        }
        let mut project = self.body.clone();
        for (id, node) in &self.filename.graph.nodes {
            match project.graph.nodes.get(id) {
                Some(existing)
                    if serde_json::to_vec(existing).map_err(refuse)?
                        != serde_json::to_vec(node).map_err(refuse)? =>
                {
                    return Err(refuse("body/filename graph identities conflict"));
                }
                Some(_) => {}
                None => {
                    project.graph.nodes.insert(*id, node.clone());
                }
            }
        }
        project.extra_targets.push(mapping::NamedTarget {
            name: filename_target.into(),
            path: None,
            schema: self.filename.target.clone(),
            options: self.filename.target_options.clone(),
            root: self.filename.root.clone(),
        });
        let issues = engine::validate(&project);
        if !issues.is_empty() {
            return Err(refuse(format!(
                "eager JSON document validation: {issues:?}"
            )));
        }
        Ok(project)
    }

    /// Explicitly selected native materialization boundaries, not schema-derived
    /// defaults. The unchanged full body remains the primary target.
    pub fn deferred_xml_project(
        &self,
        filename_target: &str,
        early_fields: &[Vec<String>],
    ) -> Result<Project, MfdError> {
        if self.format != X12DocumentBodyFormat::Xml {
            return Err(refuse("deferred XML preparation requires an XML body"));
        }
        if matches!(filename_target, "PreparedXmlValues" | "DeferredXmlValues") {
            return Err(refuse("reserved XML phase target name"));
        }
        let mut project = self.with_filename_target(filename_target)?;
        let (early, late) =
            mapping::split_xml_preparation(&project.graph, &project.root, early_fields)
                .map_err(refuse)?;
        for (name, root) in [("PreparedXmlValues", early), ("DeferredXmlValues", late)] {
            project.extra_targets.push(mapping::NamedTarget {
                name: name.into(),
                path: None,
                schema: project.target.clone(),
                options: project.target_options.clone(),
                root,
            });
        }
        let issues = engine::validate(&project);
        if !issues.is_empty() {
            return Err(refuse(format!("XML preparation validation: {issues:?}")));
        }
        Ok(project)
    }
}

pub struct ImportedX12DocumentBodies {
    pub source_schema: SchemaNode,
    pub source_options: mapping::FormatOptions,
    pub source_encoding: mapping::X12SourceEncoding,
    pub source_structure: mapping::X12SourceStructure,
    pub source_minimums: Vec<mapping::MapForceEdiMinimum>,
    pub input_parameter: String,
    pub input_preview: Option<String>,
    pub message_path: Vec<String>,
    pub targets: Vec<X12DocumentBody>,
    pub mapping_path: PathBuf,
}

impl ImportedX12DocumentBodies {
    /// Retain the original primary parameter separately from body/filename
    /// execution. No preview text is supplied as a runtime value.
    pub fn eager_json_document(
        &self,
        target_name: &str,
        filename_target: &str,
    ) -> Result<mapping::DocumentProject, MfdError> {
        let targets = self
            .targets
            .iter()
            .filter(|target| target.name == target_name)
            .collect::<Vec<_>>();
        let [target] = targets.as_slice() else {
            return Err(refuse("JSON document target is absent or ambiguous"));
        };
        self.with_primary_filename(target.eager_json_project(filename_target)?)
    }

    pub fn deferred_xml_document(
        &self,
        target_name: &str,
        filename_target: &str,
        early_fields: &[Vec<String>],
    ) -> Result<mapping::DocumentProject, MfdError> {
        let targets = self
            .targets
            .iter()
            .filter(|t| t.name == target_name)
            .collect::<Vec<_>>();
        let [target] = targets.as_slice() else {
            return Err(refuse("XML document target is absent or ambiguous"));
        };
        self.with_primary_filename(target.deferred_xml_project(filename_target, early_fields)?)
    }

    fn with_primary_filename(
        &self,
        mut project: Project,
    ) -> Result<mapping::DocumentProject, MfdError> {
        let node = project
            .graph
            .nodes
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| refuse("primary filename node identity overflow"))?;
        project.graph.nodes.insert(
            node,
            Node::RuntimeParameter {
                name: self.input_parameter.clone(),
                ty: ScalarType::String,
                preview: self.input_preview.clone(),
            },
        );
        Ok(mapping::DocumentProject::new(
            project,
            mapping::PrimarySourcePlan { filename: node },
        ))
    }
}

fn refuse(reason: impl std::fmt::Display) -> MfdError {
    MfdError::UnsupportedImport(format!("X12 document bodies: {reason}"))
}
fn number(value: Option<&str>) -> Result<u32, MfdError> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| refuse("invalid raw port identity"))
}
fn lexical_schema(node: &mut SchemaNode) {
    match &mut node.kind {
        SchemaKind::Scalar { ty } => *ty = ScalarType::String,
        SchemaKind::Group { children, .. } => {
            for child in children {
                lexical_schema(child);
            }
        }
        SchemaKind::ScalarUnion { .. } => {}
    }
}
fn schema_at<'a>(root: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    path.iter().try_fold(root, |node, name| node.child(name))
}
fn find_message(
    node: &SchemaNode,
    wanted: &str,
    path: &mut Vec<String>,
    found: &mut Vec<Vec<String>>,
) {
    if let SchemaKind::Group { children, .. } = &node.kind {
        for child in children {
            path.push(child.name.clone());
            if child.name == wanted {
                found.push(path.clone());
            }
            find_message(child, wanted, path, found);
            path.pop();
        }
    }
}

#[derive(Clone)]
struct Function {
    parsed: Rc<function::FnComponent>,
    uid: u32,
    inputs: Vec<Option<u32>>,
}
#[derive(Clone, Copy)]
struct Expr {
    value: NodeId,
    present: NodeId,
    always: bool,
}
struct Builder<'a> {
    graph: Graph,
    next: NodeId,
    functions: &'a BTreeMap<u32, Function>,
    feeds: &'a BTreeMap<u32, u32>,
    ports: &'a BTreeMap<u32, Vec<String>>,
    message: &'a SchemaNode,
    required: &'a BTreeSet<Vec<String>>,
    target: String,
    cache: BTreeMap<(u32, Vec<String>, bool), Expr>,
    active: BTreeSet<(u32, Vec<String>, bool)>,
    consumed_target_inputs: BTreeSet<u32>,
    nullable_input: bool,
}

/// Import supported JSON and XML body targets from their original raw entries.
/// This does not declare full native file execution or choose a transport.
pub fn import_x12_document_bodies_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedX12DocumentBodies, MfdError> {
    let resources = super::resolved_resources(path, options)?;
    let result = (|| {
        let text = resources.read_design()?;
        let doc = roxmltree::Document::parse(&text)?;
        if doc.root_element().attribute("version") != Some("36") {
            return Err(refuse("requires version 36"));
        }
        let wrappers: Vec<_> = doc
            .root_element()
            .children()
            .filter(|n| n.has_tag_name("component"))
            .collect();
        let [wrapper] = wrappers.as_slice() else {
            return Err(refuse("requires one mapping wrapper"));
        };
        let structure = wrapper
            .children()
            .find(|n| n.has_tag_name("structure"))
            .ok_or_else(|| refuse("missing structure"))?;
        let children = structure
            .children()
            .find(|n| n.has_tag_name("children"))
            .ok_or_else(|| refuse("missing components"))?;
        let edges = graph::read_edge_inputs(&structure, Some(wrapper));
        if !edges.complete || !edges.conflicts.is_empty() {
            return Err(refuse("incomplete or conflicting connections"));
        }
        let components: Vec<_> = children
            .children()
            .filter(|n| n.has_tag_name("component"))
            .collect();
        let sources: Vec<_> = components
            .iter()
            .filter(|n| {
                n.attribute("library") == Some("text")
                    && n.descendants()
                        .any(|c| c.has_tag_name("text") && c.attribute("kind") == Some("EDIX12"))
            })
            .copied()
            .collect();
        let [source] = sources.as_slice() else {
            return Err(refuse("requires one X12 source"));
        };
        check_component_properties(*source, true)?;
        if components
            .iter()
            .any(|c| c.attribute("library") == Some("text") && c.id() != source.id())
        {
            return Err(refuse(
                "additional text sources are outside this body profile",
            ));
        }
        let mut warnings = Vec::new();
        let source_schema = schema::read_edi_component(source, &resources, &mut warnings)
            .ok_or_else(|| refuse("source schema unavailable"))?;
        let source_text = source
            .children()
            .find(|node| node.has_tag_name("data"))
            .and_then(|node| node.children().find(|node| node.has_tag_name("text")))
            .ok_or_else(|| refuse("missing X12 source settings"))?;
        let source_encoding = match source_text.attribute("encoding") {
            None | Some("1000") => mapping::X12SourceEncoding::Utf8,
            Some("1") => mapping::X12SourceEncoding::Latin1,
            _ => return Err(refuse("unsupported native X12 input encoding")),
        };
        if source_text
            .attribute("byteorder")
            .is_some_and(|value| value != "1")
            || source_text
                .attribute("byteordermark")
                .is_some_and(|value| value != "0")
        {
            return Err(refuse("unsupported native X12 encoding flags"));
        }
        if !source_schema.is_source || !warnings.is_empty() {
            return Err(refuse("source schema is not authoritative"));
        }
        let source_minimums =
            schema::read_edi_source_minimums(source, &resources).map_err(refuse)?;
        let source_structure =
            schema::read_edi_source_structure(source, &resources).map_err(refuse)?;
        let message_names: Vec<_> = source
            .descendants()
            .filter(|n| n.has_tag_name("message"))
            .filter_map(|n| n.attribute("type"))
            .collect();
        let [message_name] = message_names.as_slice() else {
            return Err(refuse("requires one selected message type"));
        };
        let mut matches = Vec::new();
        find_message(
            &source_schema.schema,
            &format!("Message_{message_name}"),
            &mut vec![],
            &mut matches,
        );
        let [message_path] = matches.as_slice() else {
            return Err(refuse("ambiguous message owner"));
        };
        let mut message = schema_at(&source_schema.schema, message_path)
            .ok_or_else(|| refuse("message schema unavailable"))?
            .clone();
        message.repeating = false;
        lexical_schema(&mut message);
        let ports = source_schema
            .ports
            .iter()
            .filter_map(|(key, path)| {
                path.strip_prefix(message_path.as_slice())
                    .map(|path| (*key, path.to_vec()))
            })
            .collect::<BTreeMap<_, _>>();
        let required = source_minimums
            .iter()
            .filter(|m| m.minimum > 0)
            .filter_map(|m| {
                m.path
                    .strip_prefix(message_path.as_slice())
                    .map(<[String]>::to_vec)
            })
            .collect::<BTreeSet<_>>();
        let mut functions = BTreeMap::new();
        let mut roles = BTreeMap::new();
        let mut uids = BTreeSet::new();
        for component in &components {
            if !uids.insert(number(component.attribute("uid"))?) {
                return Err(refuse("duplicate component identity"));
            }
            for node in component.descendants().filter(|n| n.has_tag_name("entry")) {
                for (attr, output) in [("inpkey", false), ("outkey", true)] {
                    if let Some(key) = node.attribute(attr)
                        && roles.insert(number(Some(key))?, output).is_some()
                    {
                        return Err(refuse("duplicate data port"));
                    }
                }
            }
            if matches!(
                component.attribute("library"),
                Some("text" | "json" | "xml")
            ) {
                continue;
            }
            let explicit_scalar = scalar_metadata::is_explicit_scalar(*component);
            if !(if explicit_scalar {
                scalar_metadata::valid(*component, &edges.feeds)
            } else {
                super::json_edi_document::function_metadata(*component)
            }) {
                return Err(refuse("unsupported function metadata"));
            }
            let parsed = Rc::new(function::read(component));
            if !matches!(
                (parsed.library.as_str(), parsed.kind, parsed.name.as_str()),
                ("core", 2, _)
                    | ("core", 6, _)
                    | ("core", 3, _)
                    | ("core", 4, "if-else")
                    | (
                        "core",
                        5,
                        "equal"
                            | "concat"
                            | "format-dateTime"
                            | "format-number"
                            | "boolean"
                            | "auto-number"
                    )
                    | ("db", 5, "substitute-null")
                    | ("lang", 5, "now" | "create-guid")
                    | ("core", 23, "value-map")
                    | ("db", 5, "is-not-null")
                    | ("lang", 5, "format-guid-string")
            ) {
                return Err(refuse(format!(
                    "unsupported function {}:{}",
                    parsed.library, parsed.name
                )));
            }
            if parsed.kind == 2 && !super::json_edi_document::literal_data(*component) {
                return Err(refuse("invalid literal declaration"));
            }
            let inputs = super::json_edi_document::pins(*component, "sources")?;
            let outputs = super::json_edi_document::pins(*component, "targets")?;
            if parsed.inputs != inputs || parsed.output_pins != outputs {
                return Err(refuse("raw function pins changed during reading"));
            }
            if parsed.kind == 3 {
                if inputs.len() != 2
                    || inputs.iter().any(Option::is_none)
                    || outputs.len() != 2
                    || outputs[1].is_some()
                {
                    return Err(refuse(
                        "only the true output of a two-input filter is supported",
                    ));
                }
            } else if outputs.len() != 1 {
                return Err(refuse("scalar functions require one declared output"));
            }
            if parsed.kind == 2 && !inputs.is_empty() {
                return Err(refuse("literal with input pins"));
            }
            for key in inputs.iter().flatten() {
                if roles.insert(*key, false).is_some() {
                    return Err(refuse("duplicate input port"));
                }
            }
            for key in outputs.iter().flatten() {
                if roles.insert(*key, true).is_some() {
                    return Err(refuse("duplicate output port"));
                }
            }
            let f = Function {
                parsed,
                uid: number(component.attribute("uid"))?,
                inputs,
            };
            for key in outputs.into_iter().flatten() {
                if functions.insert(key, f.clone()).is_some() {
                    return Err(refuse("duplicate function output"));
                }
            }
        }
        for (input, output) in &edges.feeds {
            if roles.get(input) != Some(&false) || roles.get(output) != Some(&true) {
                return Err(refuse("unknown connection endpoint"));
            }
        }
        let input_file = source
            .descendants()
            .find(|n| n.has_tag_name("entry") && n.attribute("name") == Some("FileInstance"))
            .ok_or_else(|| refuse("missing source file wrapper"))?;
        let input_key = number(input_file.attribute("inpkey"))?;
        let input_feed = edges
            .feeds
            .get(&input_key)
            .ok_or_else(|| refuse("unconnected source filename"))?;
        let input = functions
            .get(input_feed)
            .ok_or_else(|| refuse("source filename must be a direct parameter"))?;
        let parameter = input
            .parsed
            .input_parameter_name
            .as_ref()
            .filter(|p| !p.optional)
            .ok_or_else(|| refuse("missing required input parameter"))?;
        if input.parsed.kind != 6
            || input.parsed.input_type != Some(ScalarType::String)
            || input.inputs != [None]
        {
            return Err(refuse("source filename must be a String parameter"));
        }
        let mut targets = Vec::new();
        for component in components
            .iter()
            .filter(|n| matches!(n.attribute("library"), Some("json" | "xml")))
        {
            check_component_properties(*component, false)?;
            let format = if component.attribute("library") == Some("json") {
                X12DocumentBodyFormat::Json
            } else {
                X12DocumentBodyFormat::Xml
            };
            warnings.clear();
            let target = match format {
                X12DocumentBodyFormat::Json => {
                    schema::read_json_component_in_package(component, &resources, &mut warnings)
                }
                X12DocumentBodyFormat::Xml => {
                    let read = schema::read_schema_component_in_package_with_provenance(
                        component,
                        &resources,
                        None,
                        &mut warnings,
                    )
                    .ok_or_else(|| refuse("XML target schema unavailable"))?;
                    if !read.schema_file_authoritative || read.entry_tree_fallback {
                        return Err(refuse("XML target schema is not authoritative"));
                    }
                    Some(read.component)
                }
            }
            .ok_or_else(|| refuse("target schema unavailable"))?;
            if target.is_source || !warnings.is_empty() {
                return Err(refuse("unresolved target schema"));
            }
            if format == X12DocumentBodyFormat::Json && !target.json_schema_authoritative {
                return Err(refuse("JSON target schema is not authoritative"));
            }
            let file = component
                .descendants()
                .find(|n| n.has_tag_name("entry") && n.attribute("name") == Some("FileInstance"))
                .ok_or_else(|| refuse("missing target file wrapper"))?;
            let filename_feed = *edges
                .feeds
                .get(&number(file.attribute("inpkey"))?)
                .ok_or_else(|| refuse("missing output filename expression"))?;
            let root = file
                .children()
                .find(|n| n.has_tag_name("entry") && n.attribute("name") == Some("document"))
                .and_then(|n| n.children().find(|n| n.has_tag_name("entry")))
                .ok_or_else(|| refuse("missing target root"))?;
            let mut builder = Builder {
                graph: Graph::default(),
                next: 0,
                functions: &functions,
                feeds: &edges.feeds,
                ports: &ports,
                message: &message,
                required: &required,
                target: target.name.clone(),
                cache: BTreeMap::new(),
                active: BTreeSet::new(),
                consumed_target_inputs: BTreeSet::new(),
                nullable_input: false,
            };
            let driver_ports = builder.dependencies(filename_feed, &mut BTreeSet::new())?;
            let [driver] = driver_ports.iter().copied().collect::<Vec<_>>()[..] else {
                return Err(refuse(
                    "filename needs exactly one message-local driving field",
                ));
            };
            let driver_path = ports
                .get(&driver)
                .ok_or_else(|| refuse("filename driver is outside message"))?
                .clone();
            if !matches!(
                schema_at(&message, &driver_path).map(|n| &n.kind),
                Some(SchemaKind::Scalar { .. })
            ) {
                return Err(refuse("filename driver must be scalar"));
            }
            let mut root_scope = builder.target_scope(root, &target.schema, &[], &[], format)?;
            root_scope.target_field.clear();
            builder
                .consumed_target_inputs
                .insert(number(file.attribute("inpkey"))?);
            for entry in component.descendants().filter(|n| n.has_tag_name("entry")) {
                if let Some(input) = entry.attribute("inpkey") {
                    let input = number(Some(input))?;
                    if edges.feeds.contains_key(&input)
                        && !builder.consumed_target_inputs.contains(&input)
                    {
                        return Err(refuse(format!(
                            "connected target input {input} is outside the admitted body"
                        )));
                    }
                }
            }
            let body_graph = builder.graph.clone();
            let filename = builder.scalar(filename_feed, &[])?;
            let name_root = Scope {
                bindings: vec![Binding {
                    target_field: "Path".into(),
                    node: filename.value,
                }],
                ..Default::default()
            };
            let mut body = super::json_edi_document::empty_project(
                message.clone(),
                target.schema,
                body_graph,
                root_scope,
            );
            body.target_options = target.options;
            let mut name = super::json_edi_document::empty_project(
                message.clone(),
                SchemaNode::group(
                    "OutputName",
                    vec![SchemaNode::scalar("Path", ScalarType::String)],
                ),
                builder.graph,
                name_root,
            );
            body.prune_unreachable_nodes();
            name.prune_unreachable_nodes();
            for (label, project) in [("body", &body), ("filename", &name)] {
                let issues = engine::validate(project);
                if !issues.is_empty() {
                    return Err(refuse(format!("{label} validation: {issues:?}")));
                }
            }
            targets.push(X12DocumentBody {
                name: target.name,
                format,
                is_default_output: target.is_default_output,
                output_preview: target.output_instance,
                body,
                filename: name,
                driver_path,
            });
        }
        if targets.is_empty() {
            return Err(refuse("no JSON/XML target bodies"));
        }
        Ok(ImportedX12DocumentBodies {
            source_schema: source_schema.schema,
            source_options: source_schema.options,
            source_encoding,
            source_structure,
            source_minimums,
            input_parameter: parameter.name.clone(),
            input_preview: input.parsed.input_preview.clone(),
            message_path: message_path.clone(),
            targets,
            mapping_path: path.to_path_buf(),
        })
    })();
    resources.finish_bindings()?;
    result
}

fn check_component_properties(
    component: roxmltree::Node<'_, '_>,
    source: bool,
) -> Result<(), MfdError> {
    let properties = component
        .children()
        .find(|n| n.has_tag_name("properties"))
        .ok_or_else(|| refuse("missing file-component properties"))?;
    if properties.attribute("DynamicFilenames") != Some("1")
        || properties
            .attribute("XSLTTargetEncoding")
            .is_some_and(|v| v != "UTF-8")
        || properties.attributes().any(|a| {
            !matches!(
                a.name(),
                "DynamicFilenames" | "XSLTTargetEncoding" | "XSLTDefaultOutput"
            )
        })
    {
        return Err(refuse("unsupported file-component properties"));
    }
    for node in component.descendants().filter(|n| n.has_tag_name("entry")) {
        if let Some(mode) = node.attribute("casttotargettypemode")
            && (source || node.attribute("name") != Some("document") || mode != "cast-in-subtree")
        {
            return Err(refuse("unqualified cast-to-target policy"));
        }
        if node
            .children()
            .any(|n| n.has_tag_name("inputnodefunctions") || n.has_tag_name("condition"))
        {
            return Err(refuse(
                "node functions and derived-type conditions require separate admission",
            ));
        }
    }
    Ok(())
}
