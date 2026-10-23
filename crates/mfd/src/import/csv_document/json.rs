//! Explicit flat CSV file-set to JSON documents. File metadata and row data
//! are separate source collections; empty physical files must not disappear.

use super::super::{function, graph};
use crate::MfdError;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, DocumentProject, DynamicBinding, Graph, Node, NodeId, PrimarySourcePlan,
    Project, RuntimeValue, Scope, ScopeIteration, SequenceExpr,
};
use std::collections::{BTreeMap, BTreeSet};

fn refuse(reason: impl AsRef<str>) -> MfdError {
    MfdError::UnsupportedImport(format!("CSV/JSON document profile: {}", reason.as_ref()))
}
#[derive(Clone)]
struct Function {
    library: String,
    name: String,
    kind: u32,
    inputs: Vec<Option<u32>>,
    outputs: Vec<Option<u32>>,
    literal: Option<(String, String)>,
    parameter: Option<String>,
    preview: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Header,
    HeaderRow,
    Row,
    File,
    FileSet,
    Filename,
}
#[derive(Clone, Copy)]
struct Expr {
    value: NodeId,
    present: NodeId,
    ty: ScalarType,
}
#[derive(Clone)]
struct Port {
    path: Vec<String>,
    input: u32,
    kind: String,
}
struct Target {
    name: String,
    filename: u32,
    ports: Vec<Port>,
}
struct Builder {
    graph: Graph,
    next: NodeId,
    functions: Vec<Function>,
    owners: BTreeMap<u32, usize>,
    feeds: BTreeMap<u32, u32>,
    fields: BTreeMap<u32, String>,
    rows_port: u32,
    filename_port: u32,
    rows: Vec<String>,
    files: Vec<String>,
    cache: BTreeMap<(Mode, u32), Expr>,
    active: BTreeSet<(Mode, u32)>,
}

pub(super) fn lower(
    text: &str,
    mut project: Project,
    filename: NodeId,
    warnings: &[String],
) -> Result<DocumentProject, MfdError> {
    if !project.extra_sources.is_empty()
        || !project.failure_rules.is_empty()
        || !project.user_functions.is_empty()
        || project.extra_targets.len() != 1
    {
        return Err(refuse(
            "requires one CSV source, two JSON targets and no independent functions or failure rules",
        ));
    }
    let doc = roxmltree::Document::parse(text)?;
    if doc.root_element().attribute("version") != Some("30") {
        return Err(refuse("requires mapping version 30"));
    }
    let wrappers: Vec<_> = doc
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect();
    let [wrapper] = wrappers.as_slice() else {
        return Err(refuse("ambiguous mapping wrapper"));
    };
    let structures: Vec<_> = wrapper
        .children()
        .filter(|node| node.has_tag_name("structure"))
        .collect();
    let [structure] = structures.as_slice() else {
        return Err(refuse("ambiguous owning structure"));
    };
    let edges = graph::read_edge_inputs(structure, Some(wrapper));
    if !edges.complete || !edges.conflicts.is_empty() {
        return Err(refuse("ambiguous or incomplete feeds"));
    }
    let children: Vec<_> = structure
        .children()
        .filter(|node| node.has_tag_name("children"))
        .collect();
    let [children] = children.as_slice() else {
        return Err(refuse("ambiguous component list"));
    };
    let mut source = None;
    let mut source_name = String::new();
    let mut targets = Vec::new();
    let mut functions = Vec::new();
    let mut owners = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut role = |key, output| -> Result<(), MfdError> {
        if roles.insert(key, output).is_some() {
            return Err(refuse("duplicate raw port identity"));
        }
        Ok(())
    };
    for component in children.children().filter(roxmltree::Node::is_element) {
        if !component.has_tag_name("component") || !ids.insert(number(component.attribute("uid"))?)
        {
            return Err(refuse("unknown or duplicate component"));
        }
        match component.attribute("library") {
            Some("text") => {
                if source.is_some() {
                    return Err(refuse("ambiguous CSV source"));
                }
                source_name = component
                    .attribute("name")
                    .ok_or_else(|| refuse("unnamed CSV component"))?
                    .into();
                let input = super::grouped::file_set_source(component, &project.source)?;
                role(input.filename, false)?;
                role(input.rows, true)?;
                role(
                    input
                        .filename_output
                        .ok_or_else(|| refuse("missing physical filename output"))?,
                    true,
                )?;
                for (_, key) in &input.fields {
                    role(*key, true)?;
                }
                source = Some(input);
            }
            Some("json") => {
                let target = target(component)?;
                role(target.filename, false)?;
                for port in &target.ports {
                    role(port.input, false)?;
                }
                targets.push(target);
            }
            _ => {
                let parsed = function::read(&component);
                if !matches!(
                    (parsed.library.as_str(), parsed.kind, parsed.name.as_str()),
                    ("core", 2, "constant")
                        | ("core", 6, _)
                        | ("core", 4, "if-else")
                        | (
                            "core",
                            5,
                            "group-by"
                                | "concat"
                                | "sum"
                                | "auto-number"
                                | "equal"
                                | "string-length"
                                | "format-date"
                                | "format-dateTime"
                                | "parse-dateTime"
                                | "item-at"
                                | "tokenize-regexp"
                        )
                        | ("lang", 5, "now" | "create-guid" | "format-guid-string")
                        | ("db", 5, "is-null")
                ) {
                    return Err(refuse("unqualified component operator"));
                }
                let inputs = super::grouped::pins(component, "sources")?;
                let outputs = super::grouped::pins(component, "targets")?;
                if inputs != parsed.inputs || outputs != parsed.output_pins {
                    return Err(refuse("normalized pins differ from raw declarations"));
                }
                let keyed = |slots: &[Option<u32>]| slots.iter().all(Option::is_some);
                let signature = match (parsed.kind, parsed.name.as_str()) {
                    (2, _) => inputs.is_empty() && outputs.len() == 1,
                    (6, _) => {
                        inputs == [None]
                            && outputs.len() == 1
                            && outputs[0].is_some()
                            && parsed.input_type == Some(ScalarType::String)
                    }
                    (_, "group-by") => inputs.len() == 2 && keyed(&inputs) && outputs.len() == 2,
                    (_, "sum") => {
                        inputs.len() == 2
                            && inputs[0].is_none()
                            && inputs[1].is_some()
                            && outputs.len() == 1
                    }
                    (_, "auto-number") => {
                        inputs.len() == 4
                            && inputs[..3] == [None, None, None]
                            && inputs[3].is_some()
                            && outputs.len() == 1
                    }
                    (_, "format-date" | "format-dateTime" | "tokenize-regexp") => {
                        inputs.len() == 3
                            && keyed(&inputs[..2])
                            && inputs[2].is_none()
                            && outputs.len() == 1
                    }
                    (_, "if-else") => {
                        (3..=9).contains(&inputs.len())
                            && inputs.len() % 2 == 1
                            && keyed(&inputs[..inputs.len() - 1])
                            && outputs.len() == 1
                    }
                    (_, "concat") => {
                        (2..=16).contains(&inputs.len()) && keyed(&inputs) && outputs.len() == 1
                    }
                    (_, "equal" | "parse-dateTime" | "item-at") => {
                        inputs.len() == 2 && keyed(&inputs) && outputs.len() == 1
                    }
                    (_, "string-length" | "is-null" | "format-guid-string") => {
                        inputs.len() == 1 && keyed(&inputs) && outputs.len() == 1
                    }
                    (_, "now" | "create-guid") => inputs.is_empty() && outputs.len() == 1,
                    _ => false,
                };
                if !signature {
                    return Err(refuse(format!(
                        "unqualified raw function signature {}: {inputs:?} -> {outputs:?}",
                        parsed.name
                    )));
                }
                if component.attributes().any(|attr| {
                    attr.namespace().is_some()
                        || !matches!(
                            attr.name(),
                            "name" | "library" | "uid" | "kind" | "growable" | "growablebasename"
                        )
                }) {
                    return Err(refuse("unqualified function metadata"));
                }
                if component
                    .children()
                    .filter(roxmltree::Node::is_element)
                    .any(|node| {
                        !matches!(
                            node.tag_name().name(),
                            "properties" | "sources" | "targets" | "view" | "data"
                        )
                    })
                {
                    return Err(refuse("unknown function metadata element"));
                }
                if component
                    .children()
                    .filter(|node| node.has_tag_name("properties"))
                    .any(|node| {
                        node.attributes().len() != 0 || node.children().any(|n| n.is_element())
                    })
                {
                    return Err(refuse("unqualified function properties"));
                }
                if parsed.kind == 6 {
                    super::grouped::validate_function_data(component)?;
                }
                if parsed.kind != 2
                    && parsed.kind != 6
                    && component
                        .children()
                        .filter(|n| n.has_tag_name("data"))
                        .any(|data| {
                            data.attributes().len() != 0 || data.children().any(|n| n.is_element())
                        })
                {
                    return Err(refuse("unexpected function data"));
                }
                if parsed.kind == 2 {
                    let data: Vec<_> = component
                        .children()
                        .filter(|node| node.has_tag_name("data"))
                        .collect();
                    let [data] = data.as_slice() else {
                        return Err(refuse("ambiguous literal data"));
                    };
                    let literal: Vec<_> = data
                        .children()
                        .filter(|node| node.has_tag_name("constant"))
                        .collect();
                    if !matches!(literal.as_slice(),[value] if value.attribute("value").is_some() && matches!(value.attribute("datatype"),Some("string"|"decimal")) && value.attributes().all(|a|matches!(a.name(),"value"|"datatype")) && !value.children().any(|n|n.is_element()))
                        || data.children().filter(|n| n.is_element()).any(|node| {
                            !node.has_tag_name("constant")
                                && !(node.has_tag_name("wsdl")
                                    && node.attributes().len() == 0
                                    && !node.children().any(|n| n.is_element()))
                        })
                    {
                        return Err(refuse("ambiguous literal data"));
                    }
                }
                if parsed
                    .input_parameter_name
                    .as_ref()
                    .is_some_and(|parameter| parameter.optional)
                {
                    return Err(refuse(
                        "optional source parameters require separate admission",
                    ));
                }
                for key in inputs.iter().flatten() {
                    role(*key, false)?;
                }
                for key in outputs.iter().flatten() {
                    role(*key, true)?;
                    owners.insert(*key, functions.len());
                }
                functions.push(Function {
                    library: parsed.library,
                    name: parsed.name,
                    kind: parsed.kind,
                    inputs,
                    outputs,
                    literal: parsed.constant,
                    parameter: parsed.input_parameter_name.map(|value| value.name),
                    preview: parsed.input_preview,
                });
            }
        }
    }
    for (input, output) in &edges.feeds {
        if roles.get(input) != Some(&false) || roles.get(output) != Some(&true) {
            return Err(refuse("unknown or wrong-direction feed"));
        }
    }
    let source = source.ok_or_else(|| refuse("missing source"))?;
    if targets.len() != 2
        || targets
            .iter()
            .map(|t| &t.name)
            .collect::<BTreeSet<_>>()
            .len()
            != 2
    {
        return Err(refuse("ambiguous JSON targets"));
    }
    if targets[1].name != project.extra_targets[0].name || project.source.name == "Path" {
        return Err(refuse("endpoint order or row-name collision"));
    }
    let filename_port = source.filename_output.unwrap();
    let mut allowed = vec![format!("CSV component `{}` has connected FileInstance input port `{}`; filename-driven documents are unsupported", source_name, source.filename),
        format!("CSV component `{}` has connected FileInstance output port `{filename_port}`; filename-driven documents are unsupported",source_name),
        "aggregate `item-at` has an unresolvable sequence input; imported as its empty-sequence result until the sequence is restored".into(),
        "auto-number restart-on-change is not isolated by a distinct related scope; imported as Null".into()];
    for (index, target) in targets.iter().enumerate() {
        allowed.push(format!("JSON component `{}` has connected FileInstance input port `{}`; filename-driven documents are unsupported",target.name,target.filename));
        let schema = if index == 0 {
            &project.target
        } else {
            &project.extra_targets[0].schema
        };
        for port in &target.ports {
            if at(schema, &port.path).is_none() {
                let parent = at(schema, &port.path[..port.path.len().saturating_sub(1)])
                    .ok_or_else(|| refuse("unknown target parent"))?;
                if parent.dynamic_fields().is_none() || port.kind != "number" {
                    return Err(refuse("unqualified undeclared JSON member"));
                }
                allowed.push(format!(
                    "target port path `{}` not found in schema",
                    port.path.join("/")
                ));
            }
        }
    }
    if warnings.iter().any(|warning| !allowed.contains(warning)) {
        return Err(refuse("unrelated unresolved import diagnostics"));
    }
    let mut b = Builder {
        next: project
            .graph
            .nodes
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| refuse("node ids exhausted"))?,
        graph: project.graph.clone(),
        functions,
        owners,
        feeds: edges.feeds,
        fields: source
            .fields
            .into_iter()
            .map(|(name, key)| (key, name))
            .collect(),
        rows_port: source.rows,
        filename_port,
        rows: vec!["Files".into(), project.source.name.clone()],
        files: vec!["Files".into()],
        cache: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    let source_feed = b.feed(source.filename)?;
    let source_fn = b.function(source_feed)?.clone();
    if source_fn.kind != 6
        || source_fn.library != "core"
        || source_fn.inputs.as_slice() != [None]
        || !matches!(b.graph.nodes.get(&filename),Some(Node::RuntimeParameter{name,ty:ScalarType::String,..}) if Some(name)==source_fn.parameter.as_ref())
    {
        return Err(refuse(
            "source must use its retained global String filename parameter",
        ));
    }
    let mut roots = BTreeMap::new();
    for (index, target) in targets.iter().enumerate() {
        let schema = if index == 0 {
            &project.target
        } else {
            &project.extra_targets[0].schema
        };
        let driver = b.function(b.feed(target.filename)?)?.clone();
        if driver.name != "if-else" || driver.inputs.len() != 3 || driver.inputs[2].is_some() {
            return Err(refuse(
                "target filename requires a guarded path with absent else",
            ));
        }
        let condition = b.compile(Mode::Header, b.input(&driver, 0)?)?;
        if condition.ty != ScalarType::Bool {
            return Err(refuse("target guard must be Boolean"));
        }
        let path_feed = b.input(&driver, 1)?;
        let mut keys = BTreeSet::new();
        b.filename_keys(path_feed, &mut BTreeSet::new(), &mut keys)?;
        if keys.len() != 1 {
            return Err(refuse("filename requires one source grouping key"));
        }
        let key = *keys.first().unwrap();
        let key_name = b
            .fields
            .get(&key)
            .ok_or_else(|| refuse("filename key must be one source column"))?
            .clone();
        let candidate = b.compile(Mode::Filename, path_feed)?;
        if candidate.ty != ScalarType::String {
            return Err(refuse("filename candidate must be String"));
        }
        let path = b.alloc(Node::DistinctTextMapLast {
            collection: b.rows.clone(),
            key: vec![key_name],
            expression: candidate.value,
        })?;
        let first = b.compile(Mode::Header, key)?;
        let active = b.call("and", vec![condition.value, first.present])?;
        let mut root = b.scope(schema, &[], target, false)?;
        root.iteration = ScopeIteration::DynamicDocuments {
            source: b.rows.clone(),
            output_path: path,
        };
        root.group_by = Some(b.constant(Value::Bool(true))?);
        root.post_group_filter = Some(active);
        roots.insert(target.name.clone(), root);
    }
    project.root = roots
        .remove(&targets[0].name)
        .ok_or_else(|| refuse("missing primary target"))?;
    for target in &mut project.extra_targets {
        target.root = roots
            .remove(&target.name)
            .ok_or_else(|| refuse("missing named target"))?;
    }
    project.source = mapping::csv_file_set_source_schema(&project.source);
    project.source_options = Default::default();
    project.graph = b.graph;
    let mut document = DocumentProject::new(project, PrimarySourcePlan { filename });
    document.prune_unreachable_nodes();
    let issues = engine::validate(document.project());
    if !issues.is_empty() {
        return Err(refuse(format!("lowered project is invalid: {issues:?}")));
    }
    Ok(document)
}

fn number(value: Option<&str>) -> Result<u32, MfdError> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| refuse("invalid raw identity"))
}
fn at<'a>(mut node: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    for name in path {
        node = node.child(name)?;
    }
    Some(node)
}
fn target(component: roxmltree::Node<'_, '_>) -> Result<Target, MfdError> {
    if component.attribute("kind") != Some("31") {
        return Err(refuse("unsupported JSON endpoint kind"));
    }
    if component.attributes().any(|a| {
        a.namespace().is_some() || !matches!(a.name(), "name" | "library" | "uid" | "kind")
    }) || component
        .children()
        .filter(roxmltree::Node::is_element)
        .any(|n| !matches!(n.tag_name().name(), "properties" | "view" | "data"))
    {
        return Err(refuse("unknown JSON component metadata"));
    }
    let properties: Vec<_> = component
        .children()
        .filter(|n| n.has_tag_name("properties"))
        .collect();
    if !matches!(properties.as_slice(),[p] if p.attribute("DynamicFilenames")==Some("1") &&
        p.attribute("XSLTTargetEncoding").is_none_or(|v|v=="UTF-8") && p.attribute("XSLTDefaultOutput").is_none_or(|v|matches!(v,"0"|"1")) &&
        p.attributes().all(|a|a.namespace().is_none()&&matches!(a.name(),"DynamicFilenames"|"XSLTTargetEncoding"|"XSLTDefaultOutput")) && !p.children().any(|n|n.is_element()))
    {
        return Err(refuse("unqualified JSON document properties"));
    }
    let data: Vec<_> = component
        .children()
        .filter(|n| n.has_tag_name("data"))
        .collect();
    let [data] = data.as_slice() else {
        return Err(refuse("ambiguous JSON data"));
    };
    if data.attributes().len() != 0
        || data.children().filter(roxmltree::Node::is_element).count() != 2
    {
        return Err(refuse("unknown JSON data settings"));
    }
    let roots: Vec<_> = data.children().filter(|n| n.has_tag_name("root")).collect();
    let [root] = roots.as_slice() else {
        return Err(refuse("ambiguous JSON root"));
    };
    if root
        .attributes()
        .any(|a| a.namespace().is_some() || a.name() != "scrollposition")
        || root
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|n| !matches!(n.tag_name().name(), "entry" | "header"))
    {
        return Err(refuse("unknown JSON root metadata"));
    }
    let headers: Vec<_> = root
        .children()
        .filter(|n| n.has_tag_name("header"))
        .collect();
    if headers.len() > 1
        || headers
            .iter()
            .any(|h| !super::protocol_namespace_header(*h))
    {
        return Err(refuse("invalid JSON protocol namespace"));
    }
    let settings: Vec<_> = data.children().filter(|n| n.has_tag_name("json")).collect();
    if !matches!(settings.as_slice(),[s] if s.attribute("json5")==Some("1") && s.attribute("schema").is_some() &&
        s.attributes().all(|a|a.namespace().is_none()&&matches!(a.name(),"json5"|"schema")) && !s.children().any(|n|n.is_element()))
    {
        return Err(refuse("requires explicit JSON5 output"));
    }
    let wrappers: Vec<_> = root
        .children()
        .filter(|n| n.has_tag_name("entry"))
        .collect();
    let [wrapper] = wrappers.as_slice() else {
        return Err(refuse("ambiguous JSON document wrapper"));
    };
    if wrapper.attribute("name") != Some("FileInstance") {
        return Err(refuse("missing JSON FileInstance"));
    }
    if wrapper.attributes().any(|a| {
        a.namespace().is_some() || !matches!(a.name(), "name" | "ns" | "inpkey" | "expanded")
    }) {
        return Err(refuse("unknown JSON FileInstance metadata"));
    }
    let mut ports = vec![];
    fn walk(
        node: roxmltree::Node<'_, '_>,
        path: &mut Vec<String>,
        ports: &mut Vec<Port>,
    ) -> Result<(), MfdError> {
        if node.attributes().any(|a| {
            a.namespace().is_some()
                || !matches!(
                    a.name(),
                    "name" | "type" | "ns" | "expanded" | "inpkey" | "casttotargettypemode"
                )
        }) || node
            .attribute("type")
            .is_some_and(|v| !matches!(v, "json-property" | "json-item"))
            || node
                .attribute("casttotargettypemode")
                .is_some_and(|v| v != "cast-in-subtree")
            || node
                .children()
                .filter(roxmltree::Node::is_element)
                .any(|n| !n.has_tag_name("entry"))
        {
            return Err(refuse("unqualified JSON entry metadata"));
        }
        let property = node.attribute("type") == Some("json-property");
        if property {
            path.push(
                node.attribute("name")
                    .ok_or_else(|| refuse("unnamed JSON property"))?
                    .into(),
            );
        }
        if node.attribute("outkey").is_some() {
            return Err(refuse("output port in a JSON target"));
        }
        if let Some(key) = node.attribute("inpkey") {
            if path.is_empty() {
                return Err(refuse("unsupported connected JSON root"));
            }
            let kind = node.attribute("name").unwrap_or_default();
            if !matches!(kind, "string" | "number" | "boolean" | "object") {
                return Err(refuse("unsupported connected JSON entry"));
            }
            ports.push(Port {
                path: path.clone(),
                input: number(Some(key))?,
                kind: kind.into(),
            });
        }
        for child in node.children().filter(|n| n.has_tag_name("entry")) {
            walk(child, path, ports)?;
        }
        if property {
            path.pop();
        }
        Ok(())
    }
    for child in wrapper.children().filter(|n| n.has_tag_name("entry")) {
        walk(child, &mut vec![], &mut ports)?;
    }
    let mut paths = BTreeSet::new();
    if ports.iter().any(|port| !paths.insert(port.path.clone())) {
        return Err(refuse("multiple connected ports for a target path"));
    }
    Ok(Target {
        name: component.attribute("name").unwrap_or_default().into(),
        filename: number(wrapper.attribute("inpkey"))?,
        ports,
    })
}

impl Builder {
    fn alloc(&mut self, node: Node) -> Result<NodeId, MfdError> {
        let id = self.next;
        self.next = id
            .checked_add(1)
            .ok_or_else(|| refuse("node ids exhausted"))?;
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
    fn choose(
        &mut self,
        condition: NodeId,
        then: NodeId,
        else_: NodeId,
    ) -> Result<NodeId, MfdError> {
        self.alloc(Node::If {
            condition,
            then,
            else_,
        })
    }
    fn feed(&self, input: u32) -> Result<u32, MfdError> {
        self.feeds
            .get(&input)
            .copied()
            .ok_or_else(|| refuse("required feed is absent"))
    }
    fn function(&self, output: u32) -> Result<&Function, MfdError> {
        self.owners
            .get(&output)
            .map(|i| &self.functions[*i])
            .ok_or_else(|| refuse("unknown scalar output owner"))
    }
    fn input(&self, f: &Function, index: usize) -> Result<u32, MfdError> {
        self.feed(
            f.inputs
                .get(index)
                .and_then(|v| *v)
                .ok_or_else(|| refuse("required function pin is absent"))?,
        )
    }
    fn group_key(&self, output: u32) -> Result<u32, MfdError> {
        let f = self.function(output)?;
        if f.name != "group-by"
            || f.outputs.len() != 2
            || f.outputs[1] != Some(output)
            || self.input(f, 0)? != self.rows_port
        {
            return Err(refuse("unqualified grouping output"));
        }
        let key = self.input(f, 1)?;
        if !self.fields.contains_key(&key) {
            return Err(refuse("group key must be a direct String column"));
        }
        Ok(key)
    }
    fn header_columns(
        &self,
        feed: u32,
        seen: &mut BTreeSet<u32>,
        columns: &mut BTreeSet<u32>,
    ) -> Result<(), MfdError> {
        if self.fields.contains_key(&feed) {
            columns.insert(feed);
            return Ok(());
        }
        if feed == self.filename_port {
            return Ok(());
        }
        if !seen.insert(feed) {
            return Ok(());
        }
        let f = self.function(feed)?;
        if f.name == "group-by" {
            columns.insert(self.group_key(feed)?);
            return Ok(());
        }
        if matches!(f.name.as_str(), "is-null" | "sum") {
            return Ok(());
        }
        if matches!(f.name.as_str(), "auto-number" | "now" | "create-guid") {
            return Err(refuse(
                "source-dependent header expressions cannot contain runtime effects",
            ));
        }
        for input in f.inputs.iter().flatten() {
            if let Some(feed) = self.feeds.get(input) {
                self.header_columns(*feed, seen, columns)?;
            }
        }
        Ok(())
    }
    fn deps(&self, feed: u32, seen: &mut BTreeSet<u32>) -> Result<(bool, bool), MfdError> {
        if self.fields.contains_key(&feed) || feed == self.rows_port {
            return Ok((true, false));
        }
        if feed == self.filename_port {
            return Ok((false, true));
        }
        if !seen.insert(feed) {
            return Ok((false, false));
        }
        if seen.len() > 256 {
            return Err(refuse("expression inventory exceeds profile"));
        }
        let f = self.function(feed)?;
        if matches!(f.name.as_str(), "is-null" | "sum") {
            return Ok((false, false));
        }
        let mut d = (false, false);
        for key in f.inputs.iter().flatten() {
            if let Some(feed) = self.feeds.get(key) {
                let next = self.deps(*feed, seen)?;
                d.0 |= next.0;
                d.1 |= next.1;
            }
        }
        Ok(d)
    }
    fn first(&mut self, mode: Mode, feed: u32, collection: Vec<String>) -> Result<Expr, MfdError> {
        let item = self.compile(mode, feed)?;
        let value = self.alloc(Node::CollectionFind {
            collection,
            predicate: item.present,
            value: item.value,
        })?;
        Ok(Expr {
            value,
            present: self.call("exists", vec![value])?,
            ty: item.ty,
        })
    }
    fn compile(&mut self, mode: Mode, feed: u32) -> Result<Expr, MfdError> {
        if let Some(expr) = self.cache.get(&(mode, feed)) {
            return Ok(*expr);
        }
        if self.active.len() > 64 || !self.active.insert((mode, feed)) {
            return Err(refuse("cyclic or deep scalar expression"));
        }
        let result = self.compile_inner(mode, feed);
        self.active.remove(&(mode, feed));
        if let Ok(expr) = result {
            self.cache.insert((mode, feed), expr);
        }
        result
    }
    fn compile_inner(&mut self, mode: Mode, feed: u32) -> Result<Expr, MfdError> {
        if matches!(mode, Mode::Header | Mode::HeaderRow) {
            let (rows, files) = self.deps(feed, &mut BTreeSet::new())?;
            if mode == Mode::Header && rows {
                let mut columns = BTreeSet::new();
                self.header_columns(feed, &mut BTreeSet::new(), &mut columns)?;
                if columns.len() != 1 {
                    return Err(refuse(
                        "header expression combines independent source sequences",
                    ));
                }
                return self.first(Mode::HeaderRow, feed, self.rows.clone());
            }
            if files && !rows {
                return self.compile(Mode::FileSet, feed);
            }
        }
        if let Some(field) = self.fields.get(&feed).cloned() {
            if mode == Mode::File {
                return Err(refuse("file expression reads a row column"));
            }
            let value = self.alloc(Node::SourceField {
                path: vec![field],
                frame: Some(self.rows.clone()),
            })?;
            return Ok(Expr {
                value,
                present: self.call("exists", vec![value])?,
                ty: ScalarType::String,
            });
        }
        if feed == self.filename_port {
            if mode == Mode::FileSet {
                return self.first(Mode::File, feed, self.files.clone());
            }
            let value = if mode == Mode::File {
                self.alloc(Node::SourceField {
                    path: vec!["Path".into()],
                    frame: Some(self.files.clone()),
                })?
            } else {
                self.alloc(Node::SourceRowDocumentPath {
                    collection: self.rows.clone(),
                })?
            };
            return Ok(Expr {
                value,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        let f = self.function(feed)?.clone();
        if f.name == "group-by" {
            let key = self.group_key(feed)?;
            return self.compile(mode, key);
        }
        if f.outputs.as_slice() != [Some(feed)] {
            return Err(refuse("scalar output has additional roles"));
        }
        if f.kind == 2 {
            let (text, ty) = f.literal.ok_or_else(|| refuse("literal missing"))?;
            let (value, ty) = match ty.as_str() {
                "string" => (Value::String(text), ScalarType::String),
                "decimal" => (
                    Value::Int(text.parse().map_err(|_| {
                        refuse("only exact integral decimal literals are admitted")
                    })?),
                    ScalarType::Int,
                ),
                _ => return Err(refuse("unqualified literal type")),
            };
            return Ok(Expr {
                value: self.constant(value)?,
                present: self.constant(Value::Bool(true))?,
                ty,
            });
        }
        if f.kind == 6 {
            return Ok(Expr {
                value: self.alloc(Node::RuntimeParameter {
                    name: f
                        .parameter
                        .ok_or_else(|| refuse("parameter name missing"))?,
                    ty: ScalarType::String,
                    preview: f.preview,
                })?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.name == "now" {
            return Ok(Expr {
                value: self.alloc(Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime,
                })?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.name == "is-null" {
            let input = self.input(&f, 0)?;
            self.group_key(input)?;
            let selected = self.compile(Mode::Header, input)?;
            return Ok(Expr {
                value: self.call("not", vec![selected.present])?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::Bool,
            });
        }
        if f.name == "auto-number" {
            if !matches!(mode, Mode::Row)
                || f.inputs.len() != 4
                || f.inputs[..3] != [None, None, None]
            {
                return Err(refuse(
                    "counter requires row ownership and default first three operands",
                ));
            }
            let restart = self.compile(Mode::Row, self.input(&f, 3)?)?;
            let one = self.constant(Value::Int(1))?;
            return Ok(Expr {
                value: self.alloc(Node::InvocationCounter {
                    key: format!("mfd-auto-number:{feed}"),
                    start: one,
                    step: one,
                    restart: Some(restart.value),
                })?,
                present: restart.present,
                ty: ScalarType::Int,
            });
        }
        if f.name == "sum" {
            if f.inputs.len() != 2 || f.inputs[0].is_some() {
                return Err(refuse("sum needs its implicit context input"));
            }
            let item = self.compile(Mode::Row, self.input(&f, 1)?)?;
            let decimal = self.call("decimal_from_nullable_text", vec![item.value])?;
            return Ok(Expr {
                value: self.alloc(Node::Aggregate {
                    function: AggregateOp::DecimalSum,
                    collection: self.rows.clone(),
                    value: vec![],
                    expression: Some(decimal),
                    arg: None,
                })?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::Float,
            });
        }
        if f.name == "item-at" {
            if f.inputs.len() != 2 {
                return Err(refuse("invalid item-at inputs"));
            }
            let token = self.function(self.input(&f, 0)?)?.clone();
            if token.name != "tokenize-regexp"
                || token.inputs.len() != 3
                || token.inputs[2].is_some()
            {
                return Err(refuse(
                    "only literal-regex scalar token sequences are admitted",
                ));
            }
            let raw_text = self.input(&token, 0)?;
            let regex = self.compile(mode, self.input(&token, 1)?)?;
            let text = if mode == Mode::FileSet && raw_text == self.filename_port {
                // Tokenization of a sequence flattens the tokens from each file.
                // For a literal delimiter, tokenize(join(files, delimiter)) is
                // exactly that flattened sequence, including empty tokens.
                let raw_pattern = self.function(self.input(&token, 1)?)?;
                let Some((pattern, ty)) = &raw_pattern.literal else {
                    return Err(refuse("file-set tokenization requires a literal delimiter"));
                };
                if ty != "string"
                    || pattern.is_empty()
                    || pattern.chars().any(|c| "\\.^$|?*+()[]{}".contains(c))
                {
                    return Err(refuse(
                        "file-set tokenizer delimiter must be regex-literal text",
                    ));
                }
                let value = self.compile(Mode::File, self.filename_port)?;
                Expr {
                    value: self.alloc(Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: self.files.clone(),
                        value: vec![],
                        expression: Some(value.value),
                        arg: Some(regex.value),
                    })?,
                    present: self.constant(Value::Bool(true))?,
                    ty: ScalarType::String,
                }
            } else {
                self.compile(mode, raw_text)?
            };
            let index = self.compile(mode, self.input(&f, 1)?)?;
            if text.ty != ScalarType::String || regex.ty != ScalarType::String {
                return Err(refuse("tokenizer inputs must be strings"));
            }
            let item = self.alloc(Node::SourceField {
                path: vec![],
                frame: None,
            })?;
            let value = self.alloc(Node::SequenceItemAt {
                sequence: SequenceExpr::TokenizeRegex {
                    input: text.value,
                    pattern: regex.value,
                    flags: None,
                    item,
                },
                index: index.value,
            })?;
            return Ok(Expr {
                value,
                present: self.call("exists", vec![value])?,
                ty: ScalarType::String,
            });
        }
        if f.name == "if-else" {
            if f.inputs.len() < 3 || f.inputs.len() % 2 != 1 {
                return Err(refuse("invalid conditional branch arity"));
            }
            let last = f.inputs.len() - 1;
            let nil = self.constant(Value::Null)?;
            let no = self.constant(Value::Bool(false))?;
            let mut result = if f.inputs[last].is_some() {
                self.compile(mode, self.input(&f, last)?)?
            } else {
                Expr {
                    value: nil,
                    present: no,
                    ty: ScalarType::String,
                }
            };
            for index in (0..last).step_by(2).rev() {
                let condition = self.compile(mode, self.input(&f, index)?)?;
                let yes = self.compile(mode, self.input(&f, index + 1)?)?;
                if condition.ty != ScalarType::Bool || yes.ty != result.ty {
                    return Err(refuse("conditional domains differ"));
                }
                let value = self.choose(condition.value, yes.value, result.value)?;
                let present = self.choose(condition.value, yes.present, result.present)?;
                result = Expr {
                    value: self.choose(condition.present, value, nil)?,
                    present: self.choose(condition.present, present, no)?,
                    ty: yes.ty,
                };
            }
            return Ok(result);
        }
        let (name, count, ty) = match f.name.as_str() {
            "concat" => ("concat", f.inputs.len(), ScalarType::String),
            "equal" => ("equal", 2, ScalarType::Bool),
            "string-length" => ("length", 1, ScalarType::Int),
            "parse-dateTime" => ("parse_datetime", 2, ScalarType::String),
            "format-dateTime" => ("format_datetime", 2, ScalarType::String),
            "format-date" => ("format_date", 2, ScalarType::String),
            "create-guid" => ("create_guid", 0, ScalarType::String),
            "format-guid-string" => ("format_guid_string", 1, ScalarType::String),
            _ => return Err(refuse("unsupported scalar route")),
        };
        if f.inputs.len() < count || f.inputs[count..].iter().any(Option::is_some) {
            return Err(refuse("unsupported optional function operands"));
        }
        let mut args = vec![];
        let mut present = self.constant(Value::Bool(true))?;
        for index in 0..count {
            let input = self.input(&f, index)?;
            let arg = self.compile(mode, input)?;
            let value = if f.name == "format-date"
                && index == 0
                && self
                    .function(input)
                    .is_ok_and(|f| f.name == "parse-dateTime")
            {
                self.call("date_from_datetime", vec![arg.value])?
            } else {
                arg.value
            };
            args.push(value);
            present = self.call("and", vec![present, arg.present])?;
        }
        Ok(Expr {
            value: self.call(name, args)?,
            present,
            ty,
        })
    }
    fn filename_keys(
        &self,
        feed: u32,
        seen: &mut BTreeSet<u32>,
        keys: &mut BTreeSet<u32>,
    ) -> Result<(), MfdError> {
        if !seen.insert(feed) {
            return Ok(());
        }
        if seen.len() > 256 {
            return Err(refuse("filename graph too large"));
        }
        let f = self.function(feed)?;
        if f.name == "is-null" {
            return Ok(());
        }
        if f.name == "group-by" {
            keys.insert(self.group_key(feed)?);
            return Ok(());
        }
        for input in f.inputs.iter().flatten() {
            if let Some(feed) = self.feeds.get(input) {
                self.filename_keys(*feed, seen, keys)?;
            }
        }
        Ok(())
    }
    fn scope(
        &mut self,
        schema: &SchemaNode,
        path: &[String],
        target: &Target,
        in_rows: bool,
    ) -> Result<Scope, MfdError> {
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return Err(refuse("scope must be an object"));
        };
        let mut scope = Scope::default();
        let mut rows = in_rows;
        if let Some(port) = target.ports.iter().find(|port| port.path == path) {
            if port.kind != "object" || !schema.repeating {
                return Err(refuse("connected object needs a repeated row target"));
            }
            let feed = self.feed(port.input)?;
            if !self.fields.contains_key(&feed) {
                return Err(refuse("row driver must be a source column"));
            }
            let key = self.compile(Mode::Row, feed)?;
            scope.iteration = ScopeIteration::Source(self.rows.clone());
            scope.filter = Some(key.present);
            rows = true;
        }
        let mut ordered = Vec::new();
        for port in &target.ports {
            if port.path.starts_with(path) && port.path.len() > path.len() {
                let name = port.path[path.len()].clone();
                if !ordered.contains(&name) {
                    ordered.push(name);
                }
            }
        }
        let mut dynamic_seen = false;
        for name in ordered {
            let mut childpath = path.to_vec();
            childpath.push(name.clone());
            let declared = children.iter().find(|child| child.name == name);
            if declared.is_some_and(|child| matches!(child.kind, SchemaKind::Group { .. })) {
                let mut child = self.scope(declared.unwrap(), &childpath, target, rows)?;
                child.target_field = name;
                scope.children.push(child);
                continue;
            }
            let port = target
                .ports
                .iter()
                .find(|port| port.path == childpath)
                .ok_or_else(|| refuse("scalar target port missing"))?;
            let expr = self.compile(
                if rows { Mode::Row } else { Mode::Header },
                self.feed(port.input)?,
            )?;
            let mut value = expr.value;
            if let Some(declared) = declared {
                if dynamic_seen {
                    return Err(refuse(
                        "static fields after a dynamic field require separate ordering admission",
                    ));
                }
                let SchemaKind::Scalar { ty } = &declared.kind else {
                    return Err(refuse("unsupported target scalar kind"));
                };
                if *ty == ScalarType::Float && expr.ty == ScalarType::String {
                    value = self.call("decimal_from_nullable_text", vec![value])?;
                } else if *ty != expr.ty {
                    return Err(refuse(
                        "target scalar conversion is outside the explicit profile",
                    ));
                }
                let nil = self.constant(Value::Null)?;
                let value = self.choose(expr.present, value, nil)?;
                scope.bindings.push(Binding {
                    target_field: name,
                    node: value,
                });
            } else {
                if schema.dynamic_fields().is_none_or(|field| !field.json_any)
                    || port.kind != "number"
                {
                    return Err(refuse("undeclared property is not an open numeric member"));
                }
                dynamic_seen = true;
                let value = self.call("string", vec![value])?;
                let nil = self.constant(Value::Null)?;
                let value = self.choose(expr.present, value, nil)?;
                let key = self.constant(Value::String(name))?;
                scope.dynamic_bindings.push(DynamicBinding { key, value });
            }
        }
        Ok(scope)
    }
}
