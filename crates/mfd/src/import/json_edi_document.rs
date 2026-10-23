//! Explicit ordered JSON-to-EDI body lowering. Source acquisition, filename
//! evaluation and physical EDI publication remain separate host contracts.

use super::{ImportOptions, ImportedJsonSourceProjection, function, graph, schema};
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
    feeds: BTreeMap<u32, u32>,
    ports: BTreeMap<u32, Vec<String>>,
    port_kinds: BTreeMap<u32, String>,
    filename_port: u32,
    string_targets: BTreeSet<u32>,
    cache: BTreeMap<(u32, Option<u32>, Vec<String>), NodeId>,
    active: BTreeSet<(u32, Option<u32>, Vec<String>)>,
}

pub fn import_json_edi_document_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedJsonEdiDocument, MfdError> {
    let source = super::import_json_source_projection_with_options(path, options)?;
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
        let mut port_kinds = BTreeMap::new();
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
            }
            if matches!(component.attribute("library"), Some("json" | "text")) {
                continue;
            }
            let f = std::rc::Rc::new(function::read(&component));
            if f.outputs.is_empty() {
                continue;
            }
            let inputs = pins(component, "sources")?;
            for key in &f.outputs {
                if source.ports.contains_key(key)
                    || *key == source.filename_output_port
                    || functions
                        .insert(
                            *key,
                            Function {
                                parsed: f.clone(),
                                inputs: inputs.clone(),
                                literal_valid: literal_data(component),
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
        let mut builder = Builder {
            graph: Graph::default(),
            next: 0,
            functions,
            feeds: edges.feeds,
            ports: source.ports.clone(),
            port_kinds,
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
        };
        let input_node = builder.alloc(Node::RuntimeParameter {
            name: source.filename_parameter.clone(),
            ty: ScalarType::String,
            preview: source.filename_preview.clone(),
        })?;
        let output_feed = builder.feed(filename_input)?;
        if !builder
            .dependencies(output_feed, &mut BTreeSet::new())?
            .is_empty()
        {
            return Err(refuse("output filename is not source-free"));
        }
        let output_node = builder.compile(output_feed, None, &[])?;
        let filename = empty_project(
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
        );
        let body = builder.scope(
            root,
            &target_schema.schema,
            &[],
            &[],
            None,
            &target_schema.options,
        )?;
        let mut project = empty_project(
            source
                .shape
                .document_set_schema()
                .map_err(|_| refuse("invalid source shape"))?,
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
        )
        .with_json_source_projection(source.shape.clone())
        .map_err(|_| refuse("invalid source projection binding"))?;
        for (label, project) in [("body", document.project()), ("filename", &filename)] {
            let issues = engine::validate(project);
            if !issues.is_empty() {
                return Err(refuse(format!("{label} validation: {issues:?}")));
            }
        }
        Ok(ImportedJsonEdiDocument {
            document,
            output_filename: filename,
            source,
            writer_minimums,
        })
    })();
    resources.finish_bindings()?;
    result
}

fn empty_project(source: SchemaNode, target: SchemaNode, graph: Graph, root: Scope) -> Project {
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
fn literal_data(component: roxmltree::Node<'_, '_>) -> bool {
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
    matches!(literals.as_slice(),[value] if value.attribute("value").is_some() && matches!(value.attribute("datatype"),Some("string"|"decimal")) && value.attributes().all(|a|matches!(a.name(),"value"|"datatype")) && !value.children().any(|n|n.is_element()))
        && data.attributes().len() == 0
        && data.children().filter(|n| n.is_element()).all(|n| {
            n.has_tag_name("constant")
                || (n.has_tag_name("wsdl")
                    && n.attributes().len() == 0
                    && !n.children().any(|n| n.is_element()))
        })
}

fn function_metadata(component: roxmltree::Node<'_, '_>) -> bool {
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
fn pins(component: roxmltree::Node<'_, '_>, kind: &str) -> Result<Vec<Option<u32>>, MfdError> {
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
        if self.ports.contains_key(&feed) || feed == self.filename_port {
            return Ok(BTreeSet::from([feed]));
        }
        if !seen.insert(feed) {
            return Err(refuse("cyclic dependency"));
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
        let key = (feed, leaf, anchor.to_vec());
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
                let number = self.call("decimal_from_nullable_text", vec![value])?;
                self.call("string", vec![number])
            } else {
                Ok(value)
            };
        }
        let f = self
            .functions
            .get(&feed)
            .ok_or_else(|| refuse("unsupported feed owner"))?
            .clone();
        if !f.parsed.raw_single_output_valid || !f.metadata_valid {
            return Err(refuse("unqualified function output"));
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
                return Err(refuse("ambiguous literal"));
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
            let collection = relative(&collection, anchor)?;
            return self.alloc(Node::Aggregate {
                function: AggregateOp::Count,
                collection,
                value: vec![],
                expression: None,
                arg: None,
            });
        }
        if f.parsed.name == "if-else" && f.parsed.kind == 4 {
            if f.inputs.len() != 3 || f.inputs[2].is_some() {
                return Err(refuse("requires one condition with absent else"));
            }
            let condition = self.compile(self.input(&f, 0)?, leaf, anchor)?;
            let then_node = self.compile(self.input(&f, 1)?, leaf, anchor)?;
            let else_node = self.constant(Value::Null)?;
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
            ("core", "format-dateTime") => ("format_datetime_milliseconds", 2),
            _ => {
                return Err(refuse(format!(
                    "unsupported scalar operator {}:{}",
                    f.parsed.library, f.parsed.name
                )));
            }
        };
        let valid_arity = if f.parsed.name == "format-dateTime" {
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
        self.call(name, args)
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
        let dependencies = self.dependencies(feed, &mut BTreeSet::new())?;
        if dependencies.len() > 1 {
            return Err(refuse("scalar combines distinct source occurrences"));
        }
        let dependency = dependencies.first().copied();
        let mut node = if let Some(leaf) = dependency {
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
                self.alloc(Node::Aggregate {
                    function: AggregateOp::FirstNonemptyText,
                    collection: relative(&self.ports[&leaf], anchor)?,
                    value: vec![],
                    expression: Some(value),
                    arg: None,
                })?
            }
        } else {
            self.compile(feed, None, anchor)?
        };
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
        let mut leaf = active_leaf;
        if let Some(key) = entry.attribute("inpkey") {
            let feed = self.feed(number(Some(key))?)?;
            if feed == self.filename_port {
                current = vec!["Documents".into()];
                if current != anchor {
                    scope.iteration = ScopeIteration::Source(relative(&current, anchor)?);
                }
            } else if let Some(source) = self.ports.get(&feed) {
                if self.port_kinds[&feed] != "object" {
                    return Err(refuse("group needs source object driver"));
                }
                current = source.clone();
                if current == anchor {
                    scope.iteration = ScopeIteration::None;
                } else if schema.repeating {
                    scope.iteration = ScopeIteration::Source(relative(&current, anchor)?);
                } else {
                    return Err(refuse("non-repeating group crosses a source owner"));
                }
            } else {
                let dependencies = self.dependencies(feed, &mut BTreeSet::new())?;
                if let Some(&key) = dependencies.first() {
                    if dependencies.len() != 1 {
                        return Err(refuse("mixed group drivers"));
                    }
                    let f = self
                        .functions
                        .get(&feed)
                        .ok_or_else(|| refuse("unknown group driver"))?
                        .clone();
                    if f.parsed.name != "if-else" || f.inputs.len() != 3 || f.inputs[2].is_some() {
                        return Err(refuse("group conditional requires absent else"));
                    }
                    current = self.ports[&key].clone();
                    leaf = Some(key);
                    scope.iteration = ScopeIteration::Source(relative(&current, anchor)?);
                    scope.filter = Some(self.compile(self.input(&f, 0)?, leaf, &current)?);
                } else {
                    self.compile(feed, None, anchor)?;
                    scope.iteration = ScopeIteration::None;
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
