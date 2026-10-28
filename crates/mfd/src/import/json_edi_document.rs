//! Explicit ordered JSON-to-EDI body lowering. Source acquisition, filename
//! evaluation and physical EDI publication remain separate host contracts.

use super::{
    ImportOptions, ImportedJsonParameterProjection, ImportedJsonSourceProjection, function, graph,
    schema,
};
use crate::MfdError;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, DocumentProject, Graph, Node, NodeId, PrimarySourcePlan, Project,
    RuntimeValue, Scope, ScopeIteration, ScopeSequence,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct ImportedJsonEdiDocument {
    pub document: DocumentProject,
    /// Source-free filename program. Evaluate before acquisition, not after the body.
    pub output_filename: Project,
    pub source: ImportedJsonSourceProjection,
    pub writer_minimums: Vec<mapping::MapForceEdiMinimum>,
}

/// Grouped JSON input with one independently named EDI document per group.
/// Acquisition and physical publication remain explicit host operations.
/// A prepared candidate retains the complete source object for each String key.
pub struct ImportedGroupedJsonEdiDocument {
    pub document: DocumentProject,
    pub source: ImportedJsonSourceProjection,
    pub writer_minimums: Vec<mapping::MapForceEdiMinimum>,
}

/// An explicit parameter-driven outer loop around a physical grouped document.
/// The host must prepare/visit the parameter invocations before acquiring files.
pub struct ImportedInvokedGroupedJsonEdiDocument {
    pub document: DocumentProject,
    pub source: ImportedJsonSourceProjection,
    pub parameter_source: ImportedJsonParameterProjection,
    pub invocation: mapping::JsonParameterInvocationPlan,
    pub writer_minimums: Vec<mapping::MapForceEdiMinimum>,
}

struct ImportedParts {
    document: DocumentProject,
    output_filename: Option<Project>,
    source: ImportedJsonSourceProjection,
    writer_minimums: Vec<mapping::MapForceEdiMinimum>,
    invocation: Option<mapping::JsonParameterInvocationPlan>,
}

#[derive(Clone)]
struct GroupedSource {
    collection: Vec<String>,
    key_output: u32,
    key_node: NodeId,
    key_path: Vec<String>,
    filename_feed: u32,
}

#[derive(Clone, Copy)]
enum SiblingStringSelection {
    Direct(u32),
    EmptyFallback { source: u32, fallback: u32 },
}

fn refuse(reason: impl AsRef<str>) -> MfdError {
    MfdError::UnsupportedImport(format!("JSON/EDI document body: {}", reason.as_ref()))
}

#[derive(Clone)]
struct Function {
    parsed: std::rc::Rc<function::FnComponent>,
    inputs: Vec<Option<u32>>,
    literal_valid: bool,
    metadata_valid: bool,
}
struct Builder {
    graph: Graph,
    next: NodeId,
    functions: BTreeMap<u32, Function>,
    udf_outputs: BTreeMap<u32, super::udf::ScalarCallOutput>,
    grouped: Option<GroupedSource>,
    source_schema: SchemaNode,
    in_group_member: bool,
    grouping_preparation: Option<mapping::JsonGroupingPreparation>,
    feeds: BTreeMap<u32, u32>,
    ports: BTreeMap<u32, Vec<String>>,
    port_kinds: BTreeMap<u32, String>,
    integer_ports: BTreeSet<u32>,
    filename_port: Option<u32>,
    string_targets: BTreeSet<u32>,
    cache: BTreeMap<(u32, Option<u32>, Vec<String>, bool), NodeId>,
    active: BTreeSet<(u32, Option<u32>, Vec<String>, bool)>,
    parameter_key: Option<(u32, NodeId)>,
    parameter_dependent_feeds: BTreeSet<u32>,
    filename_phase: bool,
}

pub fn import_json_edi_document_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedJsonEdiDocument, MfdError> {
    let imported = import_document(path, options, false)?;
    Ok(ImportedJsonEdiDocument {
        document: imported.document,
        output_filename: imported
            .output_filename
            .ok_or_else(|| refuse("missing static output filename"))?,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    })
}

pub fn import_grouped_json_edi_document_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedGroupedJsonEdiDocument, MfdError> {
    let imported = import_document(path, options, true)?;
    Ok(ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    })
}

pub fn import_invoked_grouped_json_edi_document_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedInvokedGroupedJsonEdiDocument, MfdError> {
    let parameter = super::import_json_parameter_projection_with_options(path, options)?;
    let source = super::json_source_projection::import_physical_json_after_parameter(
        path, options, &parameter,
    )?;
    let imported = import_document_from_source(path, options, true, source, Some(&parameter))?;
    Ok(ImportedInvokedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        parameter_source: parameter,
        invocation: imported
            .invocation
            .ok_or_else(|| refuse("missing outer parameter invocation"))?,
        writer_minimums: imported.writer_minimums,
    })
}

fn import_document(
    path: &Path,
    options: &ImportOptions,
    grouped: bool,
) -> Result<ImportedParts, MfdError> {
    let source = super::import_json_source_projection_with_options(path, options)?;
    import_document_from_source(path, options, grouped, source, None)
}

fn import_document_from_source(
    path: &Path,
    options: &ImportOptions,
    grouped: bool,
    source: ImportedJsonSourceProjection,
    parameter: Option<&ImportedJsonParameterProjection>,
) -> Result<ImportedParts, MfdError> {
    let resources = super::resolved_resources(path, options)?;
    let result = (|| {
        let text = resources.read_design()?;
        let parsed = roxmltree::Document::parse(&text)?;
        let wrapper = parsed
            .root_element()
            .children()
            .find(|n| n.has_tag_name("component"))
            .ok_or_else(|| refuse("missing wrapper"))?;
        let structure = wrapper
            .children()
            .find(|n| n.has_tag_name("structure"))
            .ok_or_else(|| refuse("missing structure"))?;
        let children = structure
            .children()
            .find(|n| n.has_tag_name("children"))
            .ok_or_else(|| refuse("missing children"))?;
        let edges = graph::read_edge_inputs(&structure, Some(&wrapper));
        if !edges.complete || !edges.conflicts.is_empty() {
            return Err(refuse("ambiguous connections"));
        }
        let targets: Vec<_> = children
            .children()
            .filter(|n| {
                n.has_tag_name("component")
                    && n.attribute("library") == Some("text")
                    && n.descendants()
                        .any(|n| n.has_tag_name("text") && n.attribute("type") == Some("edi"))
            })
            .collect();
        let [target] = targets.as_slice() else {
            return Err(refuse("requires one EDI target"));
        };
        if let Some(parameter) = parameter {
            for component in children
                .children()
                .filter(|node| node.has_tag_name("component"))
            {
                match component.attribute("library") {
                    Some("json") => {
                        let inputs = component
                            .descendants()
                            .filter(|node| {
                                node.has_tag_name("entry")
                                    && node.attribute("name") == Some("FileInstance")
                            })
                            .filter_map(|node| {
                                node.attribute("inpkey")
                                    .and_then(|value| value.parse::<u32>().ok())
                            })
                            .collect::<Vec<_>>();
                        if inputs.len() != 1
                            || ![source.filename_input_port, parameter.input_port]
                                .contains(&inputs[0])
                        {
                            return Err(refuse("unqualified additional JSON source or target"));
                        }
                    }
                    Some("text") if component == *target => {}
                    Some("core" | "lang" | "db") if component.attribute("kind") != Some("7") => {}
                    _ if component.attribute("kind") == Some("19") => {}
                    _ => return Err(refuse("unqualified additional document component")),
                }
            }
        }
        let properties = target
            .children()
            .find(|node| node.has_tag_name("properties"))
            .ok_or_else(|| refuse("missing target properties"))?;
        if properties.attribute("DynamicFilenames") != Some("1")
            || properties.attributes().any(|attribute| {
                !matches!(attribute.name(), "DynamicFilenames" | "XSLTDefaultOutput")
            })
        {
            return Err(refuse("unqualified target properties"));
        }
        let mut warnings = Vec::new();
        let target_schema = schema::read_edi_component(target, &resources, &mut warnings)
            .ok_or_else(|| refuse("target schema did not load"))?;
        let writer_minimums =
            schema::read_edi_writer_minimums(target, &resources).map_err(refuse)?;
        if !warnings.is_empty() || target_schema.is_source || target_schema.has_target_node_controls
        {
            return Err(refuse("unresolved target schema or node controls"));
        }
        let roots: Vec<_> = target
            .descendants()
            .filter(|n| n.has_tag_name("entry") && n.attribute("name") == Some("FileInstance"))
            .collect();
        let [file] = roots.as_slice() else {
            return Err(refuse("ambiguous output filename wrapper"));
        };
        let filename_input = number(file.attribute("inpkey"))?;
        let root = file
            .children()
            .find(|n| n.has_tag_name("entry") && n.attribute("name") == Some("document"))
            .and_then(|n| n.children().find(|n| n.has_tag_name("entry")))
            .ok_or_else(|| refuse("missing output root"))?;
        if root.attribute("name") != Some(target_schema.schema.name.as_str()) {
            return Err(refuse("output root differs from schema"));
        }
        let mut functions = BTreeMap::new();
        let definitions = super::udf::Registry::read_in_package(
            &parsed.root_element(),
            &resources,
            &mut Vec::new(),
        );
        let mut udf_outputs = BTreeMap::new();
        let mut port_kinds = BTreeMap::new();
        let mut parameter_kinds = BTreeMap::new();
        if parameter.is_some_and(|parameter| {
            parameter.ports.keys().any(|key| {
                source.ports.contains_key(key) || source.filename_output_port == Some(*key)
            })
        }) {
            return Err(refuse("physical and parameter source identities overlap"));
        }
        for component in children.children().filter(|n| n.has_tag_name("component")) {
            for entry in component
                .descendants()
                .filter(|n| n.has_tag_name("entry") && n.attribute("outkey").is_some())
            {
                let key = number(entry.attribute("outkey"))?;
                if source.ports.contains_key(&key)
                    && port_kinds
                        .insert(key, entry.attribute("name").unwrap_or_default().into())
                        .is_some()
                {
                    return Err(refuse("duplicate source output owner"));
                }
                if parameter.is_some_and(|parameter| parameter.ports.contains_key(&key))
                    && parameter_kinds
                        .insert(key, entry.attribute("name").unwrap_or_default().to_string())
                        .is_some()
                {
                    return Err(refuse("duplicate parameter output owner"));
                }
            }
            if matches!(component.attribute("library"), Some("json" | "text")) {
                continue;
            }
            if component.attribute("kind") == Some("19") {
                // MapForce retains palette instances after every input/output
                // port is removed. Such an instance cannot be demanded by this
                // graph. A connected input without outputs is not this case.
                if !component.descendants().any(|node| {
                    node.has_attribute("inpkey")
                        || node.has_attribute("outkey")
                        || node.has_tag_name("datapoint") && node.has_attribute("key")
                }) {
                    continue;
                }
                let outputs = definitions
                    .string_call_outputs(&component)
                    .map_err(|reason| {
                        refuse(format!(
                            "embedded function {}: {reason}",
                            component.attribute("name").unwrap_or_default()
                        ))
                    })?;
                for (port, output) in outputs {
                    if source.ports.contains_key(&port)
                        || parameter.is_some_and(|parameter| parameter.ports.contains_key(&port))
                        || source.filename_output_port == Some(port)
                        || functions.contains_key(&port)
                        || udf_outputs.insert(port, output).is_some()
                    {
                        return Err(refuse("ambiguous embedded scalar output"));
                    }
                }
                continue;
            }
            let f = std::rc::Rc::new(function::read(&component));
            if f.outputs.is_empty() {
                continue;
            }
            let inputs = pins(component, "sources")?;
            for key in &f.outputs {
                if source.ports.contains_key(key)
                    || parameter.is_some_and(|parameter| parameter.ports.contains_key(key))
                    || source.filename_output_port == Some(*key)
                    || udf_outputs.contains_key(key)
                    || functions
                        .insert(
                            *key,
                            Function {
                                parsed: f.clone(),
                                inputs: inputs.clone(),
                                literal_valid: literal_data_with_default(
                                    component,
                                    parameter.is_some(),
                                ),
                                metadata_valid: function_metadata(component),
                            },
                        )
                        .is_some()
                {
                    return Err(refuse("ambiguous output owner"));
                }
            }
        }
        if port_kinds.len() != source.ports.len() {
            return Err(refuse("missing typed source owner"));
        }
        if parameter.is_some_and(|parameter| parameter_kinds.len() != parameter.ports.len()) {
            return Err(refuse("missing typed parameter owner"));
        }
        let mut builder = Builder {
            graph: Graph::default(),
            next: 0,
            functions,
            udf_outputs,
            grouped: None,
            source_schema: source
                .shape
                .document_set_schema()
                .map_err(|_| refuse("invalid source shape"))?,
            in_group_member: false,
            grouping_preparation: None,
            feeds: edges.feeds,
            ports: source.ports.clone(),
            port_kinds,
            integer_ports: source
                .ports
                .iter()
                .filter_map(|(port, path)| {
                    let mut schema = &source.source_schema;
                    let mut item_pending = schema.repeating;
                    for part in path.iter().skip(2) {
                        if item_pending && part == "Items" {
                            item_pending = false;
                        } else {
                            schema = schema.child(part)?;
                            item_pending = schema.repeating;
                        }
                    }
                    match &schema.kind {
                        SchemaKind::Scalar {
                            ty: ScalarType::Int,
                        } => Some(*port),
                        SchemaKind::ScalarUnion { types }
                            if types.contains(ScalarType::Int)
                                && !types.contains(ScalarType::Float) =>
                        {
                            Some(*port)
                        }
                        _ => None,
                    }
                })
                .collect(),
            filename_port: source.filename_output_port,
            string_targets: target_schema
                .ports
                .iter()
                .filter_map(|(key, path)| {
                    let mut node = &target_schema.schema;
                    for name in path {
                        node = node.child(name)?;
                    }
                    matches!(
                        node.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::String
                        }
                    )
                    .then_some(*key)
                })
                .collect(),
            cache: BTreeMap::new(),
            active: BTreeSet::new(),
            parameter_key: None,
            parameter_dependent_feeds: BTreeSet::new(),
            filename_phase: true,
        };
        let input_node = builder.alloc(Node::RuntimeParameter {
            name: source.filename_parameter.clone(),
            ty: ScalarType::String,
            preview: source.filename_preview.clone(),
        })?;
        let output_feed = builder.feed(filename_input)?;
        let invocation = parameter
            .map(|parameter| builder.bind_parameter_group(output_feed, parameter))
            .transpose()?;
        if grouped {
            builder.bind_grouped_source(output_feed, &source.shape)?;
        }
        if !builder
            .dependencies(output_feed, &mut BTreeSet::new())?
            .is_empty()
        {
            return Err(refuse("output filename is not source-free"));
        }
        let anchor = builder
            .grouped
            .as_ref()
            .map(|group| group.collection.clone())
            .unwrap_or_default();
        let output_node = builder.compile(output_feed, None, &anchor)?;
        let filename = (!grouped).then(|| {
            empty_project(
                SchemaNode::group("FilenameInput", vec![]),
                SchemaNode::group(
                    "Filename",
                    vec![SchemaNode::scalar("Path", ScalarType::String)],
                ),
                builder.graph.clone(),
                Scope {
                    bindings: vec![Binding {
                        target_field: "Path".into(),
                        node: output_node,
                    }],
                    ..Default::default()
                },
            )
        });
        builder.filename_phase = false;
        let mut body = builder.scope(
            root,
            &target_schema.schema,
            &[],
            &anchor,
            None,
            &target_schema.options,
        )?;
        if let Some(group) = &builder.grouped {
            body.iteration = ScopeIteration::DynamicDocuments {
                source: group.collection.clone(),
                output_path: output_node,
            };
            body.group_by = Some(group.key_node);
            let key = group.key_node;
            body.filter = Some(builder.call("exists", vec![key])?);
        }
        let preparation = builder.grouping_preparation.clone();
        let mut project = empty_project(
            builder.source_schema.clone(),
            target_schema.schema,
            builder.graph,
            body,
        );
        project.target_options = target_schema.options;
        let document = DocumentProject::new(
            project,
            PrimarySourcePlan {
                filename: input_node,
            },
        );
        let document = match preparation {
            Some(preparation) => {
                document.with_json_grouping_preparation(source.shape.clone(), preparation)
            }
            None => document.with_json_source_projection(source.shape.clone()),
        }
        .map_err(|_| refuse("invalid source projection binding"))?;
        for (label, project) in std::iter::once(("body", document.project()))
            .chain(filename.as_ref().map(|project| ("filename", project)))
        {
            let issues = engine::validate(project);
            if !issues.is_empty() {
                return Err(refuse(format!("{label} validation: {issues:?}")));
            }
        }
        Ok(ImportedParts {
            document,
            output_filename: filename,
            source,
            writer_minimums,
            invocation,
        })
    })();
    resources.finish_bindings()?;
    result
}

pub(super) fn empty_project(
    source: SchemaNode,
    target: SchemaNode,
    graph: Graph,
    root: Scope,
) -> Project {
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph,
        root,
    }
}
fn number(text: Option<&str>) -> Result<u32, MfdError> {
    text.and_then(|s| s.parse().ok())
        .ok_or_else(|| refuse("invalid port identity"))
}
fn relative(path: &[String], anchor: &[String]) -> Result<Vec<String>, MfdError> {
    path.strip_prefix(anchor)
        .map(<[String]>::to_vec)
        .ok_or_else(|| refuse("source expression crosses its document/row owner"))
}
pub(super) fn literal_data(component: roxmltree::Node<'_, '_>) -> bool {
    literal_data_with_default(component, false)
}

fn literal_data_with_default(component: roxmltree::Node<'_, '_>, empty_string: bool) -> bool {
    let data: Vec<_> = component
        .children()
        .filter(|n| n.has_tag_name("data"))
        .collect();
    let [data] = data.as_slice() else {
        return false;
    };
    let literals: Vec<_> = data
        .children()
        .filter(|n| n.has_tag_name("constant"))
        .collect();
    matches!(literals.as_slice(),[value] if (value.attribute("value").is_some() || empty_string && value.attribute("datatype") == Some("string")) && matches!(value.attribute("datatype"),Some("string"|"decimal")) && value.attributes().all(|a|matches!(a.name(),"value"|"datatype")) && !value.children().any(|n|n.is_element()))
        && data.attributes().len() == 0
        && data.children().filter(|n| n.is_element()).all(|n| {
            n.has_tag_name("constant")
                || (n.has_tag_name("wsdl")
                    && n.attributes().len() == 0
                    && !n.children().any(|n| n.is_element()))
        })
}

pub(super) fn function_metadata(component: roxmltree::Node<'_, '_>) -> bool {
    component.attributes().all(|attribute| {
        attribute.namespace().is_none()
            && matches!(
                attribute.name(),
                "name" | "library" | "kind" | "uid" | "growable" | "growablebasename"
            )
    }) && component
        .children()
        .filter(|node| node.is_element())
        .all(|node| {
            matches!(
                node.tag_name().name(),
                "sources" | "targets" | "view" | "data" | "properties"
            ) && (node.tag_name().name() != "properties"
                || node.attributes().len() == 0 && !node.children().any(|child| child.is_element()))
                && (node.tag_name().name() != "data"
                    || matches!(component.attribute("kind"), Some("2" | "6"))
                    || node.attributes().len() == 0
                        && !node.children().any(|child| child.is_element()))
        })
}
pub(super) fn pins(
    component: roxmltree::Node<'_, '_>,
    kind: &str,
) -> Result<Vec<Option<u32>>, MfdError> {
    let wrappers: Vec<_> = component
        .children()
        .filter(|n| n.has_tag_name(kind))
        .collect();
    if wrappers.len() > 1 {
        return Err(refuse("ambiguous function pins"));
    }
    let Some(wrapper) = wrappers.first() else {
        return Ok(vec![]);
    };
    let mut result = Vec::new();
    for (index, pin) in wrapper.children().filter(|n| n.is_element()).enumerate() {
        if !pin.has_tag_name("datapoint")
            || index >= 16
            || pin.attributes().any(|a| !matches!(a.name(), "key" | "pos"))
        {
            return Err(refuse("unqualified function pin"));
        }
        if pin.attribute("key").is_some() {
            if pin.attribute("pos").and_then(|s| s.parse::<usize>().ok()) != Some(index) {
                return Err(refuse("invalid pin position"));
            }
            result.push(Some(number(pin.attribute("key"))?));
        } else {
            if pin.attributes().len() != 0 {
                return Err(refuse("invalid empty pin"));
            }
            result.push(None);
        }
    }
    Ok(result)
}

impl Builder {
    // A pure sibling-String conditional has one predicate occurrence loop and
    // a separately reduced value sequence. It is not a Cartesian scalar call.
    fn sibling_string_conditional(
        &self,
        feed: u32,
    ) -> Option<(u32, Option<u32>, SiblingStringSelection)> {
        self.parameter_key?;
        let branch = self.functions.get(&feed)?;
        if branch.parsed.library != "core"
            || branch.parsed.name != "if-else"
            || branch.parsed.kind != 4
            || !branch.metadata_valid
            || !branch.parsed.raw_single_output_valid
            || branch.inputs.len() != 3
        {
            return None;
        }
        let condition_feed = self.input(branch, 0).ok()?;
        let condition = self.functions.get(&condition_feed)?;
        if condition.parsed.library != "core"
            || condition.parsed.name != "equal"
            || condition.parsed.kind != 5
            || !condition.metadata_valid
            || !condition.parsed.raw_single_output_valid
            || condition.inputs.len() != 2
        {
            return None;
        }
        let left = self.input(condition, 0).ok()?;
        let right = self.input(condition, 1).ok()?;
        let is_literal = |key| {
            self.functions.get(&key).is_some_and(|value| {
                value.parsed.kind == 2
                    && value.parsed.library == "core"
                    && value.literal_valid
                    && value.metadata_valid
                    && value
                        .parsed
                        .constant
                        .as_ref()
                        .is_some_and(|(_, ty)| ty == "string")
            })
        };
        let guard = match (
            self.port_kinds.get(&left).map(String::as_str),
            self.port_kinds.get(&right).map(String::as_str),
        ) {
            (Some("string"), _) if is_literal(right) => left,
            (_, Some("string")) if is_literal(left) => right,
            _ => return None,
        };
        let value = self.input(branch, 1).ok()?;
        let guard_path = self.ports.get(&guard)?;
        let (_, guard_owner) = guard_path.split_last()?;
        let sibling = |port| {
            self.port_kinds.get(&port).map(String::as_str) == Some("string")
                && self
                    .ports
                    .get(&port)
                    .and_then(|path| path.split_last())
                    .is_some_and(|(_, owner)| !owner.is_empty() && owner == guard_owner)
        };
        let selection = if value != guard && sibling(value) {
            SiblingStringSelection::Direct(value)
        } else {
            let choice = self.functions.get(&value)?;
            if choice.parsed.library != "core"
                || choice.parsed.name != "if-else"
                || choice.parsed.kind != 4
                || !choice.metadata_valid
                || !choice.parsed.raw_single_output_valid
                || choice.inputs.len() != 3
            {
                return None;
            }
            let empty = self.functions.get(&self.input(choice, 0).ok()?)?;
            if empty.parsed.library != "lang"
                || empty.parsed.name != "empty"
                || empty.parsed.kind != 5
                || !empty.metadata_valid
                || !empty.parsed.raw_single_output_valid
                || empty.inputs.len() != 1
            {
                return None;
            }
            let source = self.input(empty, 0).ok()?;
            let fallback = self.input(choice, 1).ok()?;
            if self.input(choice, 2).ok()? != source
                || source == guard
                || fallback == guard
                || fallback == source
                || !sibling(source)
                || !sibling(fallback)
            {
                return None;
            }
            SiblingStringSelection::EmptyFallback { source, fallback }
        };
        let predicate = if branch.inputs[2].is_some() {
            if self.input(branch, 2).ok()? != value {
                return None;
            }
            None
        } else {
            if !matches!(selection, SiblingStringSelection::Direct(_)) {
                return None;
            }
            Some(condition_feed)
        };
        Some((guard, predicate, selection))
    }

    fn sibling_path(&self, source: u32, anchor: &[String]) -> Result<Vec<String>, MfdError> {
        self.relative_source(&self.ports[&source], anchor)?;
        let common = self.ports[&source]
            .iter()
            .zip(anchor)
            .take_while(|(left, right)| left == right)
            .count();
        let mut owner = &self.source_schema;
        let mut repeating_owner = 0;
        for (index, name) in anchor[..common].iter().enumerate() {
            owner = owner
                .child(name)
                .ok_or_else(|| refuse("missing sibling owner"))?;
            if owner.repeating {
                repeating_owner = index + 1;
            }
        }
        Ok(self.ports[&source][repeating_owner..].to_vec())
    }

    fn last_sibling_string(&mut self, source: u32, anchor: &[String]) -> Result<NodeId, MfdError> {
        let value = self.compile(source, Some(source), anchor)?;
        self.alloc(Node::Aggregate {
            function: AggregateOp::LastPresentText,
            collection: self.sibling_path(source, anchor)?,
            value: vec![],
            expression: Some(value),
            arg: None,
        })
    }

    fn sibling_conditional_value(
        &mut self,
        feed: u32,
        active_leaf: Option<u32>,
        anchor: &[String],
    ) -> Result<Option<NodeId>, MfdError> {
        let Some((guard, condition, value)) = self.sibling_string_conditional(feed) else {
            return Ok(None);
        };
        let evaluation_anchor = if active_leaf == Some(guard) {
            anchor.to_vec()
        } else {
            self.ports[&guard].clone()
        };
        let predicate = match condition {
            Some(condition) => self.compile(condition, Some(guard), &evaluation_anchor)?,
            None => self.constant(Value::Bool(true))?,
        };
        let selected = match value {
            SiblingStringSelection::Direct(source) => {
                self.last_sibling_string(source, &evaluation_anchor)?
            }
            SiblingStringSelection::EmptyFallback { source, fallback } => {
                let source_anchor = self.ports[&source].clone();
                let value = self.compile(source, Some(source), &source_anchor)?;
                let empty = self.call("is_empty", vec![value])?;
                let fallback = self.last_sibling_string(fallback, &source_anchor)?;
                let expression = self.alloc(Node::If {
                    condition: empty,
                    then: fallback,
                    else_: value,
                })?;
                self.alloc(Node::Aggregate {
                    function: AggregateOp::LastPresentText,
                    collection: self.sibling_path(source, &evaluation_anchor)?,
                    value: vec![],
                    expression: Some(expression),
                    arg: None,
                })?
            }
        };
        let absent = self.constant(Value::Null)?;
        let expression = self.alloc(Node::If {
            condition: predicate,
            then: selected,
            else_: absent,
        })?;
        if active_leaf == Some(guard) {
            return Ok(Some(expression));
        }
        let node = self.alloc(Node::Aggregate {
            function: AggregateOp::FirstNonemptyText,
            collection: self.relative_source(&self.ports[&guard], anchor)?,
            value: vec![],
            expression: Some(expression),
            arg: None,
        })?;
        Ok(Some(node))
    }

    fn sibling_udf_value(
        &mut self,
        feed: u32,
        active_leaf: Option<u32>,
        anchor: &[String],
    ) -> Result<Option<NodeId>, MfdError> {
        use super::udf::ScalarExpr;
        let Some(call) = self.udf_outputs.get(&feed).cloned() else {
            return Ok(None);
        };
        if let ScalarExpr::Parameter(parameter) = call.expression {
            let Some(input) = call.inputs.get(&parameter) else {
                return Ok(None);
            };
            let source = self.feed(*input)?;
            return self.sibling_conditional_value(source, active_leaf, anchor);
        }
        let ScalarExpr::If {
            condition,
            then,
            else_,
        } = &call.expression
        else {
            return Ok(None);
        };
        let ScalarExpr::Call { function, args } = &**condition else {
            return Ok(None);
        };
        if function != "not" || !matches!(&**then, ScalarExpr::Const(Value::Null)) {
            return Ok(None);
        }
        let [ScalarExpr::Call { function, args }] = args.as_slice() else {
            return Ok(None);
        };
        if function != "exists" {
            return Ok(None);
        }
        let [ScalarExpr::Parameter(value_parameter)] = args.as_slice() else {
            return Ok(None);
        };
        let ScalarExpr::Parameter(qualifier_parameter) = &**else_ else {
            return Ok(None);
        };
        let Some(input) = call.inputs.get(value_parameter) else {
            return Ok(None);
        };
        let source = self.feed(*input)?;
        let Some((guard, _, _)) = self.sibling_string_conditional(source) else {
            return Ok(None);
        };
        let Some(input) = call.inputs.get(qualifier_parameter) else {
            return Ok(None);
        };
        let qualifier = self.feed(*input)?;
        if !self.functions.get(&qualifier).is_some_and(|function| {
            function.parsed.library == "core"
                && function.parsed.kind == 2
                && function.literal_valid
                && function
                    .parsed
                    .constant
                    .as_ref()
                    .is_some_and(|(_, ty)| ty == "string")
        }) {
            return Ok(None);
        }
        // Presence is independent of the writer's first-nonempty reduction:
        // a selected empty String still admits the constant qualifier.
        let guard_path = self.ports[&guard].clone();
        let selected = self
            .sibling_conditional_value(source, Some(guard), &guard_path)?
            .unwrap();
        let predicate = self.call("exists", vec![selected])?;
        let value = self.compile(qualifier, None, anchor)?;
        if active_leaf == Some(guard) {
            let absent = self.constant(Value::Null)?;
            return self
                .alloc(Node::If {
                    condition: predicate,
                    then: value,
                    else_: absent,
                })
                .map(Some);
        }
        self.alloc(Node::CollectionFind {
            collection: self.relative_source(&guard_path, anchor)?,
            predicate,
            value,
        })
        .map(Some)
    }

    fn ancestor_empty_group(&self, feed: u32, anchor: &[String]) -> Option<(u32, u32)> {
        use super::udf::ScalarExpr;
        self.parameter_key?;
        let branch = self.functions.get(&feed)?;
        if branch.parsed.library != "core"
            || branch.parsed.name != "if-else"
            || branch.parsed.kind != 4
            || !branch.metadata_valid
            || !branch.parsed.raw_single_output_valid
            || branch.inputs.len() != 3
            || branch.inputs[2].is_some()
        {
            return None;
        }
        let empty = self.functions.get(&self.input(branch, 0).ok()?)?;
        if empty.parsed.library != "lang"
            || empty.parsed.name != "empty"
            || empty.parsed.kind != 5
            || !empty.metadata_valid
            || !empty.parsed.raw_single_output_valid
            || empty.inputs.len() != 1
        {
            return None;
        }
        let guard = self.input(empty, 0).ok()?;
        let call = self.udf_outputs.get(&self.input(branch, 1).ok()?)?;
        let ScalarExpr::If {
            condition,
            then,
            else_,
        } = &call.expression
        else {
            return None;
        };
        let ScalarExpr::Call { function, args } = &**condition else {
            return None;
        };
        if function != "not" || !matches!(&**then, ScalarExpr::Const(Value::Null)) {
            return None;
        }
        let [ScalarExpr::Call { function, args }] = args.as_slice() else {
            return None;
        };
        if function != "exists" {
            return None;
        }
        let [ScalarExpr::Parameter(value_parameter)] = args.as_slice() else {
            return None;
        };
        let ScalarExpr::Parameter(qualifier_parameter) = &**else_ else {
            return None;
        };
        let value = self.feed(*call.inputs.get(value_parameter)?).ok()?;
        let qualifier = self.feed(*call.inputs.get(qualifier_parameter)?).ok()?;
        let literal = self.functions.get(&qualifier)?;
        if literal.parsed.library != "core"
            || literal.parsed.kind != 2
            || !literal.literal_valid
            || !literal.metadata_valid
            || !literal
                .parsed
                .constant
                .as_ref()
                .is_some_and(|(_, ty)| ty == "string")
        {
            return None;
        }
        if self.port_kinds.get(&guard).map(String::as_str) != Some("string")
            || self.port_kinds.get(&value).map(String::as_str) != Some("string")
        {
            return None;
        }
        let (_, guard_owner) = self.ports.get(&guard)?.split_last()?;
        let (_, value_owner) = self.ports.get(&value)?.split_last()?;
        // Admit only a direct value in this detail object and a guard in one
        // enclosing object. Never search across unrelated siblings or rows.
        if anchor != value_owner
            || guard_owner.len() >= value_owner.len()
            || !value_owner.starts_with(guard_owner)
        {
            return None;
        }
        Some((guard, value))
    }

    fn bind_parameter_group(
        &mut self,
        filename: u32,
        parameter: &ImportedJsonParameterProjection,
    ) -> Result<mapping::JsonParameterInvocationPlan, MfdError> {
        fn visit(
            builder: &Builder,
            parameter: &ImportedJsonParameterProjection,
            feed: u32,
            active: &mut BTreeSet<u32>,
            found: &mut BTreeMap<u32, (String, u32, u32)>,
        ) -> Result<(), MfdError> {
            if builder.ports.contains_key(&feed) || builder.filename_port == Some(feed) {
                return Ok(());
            }
            if parameter.ports.contains_key(&feed) {
                return Err(refuse(
                    "parameter filename value is not an outer grouping key",
                ));
            }
            if !active.insert(feed) {
                return Err(refuse("cyclic parameter filename"));
            }
            if let Some(call) = builder.udf_outputs.get(&feed) {
                for input in call.inputs.values() {
                    visit(builder, parameter, builder.feed(*input)?, active, found)?;
                }
            } else {
                let function = builder
                    .functions
                    .get(&feed)
                    .ok_or_else(|| refuse("unknown parameter filename feed"))?;
                if function.parsed.library == "core" && function.parsed.name == "group-by" {
                    let source = builder.input(function, 0)?;
                    if let Some(path) = parameter.ports.get(&source) {
                        if function.parsed.kind != 5
                            || !function.metadata_valid
                            || !matches!(function.parsed.output_pins.as_slice(), [None, Some(port)] if *port == feed)
                            || function.inputs.len() != 2
                            || path != &["Documents".to_string(), "Root".to_string()]
                        {
                            return Err(refuse("unqualified root parameter grouping"));
                        }
                        let key = builder.input(function, 1)?;
                        let key_path = parameter.ports.get(&key).ok_or_else(|| {
                            refuse("parameter grouping needs a direct String member")
                        })?;
                        if key_path.len() != 3 || key_path[..2] != path[..] {
                            return Err(refuse("parameter key crosses its root object"));
                        }
                        found.insert(feed, (key_path[2].clone(), source, key));
                    } else if !builder.ports.contains_key(&source) {
                        return Err(refuse("unknown outer grouping source"));
                    }
                } else if function.parsed.kind != 6 {
                    for input in function.inputs.iter().flatten() {
                        visit(builder, parameter, builder.feed(*input)?, active, found)?;
                    }
                }
            }
            active.remove(&feed);
            Ok(())
        }
        let mut found = BTreeMap::new();
        visit(self, parameter, filename, &mut BTreeSet::new(), &mut found)?;
        if found.len() != 1 {
            return Err(refuse("requires one root String parameter grouping"));
        }
        let (output, (member, source, key)) = found.pop_first().unwrap();
        let function = self.functions[&output].clone();
        for (input, feed) in &self.feeds {
            if parameter.ports.contains_key(feed)
                && ![
                    (function.inputs[0].unwrap(), source),
                    (function.inputs[1].unwrap(), key),
                ]
                .contains(&(*input, *feed))
            {
                return Err(refuse("parameter group has an unqualified consumer"));
            }
        }
        let binding = format!("__ferrule_parameter_group_{output}");
        if self.functions.values().any(|function| {
            function
                .parsed
                .input_parameter_name
                .as_ref()
                .is_some_and(|parameter| parameter.name == binding)
        }) {
            return Err(refuse(
                "parameter group binding collides with a declared input",
            ));
        }
        let plan = mapping::JsonParameterInvocationPlan {
            parameter: parameter.parameter_name.clone(),
            key_member: member,
            iteration: mapping::JsonParameterIteration::DistinctRootStringKeys,
            key_parameter: binding.clone(),
            projection: parameter.shape.clone(),
        };
        plan.validate().map_err(|error| refuse(error.to_string()))?;
        let node = self.alloc(Node::RuntimeParameter {
            name: binding,
            ty: ScalarType::String,
            preview: None,
        })?;
        self.parameter_key = Some((output, node));
        self.parameter_dependent_feeds.insert(output);
        let mut users = BTreeMap::<_, Vec<_>>::new();
        for (output, function) in &self.functions {
            for input in function.inputs.iter().flatten() {
                if let Some(feed) = self.feeds.get(input) {
                    users.entry(*feed).or_default().push(*output);
                }
            }
        }
        for (output, call) in &self.udf_outputs {
            for input in call.inputs.values() {
                if let Some(feed) = self.feeds.get(input) {
                    users.entry(*feed).or_default().push(*output);
                }
            }
        }
        let mut pending = vec![output];
        while let Some(feed) = pending.pop() {
            for user in users.get(&feed).into_iter().flatten() {
                if self.parameter_dependent_feeds.insert(*user) {
                    pending.push(*user);
                }
            }
        }
        Ok(plan)
    }

    fn grouped_substitution(&self, function: &Function) -> bool {
        self.grouped.is_some()
            && function.parsed.library == "db"
            && function.parsed.name == "substitute-null"
            && function.inputs.len() == 2
            && self.input(function, 0).ok().is_some_and(|feed| {
                matches!(
                    self.port_kinds.get(&feed).map(String::as_str),
                    Some("string" | "number")
                )
            })
            && self.input(function, 1).ok().is_some_and(|feed| {
                matches!(
                    self.port_kinds.get(&feed).map(String::as_str),
                    Some("string" | "number")
                ) || self
                    .functions
                    .get(&feed)
                    .is_some_and(|f| f.parsed.kind == 2)
            })
    }
    fn grouped_sum(&self, function: &Function) -> bool {
        self.grouped.is_some()
            && function.parsed.library == "core"
            && function.parsed.name == "sum"
            && function.inputs.len() == 2
            && function.inputs[0].is_none()
            && self.input(function, 1).ok().is_some_and(|feed| {
                self.port_kinds.get(&feed).map(String::as_str) == Some("number")
            })
    }

    fn context_sum_value(
        &mut self,
        feed: u32,
        anchor: &[String],
    ) -> Result<Option<NodeId>, MfdError> {
        if self.parameter_key.is_none() {
            return Ok(None);
        }
        let Some(function) = self.functions.get(&feed).cloned() else {
            return Ok(None);
        };
        if function.parsed.library != "core"
            || function.parsed.name != "sum"
            || function.parsed.kind != 5
            || !function.metadata_valid
            || !function.parsed.raw_single_output_valid
            || function.inputs.len() != 2
            || function.inputs.iter().any(Option::is_none)
        {
            return Ok(None);
        }
        let context = self.input(&function, 0)?;
        let number = self.input(&function, 1)?;
        if self.port_kinds.get(&context).map(String::as_str) != Some("object")
            || self.port_kinds.get(&number).map(String::as_str) != Some("number")
        {
            return Ok(None);
        }
        let context_path = self.ports[&context].clone();
        if self.ports[&number]
            .strip_prefix(context_path.as_slice())
            .is_none_or(|suffix| suffix.len() != 1)
        {
            return Ok(None);
        }
        let raw = self.alloc(Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        })?;
        let number_value = self.numeric_source_value(number, raw)?;
        let subtotal = self.alloc(Node::Aggregate {
            function: AggregateOp::DecimalSum,
            collection: self.sibling_path(number, &context_path)?,
            value: vec![],
            expression: Some(number_value),
            arg: None,
        })?;
        if anchor == context_path {
            return Ok(Some(subtotal));
        }
        let text = self.call("string", vec![subtotal])?;
        self.alloc(Node::Aggregate {
            function: AggregateOp::FirstNonemptyText,
            collection: self.relative_source(&context_path, anchor)?,
            value: vec![],
            expression: Some(text),
            arg: None,
        })
        .map(Some)
    }
    fn bind_grouped_source(
        &mut self,
        filename: u32,
        shape: &mapping::JsonSourceShape,
    ) -> Result<(), MfdError> {
        fn visit(
            builder: &Builder,
            feed: u32,
            active: &mut BTreeSet<u32>,
            found: &mut BTreeMap<u32, (Vec<String>, u32)>,
        ) -> Result<(), MfdError> {
            if builder.ports.contains_key(&feed)
                || builder.filename_port == Some(feed)
                || builder
                    .parameter_key
                    .is_some_and(|(output, _)| output == feed)
            {
                return Ok(());
            }
            if !active.insert(feed) {
                return Err(refuse("cyclic grouped filename"));
            }
            if let Some(call) = builder.udf_outputs.get(&feed) {
                for input in call.inputs.values() {
                    visit(builder, builder.feed(*input)?, active, found)?;
                }
            } else {
                let function = builder
                    .functions
                    .get(&feed)
                    .ok_or_else(|| refuse("unknown grouped filename feed"))?;
                if function.parsed.library == "core" && function.parsed.name == "group-by" {
                    if function.parsed.kind != 5
                        || !function.metadata_valid
                        || !matches!(function.parsed.output_pins.as_slice(),[None,Some(port)] if *port==feed)
                        || function.inputs.len() != 2
                    {
                        return Err(refuse(
                            "grouped filename requires the declared group key output",
                        ));
                    }
                    let source = builder.input(function, 0)?;
                    let key = builder.input(function, 1)?;
                    let collection = builder
                        .ports
                        .get(&source)
                        .filter(|_| {
                            let kind = builder.port_kinds.get(&source).map(String::as_str);
                            kind == Some("object")
                                || builder.parameter_key.is_some() && kind == Some("array")
                        })
                        .ok_or_else(|| {
                            refuse("grouped filename requires supported source containers")
                        })?
                        .clone();
                    let key_path = builder
                        .ports
                        .get(&key)
                        .filter(|_| {
                            builder.port_kinds.get(&key).map(String::as_str) == Some("string")
                        })
                        .ok_or_else(|| refuse("grouped filename requires a String key"))?;
                    if key_path
                        .strip_prefix(collection.as_slice())
                        .is_none_or(|path| path.is_empty())
                    {
                        return Err(refuse("group key crosses its source object owner"));
                    }
                    found.insert(feed, (collection, key));
                } else if function.parsed.kind != 6 {
                    for input in function.inputs.iter().flatten() {
                        visit(builder, builder.feed(*input)?, active, found)?;
                    }
                }
            }
            active.remove(&feed);
            Ok(())
        }
        let mut found = BTreeMap::new();
        visit(self, filename, &mut BTreeSet::new(), &mut found)?;
        if found.len() != 1 {
            return Err(refuse(
                "requires one source grouping in the output filename",
            ));
        }
        let (key_output, (collection, key)) = found.pop_first().unwrap();
        let key_path = relative(&self.ports[&key], &collection)?;
        let physical = collection
            .strip_prefix(&["Documents".to_string()])
            .ok_or_else(|| refuse("grouping requires a physical JSON document"))?;
        let preparation = mapping::JsonGroupingPreparation {
            collection: physical.to_vec(),
            keys: key_path,
        };
        self.source_schema = preparation
            .document_set_schema(shape)
            .map_err(|_| refuse("invalid grouping preparation"))?;
        for path in self.ports.values_mut() {
            if let Some(relative) = path.strip_prefix(collection.as_slice()) {
                let mut prepared = collection.clone();
                prepared.push(mapping::JsonGroupingPreparation::MEMBER.into());
                prepared.extend_from_slice(relative);
                *path = prepared;
            }
        }
        self.grouping_preparation = Some(preparation);
        let key_node = self.alloc(Node::SourceField {
            path: vec![mapping::JsonGroupingPreparation::KEY.into()],
            frame: None,
        })?;
        self.grouped = Some(GroupedSource {
            collection,
            key_output,
            key_node,
            key_path: vec![mapping::JsonGroupingPreparation::KEY.into()],
            filename_feed: filename,
        });
        Ok(())
    }

    fn relative_source(&self, path: &[String], anchor: &[String]) -> Result<Vec<String>, MfdError> {
        if path.starts_with(anchor) {
            let mut result = relative(path, anchor)?;
            if let Some(group) = &self.grouped
                && anchor == group.collection
                && !self.in_group_member
            {
                result.insert(
                    0,
                    group
                        .collection
                        .last()
                        .ok_or_else(|| refuse("empty grouped source"))?
                        .clone(),
                );
            }
            return Ok(result);
        }
        let Some(group) = &self.grouped else {
            return relative(path, anchor);
        };
        let common = path
            .iter()
            .zip(anchor)
            .take_while(|(left, right)| left == right)
            .count();
        if common < group.collection.len() || common == path.len() {
            return Err(refuse("source collection escapes its grouped owner"));
        }
        let head = &path[common];
        for end in common + 1..=anchor.len() {
            let node = anchor[..end]
                .iter()
                .try_fold(&self.source_schema, |node, name| node.child(name))
                .ok_or_else(|| refuse("unknown grouped source frame"))?;
            if node.child(head).is_some() {
                return Err(refuse("ambiguous inherited grouped source field"));
            }
        }
        Ok(path[common..].to_vec())
    }

    // The IR keeps well-formed Unicode. A split surrogate may be replaced only
    // where it goes directly to the Latin-1 wire or an ASCII equality test.
    fn require_wire_left(&self, feed: u32) -> Result<(), MfdError> {
        let function = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unknown left operator"))?;
        let count = self
            .functions
            .get(&self.input(function, 1)?)
            .and_then(|f| f.parsed.constant.as_ref())
            .and_then(|(s, t)| (t == "decimal").then(|| s.parse::<f64>().ok()).flatten())
            .filter(|n| n.is_finite() && *n >= 0.0 && *n <= i32::MAX as f64 && n.fract() == 0.0);
        if count.is_none() {
            return Err(refuse("UTF-16 prefix requires a bounded literal count"));
        }
        for (input, output) in &self.feeds {
            if *output != feed || self.string_targets.contains(input) {
                continue;
            }
            let owner = self
                .functions
                .values()
                .find(|f| f.inputs.contains(&Some(*input)))
                .ok_or_else(|| refuse("unqualified UTF-16 prefix consumer"))?;
            if owner.parsed.library != "core"
                || owner.parsed.name != "equal"
                || owner.inputs.len() != 2
            {
                return Err(refuse(
                    "UTF-16 prefix cannot escape the qualified wire path",
                ));
            }
            let other = owner
                .inputs
                .iter()
                .flatten()
                .find(|key| **key != *input)
                .ok_or_else(|| refuse("ambiguous prefix comparison"))?;
            let literal = self
                .functions
                .get(&self.feed(*other)?)
                .and_then(|f| f.parsed.constant.as_ref());
            if !literal.is_some_and(|(value, _)| value.is_ascii()) {
                return Err(refuse("prefix comparison requires an ASCII literal"));
            }
        }
        Ok(())
    }
    fn alloc(&mut self, node: Node) -> Result<NodeId, MfdError> {
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| refuse("node limit"))?;
        self.graph.nodes.insert(id, node);
        Ok(id)
    }
    fn constant(&mut self, value: Value) -> Result<NodeId, MfdError> {
        self.alloc(Node::Const { value })
    }
    fn call(&mut self, name: &str, args: Vec<NodeId>) -> Result<NodeId, MfdError> {
        self.alloc(Node::Call {
            function: name.into(),
            args,
        })
    }
    fn feed(&self, input: u32) -> Result<u32, MfdError> {
        self.feeds
            .get(&input)
            .copied()
            .ok_or_else(|| refuse("missing input feed"))
    }
    fn dependencies(&self, feed: u32, seen: &mut BTreeSet<u32>) -> Result<BTreeSet<u32>, MfdError> {
        if self.parameter_key.is_some_and(|(output, _)| output == feed) {
            return Ok(BTreeSet::new());
        }
        if self
            .grouped
            .as_ref()
            .is_some_and(|group| group.key_output == feed)
        {
            return Ok(BTreeSet::new());
        }
        if self.ports.contains_key(&feed) || self.filename_port == Some(feed) {
            return Ok(BTreeSet::from([feed]));
        }
        if !seen.insert(feed) {
            return Err(refuse("cyclic dependency"));
        }
        if self.functions.get(&feed).is_some_and(|function| {
            self.grouped_substitution(function) || self.grouped_sum(function)
        }) {
            seen.remove(&feed);
            return Ok(BTreeSet::new());
        }
        if let Some(call) = self.udf_outputs.get(&feed) {
            let mut result = BTreeSet::new();
            for input in call.inputs.values() {
                result.extend(self.dependencies(self.feed(*input)?, seen)?);
            }
            seen.remove(&feed);
            return Ok(result);
        }
        let function = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unknown feed"))?;
        let mut result = BTreeSet::new();
        if function.parsed.kind != 6 && function.parsed.name != "count" {
            for input in function.inputs.iter().flatten() {
                result.extend(self.dependencies(self.feed(*input)?, seen)?);
            }
        }
        seen.remove(&feed);
        Ok(result)
    }
    fn compile(
        &mut self,
        feed: u32,
        leaf: Option<u32>,
        anchor: &[String],
    ) -> Result<NodeId, MfdError> {
        if !self.filename_phase && self.parameter_dependent_feeds.contains(&feed) {
            return Err(refuse(
                "parameter grouping is only admitted in the output filename",
            ));
        }
        let key = (feed, leaf, anchor.to_vec(), self.in_group_member);
        if let Some(node) = self.cache.get(&key) {
            return Ok(*node);
        }
        if !self.active.insert(key.clone()) {
            return Err(refuse("cyclic expression"));
        }
        let result = self.compile_inner(feed, leaf, anchor);
        self.active.remove(&key);
        let node = result?;
        self.cache.insert(key, node);
        Ok(node)
    }
    fn input(&self, f: &Function, index: usize) -> Result<u32, MfdError> {
        let input = f
            .inputs
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| refuse("missing operand"))?;
        self.feed(input)
    }
    fn compile_inner(
        &mut self,
        feed: u32,
        leaf: Option<u32>,
        anchor: &[String],
    ) -> Result<NodeId, MfdError> {
        if let Some((output, node)) = self.parameter_key
            && output == feed
        {
            return Ok(node);
        }
        if let Some(group) = &self.grouped
            && group.key_output == feed
        {
            if anchor != group.collection {
                return Err(refuse("group key used outside its filename owner"));
            }
            let collection = vec![
                group
                    .collection
                    .last()
                    .ok_or_else(|| refuse("empty grouped source"))?
                    .clone(),
            ];
            let key_path = group.key_path.clone();
            let value = self.alloc(Node::SourceField {
                path: key_path,
                frame: None,
            })?;
            let first = self.constant(Value::Int(1))?;
            return self.alloc(Node::Aggregate {
                function: AggregateOp::ItemAt,
                collection,
                value: vec![],
                expression: Some(value),
                arg: Some(first),
            });
        }
        if let Some(path) = self.ports.get(&feed).cloned() {
            if leaf != Some(feed) || !matches!(self.port_kinds[&feed].as_str(), "string" | "number")
            {
                return Err(refuse("unframed or non-scalar source value"));
            }
            let _ = path;
            let value = self.alloc(Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            })?;
            return if self.port_kinds[&feed] == "number" {
                let number = self.numeric_source_value(feed, value)?;
                self.call("string", vec![number])
            } else {
                Ok(value)
            };
        }
        if let Some(call) = self.udf_outputs.get(&feed).cloned() {
            return self.compile_udf(&call.expression, &call.inputs, leaf, anchor, 0);
        }
        let f = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unsupported feed owner"))?
            .clone();
        if !f.parsed.raw_single_output_valid || !f.metadata_valid {
            return Err(refuse("unqualified function output"));
        }
        if self.grouped_substitution(&f) {
            let left = self.reduced_value(self.input(&f, 0)?, None, anchor, true)?;
            let right = self.reduced_value(self.input(&f, 1)?, None, anchor, true)?;
            let condition = self.call("exists", vec![left])?;
            return self.alloc(Node::If {
                condition,
                then: left,
                else_: right,
            });
        }
        if self.grouped_sum(&f) {
            let input = self.input(&f, 1)?;
            let value = self.compile(input, Some(input), anchor)?;
            let value = self.call("decimal_from_nullable_text", vec![value])?;
            return self.alloc(Node::Aggregate {
                function: AggregateOp::DecimalSum,
                collection: self.relative_source(&self.ports[&input], anchor)?,
                value: vec![],
                expression: Some(value),
                arg: None,
            });
        }
        if f.parsed.kind == 2 {
            if f.parsed.library != "core" || !f.inputs.is_empty() {
                return Err(refuse("unqualified literal signature"));
            }
            let (text, ty) = f
                .parsed
                .constant
                .as_ref()
                .ok_or_else(|| refuse("invalid constant"))?;
            if !f.literal_valid {
                return Err(refuse(format!("ambiguous literal at output {feed}")));
            }
            let value = match ty.as_str() {
                "string" => Value::String(text.clone()),
                "decimal" => Value::Float(
                    text.parse::<f64>()
                        .ok()
                        .filter(|x| x.is_finite())
                        .ok_or_else(|| refuse("invalid numeric literal"))?,
                ),
                _ => return Err(refuse("unsupported constant type")),
            };
            return self.constant(value);
        }
        if f.parsed.kind == 6 {
            if f.parsed.library != "core" {
                return Err(refuse("unqualified parameter library"));
            }
            let param = f
                .parsed
                .input_parameter_name
                .as_ref()
                .filter(|p| !p.optional)
                .ok_or_else(|| refuse("unsupported parameter"))?;
            return self.alloc(Node::RuntimeParameter {
                name: param.name.clone(),
                ty: f
                    .parsed
                    .input_type
                    .ok_or_else(|| refuse("untyped parameter"))?,
                preview: f.parsed.input_preview.clone(),
            });
        }
        if f.parsed.library == "lang" && f.parsed.name == "now" && f.inputs.is_empty() {
            return self.alloc(Node::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            });
        }
        if f.parsed.name == "count" && f.parsed.library == "core" {
            if f.inputs.len() != 2 || f.inputs[0].is_some() {
                return Err(refuse("unsupported count signature"));
            }
            let key = self.input(&f, 1)?;
            let collection = self
                .ports
                .get(&key)
                .filter(|_| self.port_kinds[&key] == "object")
                .cloned()
                .ok_or_else(|| refuse("count must select source objects"))?;
            let collection = self.relative_source(&collection, anchor)?;
            return self.alloc(Node::Aggregate {
                function: AggregateOp::Count,
                collection,
                value: vec![],
                expression: None,
                arg: None,
            });
        }
        if f.parsed.name == "if-else" && f.parsed.kind == 4 {
            if f.parsed.library != "core" || f.inputs.len() != 3 {
                return Err(refuse(
                    "requires one core condition with then and optional else",
                ));
            }
            let condition = self.compile(self.input(&f, 0)?, leaf, anchor)?;
            let then_node = self.compile(self.input(&f, 1)?, leaf, anchor)?;
            let else_node = if f.inputs[2].is_some() {
                self.compile(self.input(&f, 2)?, leaf, anchor)?
            } else {
                self.constant(Value::Null)?
            };
            return self.alloc(Node::If {
                condition,
                then: then_node,
                else_: else_node,
            });
        }
        let (name, count) = match (f.parsed.library.as_str(), f.parsed.name.as_str()) {
            ("core", "concat") => ("concat", f.inputs.len()),
            ("core", "equal") => ("equal", 2),
            ("core", "logical-and") => ("and", 2),
            ("core", "string-length") => ("length_utf16", 1),
            ("lang", "left") => {
                self.require_wire_left(feed)?;
                ("left_utf16_lossy", 2)
            }
            ("core", "parse-date") => ("parse_date", 2),
            ("core", "format-date") => ("format_date", 2),
            ("core", "format-dateTime") => ("format_datetime_milliseconds", 2),
            _ => {
                return Err(refuse(format!(
                    "unsupported scalar operator {}:{}",
                    f.parsed.library, f.parsed.name
                )));
            }
        };
        let valid_arity = if matches!(f.parsed.name.as_str(), "format-dateTime" | "format-date") {
            (2..=3).contains(&f.inputs.len()) && f.inputs[2..].iter().all(Option::is_none)
        } else if f.parsed.name == "concat" {
            (2..=16).contains(&f.inputs.len())
        } else {
            f.inputs.len() == count
        };
        if f.parsed.kind != 5 || !valid_arity || f.inputs[..count].iter().any(Option::is_none) {
            return Err(refuse("unqualified scalar arity"));
        }
        let mut args = Vec::new();
        for index in 0..count {
            args.push(self.compile(self.input(&f, index)?, leaf, anchor)?);
        }
        if f.parsed.name == "parse-date"
            && self
                .functions
                .get(&self.input(&f, 1)?)
                .is_some_and(|picture| {
                    picture
                        .parsed
                        .constant
                        .as_ref()
                        .is_some_and(|(text, ty)| ty == "string" && text == "[Y,4-4][M,2-2][D,2-2]")
                })
        {
            // Native ParseDateTime consumes this fixed-width date prefix and
            // leaves a formatted time suffix unread. Ordinary parse_date stays
            // strict; qualify only the exact eight-character source picture.
            let width = self.constant(Value::Int(8))?;
            args[0] = self.call("left_utf16_lossy", vec![args[0], width])?;
        }
        self.call(name, args)
    }

    fn numeric_source_value(&mut self, feed: u32, raw: NodeId) -> Result<NodeId, MfdError> {
        let value = self.call("decimal_from_nullable_text", vec![raw])?;
        if self.integer_ports.contains(&feed) {
            self.call("decimal_truncate", vec![value])
        } else {
            Ok(value)
        }
    }

    fn compile_udf(
        &mut self,
        expression: &super::udf::ScalarExpr,
        inputs: &BTreeMap<u32, u32>,
        leaf: Option<u32>,
        anchor: &[String],
        depth: usize,
    ) -> Result<NodeId, MfdError> {
        use super::udf::ScalarExpr;
        if depth > 64 {
            return Err(refuse("embedded scalar expression depth exceeded"));
        }
        match expression {
            ScalarExpr::Parameter(parameter) => {
                let input = inputs
                    .get(parameter)
                    .copied()
                    .ok_or_else(|| refuse("missing scalar parameter binding"))?;
                self.compile(self.feed(input)?, leaf, anchor)
            }
            ScalarExpr::Const(value) => self.constant(value.clone()),
            ScalarExpr::RuntimeValue(value) => self.alloc(Node::RuntimeValue { value: *value }),
            ScalarExpr::Call { function, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.compile_udf(arg, inputs, leaf, anchor, depth + 1))
                    .collect::<Result<_, _>>()?;
                self.call(function, args)
            }
            ScalarExpr::If {
                condition,
                then,
                else_,
            } => {
                let condition = self.compile_udf(condition, inputs, leaf, anchor, depth + 1)?;
                let then = self.compile_udf(then, inputs, leaf, anchor, depth + 1)?;
                let else_ = self.compile_udf(else_, inputs, leaf, anchor, depth + 1)?;
                self.alloc(Node::If {
                    condition,
                    then,
                    else_,
                })
            }
            ScalarExpr::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                let input = self.compile_udf(input, inputs, leaf, anchor, depth + 1)?;
                self.alloc(Node::ValueMap {
                    input,
                    input_type: *input_type,
                    table: table.clone(),
                    default: default.clone(),
                })
            }
            _ => Err(refuse("unsupported embedded scalar expression")),
        }
    }
    fn reduced_value(
        &mut self,
        feed: u32,
        active_leaf: Option<u32>,
        anchor: &[String],
        retain_empty: bool,
    ) -> Result<NodeId, MfdError> {
        if let Some(sum) = self.context_sum_value(feed, anchor)? {
            return Ok(sum);
        }
        if let Some((guard, _)) = self.ancestor_empty_group(feed, anchor)
            && active_leaf == Some(guard)
        {
            // The enclosing group already admitted this exact guard occurrence.
            // Evaluate its selected value in the retained detail frame, rather
            // than collecting all sibling details through the ancestor frame.
            let branch = self.functions[&feed].clone();
            return self.reduced_value(self.input(&branch, 1)?, None, anchor, retain_empty);
        }
        let dependencies = self.dependencies(feed, &mut BTreeSet::new())?;
        if dependencies.len() > 1 {
            if let Some(node) = self.sibling_udf_value(feed, active_leaf, anchor)? {
                return Ok(node);
            }
            if let Some(node) = self.sibling_conditional_value(feed, active_leaf, anchor)? {
                return Ok(node);
            }
            return Err(refuse(format!(
                "scalar output {feed} combines distinct source occurrences {dependencies:?}"
            )));
        }
        let dependency = dependencies.first().copied();
        let node = if let Some(leaf) = dependency {
            if !matches!(
                self.port_kinds.get(&leaf).map(String::as_str),
                Some("string" | "number")
            ) {
                return Err(refuse("non-scalar field dependency"));
            }
            let value = self.compile(feed, Some(leaf), anchor)?;
            if active_leaf == Some(leaf) {
                value
            } else {
                let value = self.call("string", vec![value])?;
                let arg = if retain_empty {
                    Some(self.constant(Value::Int(1))?)
                } else {
                    None
                };
                self.alloc(Node::Aggregate {
                    function: if retain_empty {
                        AggregateOp::ItemAt
                    } else {
                        AggregateOp::FirstNonemptyText
                    },
                    collection: self.relative_source(&self.ports[&leaf], anchor)?,
                    value: vec![],
                    expression: Some(value),
                    arg,
                })?
            }
        } else {
            self.compile(feed, None, anchor)?
        };
        Ok(node)
    }
    fn binding(
        &mut self,
        feed: u32,
        active_leaf: Option<u32>,
        target: &SchemaNode,
        path: &[String],
        anchor: &[String],
        options: &mapping::FormatOptions,
    ) -> Result<NodeId, MfdError> {
        let mut node = self.reduced_value(feed, active_leaf, anchor, false)?;
        if self.functions.get(&feed).is_some_and(|function| {
            function.parsed.kind == 6 && function.parsed.input_type == Some(ScalarType::String)
        }) {
            let present = self.call("exists", vec![node])?;
            let message = self.constant(Value::String(
                "native mapped String parameter cannot be Null".into(),
            ))?;
            let missing = self.alloc(Node::Raise {
                message: Some(message),
            })?;
            node = self.alloc(Node::If {
                condition: present,
                then: node,
                else_: missing,
            })?;
        }
        if self
            .functions
            .get(&feed)
            .is_some_and(|f| f.parsed.name == "now")
        {
            let kind = options
                .edi_lexical_formats
                .iter()
                .find(|f| f.path() == path)
                .map(|f| f.kind());
            node = match kind {
                Some(
                    mapping::EdiLexicalKind::CompactDate6 | mapping::EdiLexicalKind::CompactDate8,
                ) => self.call("date_from_datetime", vec![node])?,
                Some(mapping::EdiLexicalKind::CompactTime { .. }) => {
                    self.call("time_from_datetime", vec![node])?
                }
                _ => return Err(refuse("untyped dateTime target conversion")),
            };
        }
        if let SchemaKind::Scalar { ty } = target.kind {
            if ty == ScalarType::String {
                node = self.call("string", vec![node])?;
            }
            if ty == ScalarType::Float {
                let condition = self.call("exists", vec![node])?;
                let lexical = self.call("string", vec![node])?;
                let converted = self.call("decimal_from_nullable_text", vec![lexical])?;
                let absent = self.constant(Value::Null)?;
                node = self.alloc(Node::If {
                    condition,
                    then: converted,
                    else_: absent,
                })?;
            }
        } else {
            return Err(refuse("non-scalar binding"));
        }
        Ok(node)
    }
    fn scope(
        &mut self,
        entry: roxmltree::Node<'_, '_>,
        schema: &SchemaNode,
        path: &[String],
        anchor: &[String],
        active_leaf: Option<u32>,
        options: &mapping::FormatOptions,
    ) -> Result<Scope, MfdError> {
        if !matches!(schema.kind, SchemaKind::Group { .. }) {
            return Err(refuse("group entry has scalar schema"));
        }
        if entry
            .attributes()
            .any(|a| !matches!(a.name(), "name" | "inpkey" | "expanded" | "clone"))
        {
            return Err(refuse("unsupported target entry modifiers"));
        }
        if entry
            .children()
            .any(|n| n.is_element() && !n.has_tag_name("entry"))
        {
            return Err(refuse("unsupported target entry child"));
        }
        let mut scope = Scope {
            target_field: schema.name.clone(),
            ..Default::default()
        };
        let mut current = anchor.to_vec();
        let parent_group_member = self.in_group_member;
        let mut leaf = active_leaf;
        if let Some(key) = entry.attribute("inpkey") {
            let feed = self.feed(number(Some(key))?)?;
            if self
                .grouped
                .as_ref()
                .is_some_and(|group| group.filename_feed == feed)
            {
                // This connector selects the already constructed document,
                // not another evaluation of its volatile filename expression.
            } else if self.filename_port == Some(feed) {
                current = vec!["Documents".into()];
                if current != anchor {
                    scope.iteration =
                        ScopeIteration::Source(self.relative_source(&current, anchor)?);
                }
            } else if let Some(source) = self.ports.get(&feed) {
                if self.port_kinds[&feed] != "object" && self.grouped.is_none() {
                    return Err(refuse("group needs source object driver"));
                }
                current = source.clone();
                if self.grouped.is_some()
                    && matches!(self.port_kinds[&feed].as_str(), "string" | "number")
                {
                    leaf = Some(feed);
                }
                if current == anchor {
                    if !self.in_group_member
                        && self
                            .grouped
                            .as_ref()
                            .is_some_and(|group| group.collection == current)
                    {
                        scope.iteration =
                            ScopeIteration::Source(self.relative_source(&current, anchor)?);
                        self.in_group_member = true;
                    } else {
                        scope.iteration = ScopeIteration::None;
                    }
                } else if schema.repeating {
                    scope.iteration =
                        ScopeIteration::Source(self.relative_source(&current, anchor)?);
                    self.in_group_member |= self.grouped.is_some();
                } else {
                    return Err(refuse("non-repeating group crosses a source owner"));
                }
            } else {
                let dependencies = self.dependencies(feed, &mut BTreeSet::new())?;
                if let Some((guard, value)) = self.ancestor_empty_group(feed, anchor) {
                    if !schema.repeating {
                        return Err(refuse(
                            "ancestor-controlled group requires a repeating target",
                        ));
                    }
                    let guard_path = self.ports[&guard].clone();
                    scope.iteration = ScopeIteration::Source(self.sibling_path(guard, anchor)?);
                    let guard_value = self.compile(guard, Some(guard), &guard_path)?;
                    let empty = self.call("is_empty", vec![guard_value])?;
                    let selected = self.compile(value, Some(value), anchor)?;
                    let first = self.constant(Value::Int(1))?;
                    let selected = self.alloc(Node::Aggregate {
                        function: AggregateOp::ItemAt,
                        collection: self.sibling_path(value, anchor)?,
                        value: vec![],
                        expression: Some(selected),
                        arg: Some(first),
                    })?;
                    let present = self.call("exists", vec![selected])?;
                    scope.filter = Some(self.call("and", vec![empty, present])?);
                    leaf = Some(guard);
                    // Keep the detail anchor for child fields. The extra
                    // ancestor-guard frame must not rebind their sibling reads.
                } else if let Some((guard, _, _)) = self.sibling_string_conditional(feed) {
                    current = self.ports[&guard].clone();
                    leaf = Some(guard);
                    let value = self
                        .sibling_conditional_value(feed, leaf, &current)?
                        .unwrap();
                    let present = self.call("exists", vec![value])?;
                    if current != anchor || active_leaf != Some(guard) {
                        scope.iteration =
                            ScopeIteration::Source(self.relative_source(&current, anchor)?);
                        self.in_group_member = true;
                        scope.filter = Some(present);
                    } else {
                        if !schema.repeating {
                            return Err(refuse(
                                "same-frame conditional group requires a repeating target",
                            ));
                        }
                        scope.iteration = ScopeIteration::OptionalPresence { condition: present };
                    }
                } else if let Some(&key) = dependencies.first() {
                    if dependencies.len() != 1 {
                        return Err(refuse("mixed group drivers"));
                    }
                    if self.grouped.is_some()
                        && matches!(
                            self.port_kinds.get(&key).map(String::as_str),
                            Some("string" | "number")
                        )
                    {
                        current = self.ports[&key].clone();
                        leaf = Some(key);
                        scope.iteration =
                            ScopeIteration::Source(self.relative_source(&current, anchor)?);
                        self.in_group_member = true;
                        let value = self.compile(feed, leaf, &current)?;
                        scope.filter = Some(self.call("exists", vec![value])?);
                    } else {
                        let f = self
                            .functions
                            .get(&feed)
                            .ok_or_else(|| refuse("unknown group driver"))?
                            .clone();
                        if f.parsed.name != "if-else"
                            || f.inputs.len() != 3
                            || f.inputs[2].is_some()
                        {
                            return Err(refuse("group conditional requires absent else"));
                        }
                        current = self.ports[&key].clone();
                        leaf = Some(key);
                        scope.iteration =
                            ScopeIteration::Source(self.relative_source(&current, anchor)?);
                        scope.filter = Some(self.compile(self.input(&f, 0)?, leaf, &current)?);
                    }
                } else {
                    let value = self.compile(feed, None, anchor)?;
                    scope.iteration = if self.grouped.is_some()
                        && !self
                            .functions
                            .get(&feed)
                            .is_some_and(|f| f.parsed.kind == 2)
                    {
                        ScopeIteration::OptionalPresence {
                            condition: self.call("exists", vec![value])?,
                        }
                    } else {
                        ScopeIteration::None
                    };
                }
            }
        }
        let mut child_positions = BTreeMap::<String, usize>::new();
        for child in entry.children().filter(|n| n.has_tag_name("entry")) {
            let name = child
                .attribute("name")
                .ok_or_else(|| refuse("unnamed target field"))?;
            let target = schema
                .child(name)
                .ok_or_else(|| refuse("target field missing from original schema"))?;
            let mut child_path = path.to_vec();
            child_path.push(name.into());
            if matches!(target.kind, SchemaKind::Scalar { .. }) {
                if let Some(key) = child.attribute("inpkey") {
                    if child
                        .attributes()
                        .any(|a| !matches!(a.name(), "name" | "inpkey" | "expanded"))
                    {
                        return Err(refuse("unsupported scalar target modifiers"));
                    }
                    let node = self.binding(
                        self.feed(number(Some(key))?)?,
                        leaf,
                        target,
                        &child_path,
                        &current,
                        options,
                    )?;
                    scope.bindings.push(Binding {
                        target_field: name.into(),
                        node,
                    });
                }
            } else {
                let mut next = self.scope(child, target, &child_path, &current, leaf, options)?;
                if self.grouped.is_some()
                    && child.attribute("inpkey").is_none()
                    && !has_bindings(&next)
                {
                    continue;
                }
                if let Some(&index) = child_positions.get(name) {
                    let prior = &mut scope.children[index];
                    next.target_field.clear();
                    let mut first = prior.clone();
                    first.target_field.clear();
                    let mut segments = match &first.iteration {
                        ScopeIteration::Concatenate(sequence) => {
                            sequence.iter().cloned().collect::<Vec<_>>()
                        }
                        _ => vec![first],
                    };
                    match &next.iteration {
                        ScopeIteration::Concatenate(sequence) => {
                            segments.extend(sequence.iter().cloned())
                        }
                        _ => segments.push(next),
                    }
                    if segments.len() > 16 {
                        return Err(refuse("too many cloned target branches"));
                    }
                    let first = segments.remove(0);
                    *prior = Scope {
                        target_field: name.into(),
                        iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, segments)),
                        ..Default::default()
                    };
                } else {
                    child_positions.insert(name.into(), scope.children.len());
                    scope.children.push(next);
                }
            }
        }
        self.in_group_member = parent_group_member;
        if schema.repeating && matches!(scope.iteration, ScopeIteration::None) {
            scope.target_field.clear();
            Ok(Scope {
                target_field: schema.name.clone(),
                iteration: ScopeIteration::Concatenate(ScopeSequence::new(scope, vec![])),
                ..Default::default()
            })
        } else {
            Ok(scope)
        }
    }
}

fn has_bindings(scope: &Scope) -> bool {
    !scope.bindings.is_empty()
        || scope.children.iter().any(has_bindings)
        || matches!(
            scope.iteration,
            ScopeIteration::Source(_) | ScopeIteration::OptionalPresence { .. }
        )
        || matches!(&scope.iteration, ScopeIteration::Concatenate(sequence) if sequence.iter().any(has_bindings))
}

#[cfg(test)]
mod grouped_owner_tests {
    use super::*;

    fn builder(shadow: bool) -> Builder {
        let fields = if shadow {
            vec![SchemaNode::scalar("header", ScalarType::String)]
        } else {
            vec![SchemaNode::scalar("value", ScalarType::String)]
        };
        let source_schema = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("header", ScalarType::String),
                        SchemaNode::group(
                            "Children",
                            vec![SchemaNode::group("Items", fields).repeating()],
                        ),
                    ],
                )
                .repeating(),
            ],
        );
        Builder {
            graph: Graph::default(),
            next: 0,
            functions: BTreeMap::new(),
            udf_outputs: BTreeMap::new(),
            grouped: Some(GroupedSource {
                collection: vec!["Rows".into()],
                key_output: 1,
                key_node: 2,
                key_path: vec!["header".into()],
                filename_feed: 3,
            }),
            source_schema,
            in_group_member: true,
            grouping_preparation: None,
            feeds: BTreeMap::new(),
            ports: BTreeMap::new(),
            port_kinds: BTreeMap::new(),
            integer_ports: BTreeSet::new(),
            filename_port: None,
            string_targets: BTreeSet::new(),
            cache: BTreeMap::new(),
            active: BTreeSet::new(),
            parameter_key: None,
            parameter_dependent_feeds: BTreeSet::new(),
            filename_phase: false,
        }
    }

    #[test]
    fn inherited_reads_cannot_escape_or_rewind_the_group() {
        let path = |parts: &[&str]| parts.iter().map(|part| (*part).into()).collect::<Vec<_>>();
        let owner = path(&["Rows", "Children", "Items", "value"]);
        let builder = builder(false);
        assert_eq!(
            builder
                .relative_source(&path(&["Rows", "header"]), &owner)
                .unwrap(),
            ["header"]
        );
        assert!(builder.relative_source(&path(&["Rows"]), &owner).is_err());
        assert!(
            builder
                .relative_source(&path(&["Other", "header"]), &owner)
                .is_err()
        );
        assert!(
            self::builder(true)
                .relative_source(&path(&["Rows", "header"]), &owner)
                .is_err()
        );
    }
}
