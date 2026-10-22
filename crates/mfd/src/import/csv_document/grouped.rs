//! A complete, opt-in two-level flat CSV document construction.
//! Missing sequence presence is carried separately from scalar text.

use super::super::{function, graph};
use crate::MfdError;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, DocumentProject, Graph, Node, NodeId, PrimarySourcePlan, Project,
    RuntimeValue, Scope, ScopeConstruction, ScopeIteration,
};
use std::collections::{BTreeMap, BTreeSet};

fn refuse(reason: impl AsRef<str>) -> MfdError {
    MfdError::UnsupportedImport(format!("grouped CSV document profile: {}", reason.as_ref()))
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
    parameter_type: Option<ScalarType>,
    preview: Option<String>,
}

struct Endpoint {
    name: String,
    filename: u32,
    rows: u32,
    fields: Vec<(String, u32)>,
}

#[derive(Clone, Copy)]
struct Expr {
    value: NodeId,
    present: NodeId,
    ty: ScalarType,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Row,
    Reduced,
    Filename,
}

struct Builder {
    graph: Graph,
    next: NodeId,
    functions: Vec<Function>,
    owners: BTreeMap<u32, usize>,
    feeds: BTreeMap<u32, u32>,
    fields: BTreeMap<u32, String>,
    rows: u32,
    outer: usize,
    outer_key: Option<Expr>,
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
        || !project.extra_targets.is_empty()
        || !project.failure_rules.is_empty()
        || !project.user_functions.is_empty()
    {
        return Err(refuse(
            "requires one CSV source, one CSV target and no independent failure rules or functions",
        ));
    }
    let doc = roxmltree::Document::parse(text)?;
    let mapping = doc.root_element();
    if mapping.attribute("version") != Some("26") {
        return Err(refuse("requires the qualified mapping version 26"));
    }
    let wrappers = mapping
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect::<Vec<_>>();
    let [wrapper] = wrappers.as_slice() else {
        return Err(refuse("requires one mapping wrapper"));
    };
    let structures = wrapper
        .children()
        .filter(|node| node.has_tag_name("structure"))
        .collect::<Vec<_>>();
    let [structure] = structures.as_slice() else {
        return Err(refuse("requires one owning structure"));
    };
    let edges = graph::read_edge_inputs(structure, Some(wrapper));
    if !edges.complete || !edges.conflicts.is_empty() {
        return Err(refuse("ambiguous or incomplete input feeds"));
    }
    let children = structure
        .children()
        .filter(|node| node.has_tag_name("children"))
        .collect::<Vec<_>>();
    let [children] = children.as_slice() else {
        return Err(refuse("requires one component list"));
    };
    let mut source = None;
    let mut target = None;
    let mut functions = Vec::new();
    let mut owners = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut uids = BTreeSet::new();
    for component in children.children().filter(roxmltree::Node::is_element) {
        if !component.has_tag_name("component") {
            return Err(refuse("unknown component-list element"));
        }
        let uid = number(component.attribute("uid"))?;
        if !uids.insert(uid) {
            return Err(refuse("duplicate component identity"));
        }
        if component.attribute("library") == Some("text") {
            let name = component.attribute("name").unwrap_or_default();
            let is_source = name == project.source.name;
            let is_target = name == project.target.name;
            if is_source == is_target {
                return Err(refuse("CSV endpoint ownership is ambiguous"));
            }
            let schema = if is_source {
                &project.source
            } else {
                &project.target
            };
            let endpoint = endpoint(component, schema, is_source)?;
            role(&mut roles, endpoint.filename, false)?;
            role(&mut roles, endpoint.rows, is_source)?;
            for (_, port) in &endpoint.fields {
                role(&mut roles, *port, is_source)?;
            }
            if is_source {
                if source.replace(endpoint).is_some() {
                    return Err(refuse("duplicate primary CSV source"));
                }
            } else if target.replace(endpoint).is_some() {
                return Err(refuse("duplicate primary CSV target"));
            }
        } else {
            validate_function_data(component)?;
            let parsed = function::read(&component);
            if !matches!(
                (parsed.library.as_str(), parsed.kind, parsed.name.as_str()),
                ("core", 2, "constant")
                    | ("core", 4, "if-else")
                    | ("core", 6, _)
                    | (
                        "core",
                        5,
                        "group-by"
                            | "concat"
                            | "sum"
                            | "auto-number"
                            | "substring-before"
                            | "format-dateTime"
                    )
                    | ("lang", 5, "now" | "empty" | "right-trim")
                    | ("db", 5, "substitute-null")
            ) {
                return Err(refuse("unqualified component operator"));
            }
            let inputs = pins(component, "sources")?;
            let outputs = pins(component, "targets")?;
            if inputs != parsed.inputs || outputs != parsed.output_pins || outputs.is_empty() {
                return Err(refuse(
                    "raw function pins differ from the normalized declaration",
                ));
            }
            for key in inputs.iter().flatten() {
                role(&mut roles, *key, false)?;
            }
            let index = functions.len();
            if index >= 256 {
                return Err(refuse("function inventory exceeds the bounded profile"));
            }
            for key in outputs.iter().flatten() {
                role(&mut roles, *key, true)?;
                if owners.insert(*key, index).is_some() {
                    return Err(refuse("duplicate output ownership"));
                }
            }
            if parsed
                .input_parameter_name
                .as_ref()
                .is_some_and(|parameter| parameter.optional)
            {
                return Err(refuse(
                    "optional input parameters require a separate profile",
                ));
            }
            functions.push(Function {
                library: parsed.library,
                name: parsed.name,
                kind: parsed.kind,
                inputs,
                outputs,
                literal: parsed.constant,
                parameter: parsed.input_parameter_name.map(|parameter| parameter.name),
                parameter_type: parsed.input_type,
                preview: parsed.input_preview,
            });
        }
    }
    for (input, output) in &edges.feeds {
        if roles.get(input) != Some(&false) || roles.get(output) != Some(&true) {
            return Err(refuse("a feed has an unknown or wrong-direction port"));
        }
    }
    let source = source.ok_or_else(|| refuse("missing primary CSV source"))?;
    let target = target.ok_or_else(|| refuse("missing primary CSV target"))?;
    let approved_warnings = [
        format!(
            "CSV component `{}` has connected FileInstance input port `{}`; filename-driven documents are unsupported",
            source.name, source.filename
        ),
        format!(
            "CSV component `{}` has connected FileInstance input port `{}`; filename-driven documents are unsupported",
            target.name, target.filename
        ),
    ];
    if warnings
        .iter()
        .any(|warning| !approved_warnings.contains(warning))
    {
        return Err(refuse(
            "the underlying import has unrelated unresolved diagnostics",
        ));
    }
    let mut builder = Builder {
        next: project
            .graph
            .nodes
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| refuse("node identities exhausted"))?,
        graph: project.graph.clone(),
        functions,
        owners,
        feeds: edges.feeds,
        fields: source
            .fields
            .iter()
            .map(|(name, port)| (*port, name.clone()))
            .collect(),
        rows: source.rows,
        outer: usize::MAX,
        outer_key: None,
        cache: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    let source_feed = builder.feed(source.filename)?;
    let source_function = builder.function(source_feed)?.1.clone();
    if source_function.library != "core"
        || source_function.kind != 6
        || source_function.parameter.is_none()
        || source_function.parameter_type != Some(ScalarType::String)
        || source_function.inputs.as_slice() != [None]
        || source_function.outputs.as_slice() != [Some(source_feed)]
        || source_function
            .inputs
            .iter()
            .any(|input| input.is_some_and(|input| builder.feeds.contains_key(&input)))
    {
        return Err(refuse(
            "source acquisition requires one unconnected global String parameter",
        ));
    }
    let retained = project.graph.nodes.get(&filename);
    if !matches!(retained, Some(Node::RuntimeParameter { name, ty: ScalarType::String, .. }) if Some(name) == source_function.parameter.as_ref())
    {
        return Err(refuse(
            "retained filename does not belong to the source parameter",
        ));
    }
    let output_feed = builder.feed(target.filename)?;
    let mut groups = BTreeSet::new();
    builder.filename_groups(output_feed, &mut BTreeSet::new(), &mut groups)?;
    if groups.len() != 1 {
        return Err(refuse("filename requires exactly one outer grouping key"));
    }
    builder.outer = *groups.first().unwrap();
    let outer_key_feed = builder.group_key(builder.outer, None)?;
    let outer_key = builder.expression(Mode::Row, outer_key_feed)?;
    text_value(outer_key)?;
    builder.outer_key = Some(outer_key);
    let body_feed = builder.feed(target.rows)?;
    let (inner, _) = builder.function(body_feed)?;
    if inner == builder.outer {
        return Err(refuse("document and row groupings must be distinct"));
    }
    let key_feed = builder.group_key(inner, Some(body_feed))?;
    let row_key = builder.expression(Mode::Row, key_feed)?;
    text_value(row_key)?;
    let mut bindings = Vec::new();
    for (field, port) in &target.fields {
        let feed = builder.feed(*port)?;
        if builder.contains_counter(feed, &mut BTreeSet::new())?
            && builder.function(feed)?.1.name != "auto-number"
        {
            return Err(refuse(
                "counter expressions must feed their fixed column directly",
            ));
        }
        let expression = builder.expression(Mode::Reduced, feed)?;
        text_value(expression)?;
        bindings.push(Binding {
            target_field: field.clone(),
            node: expression.value,
        });
    }
    let output = builder.expression(Mode::Filename, output_feed)?;
    text_value(output)?;
    project.graph = builder.graph;
    project.target_path = None;
    project.root = Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec![],
            output_path: output.value,
        },
        construction: ScopeConstruction::DocumentRows,
        filter: Some(outer_key.present),
        group_by: Some(outer_key.value),
        children: vec![Scope {
            iteration: ScopeIteration::Source(vec![]),
            filter: Some(row_key.present),
            group_by: Some(row_key.value),
            bindings,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut document = DocumentProject::new(project, PrimarySourcePlan { filename });
    document.prune_unreachable_nodes();
    let diagnostics = engine::validate(document.project());
    if !diagnostics.is_empty() {
        return Err(refuse(format!(
            "lowered mapping is invalid: {diagnostics:?}"
        )));
    }
    Ok(document)
}

fn number(value: Option<&str>) -> Result<u32, MfdError> {
    value
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| refuse("missing or invalid raw identity"))
}

fn role(roles: &mut BTreeMap<u32, bool>, key: u32, output: bool) -> Result<(), MfdError> {
    if roles.insert(key, output).is_some() {
        return Err(refuse("a raw port has multiple owners"));
    }
    Ok(())
}

fn pins(component: roxmltree::Node<'_, '_>, side: &str) -> Result<Vec<Option<u32>>, MfdError> {
    let lists = component
        .children()
        .filter(|node| node.has_tag_name(side))
        .collect::<Vec<_>>();
    if lists.len() > 1 {
        return Err(refuse("duplicate function pin lists"));
    }
    let Some(list) = lists.first() else {
        return Ok(vec![]);
    };
    if list.attributes().len() != 0 {
        return Err(refuse("unexpected function pin-list attributes"));
    }
    let mut result = Vec::new();
    for (index, pin) in list
        .children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
    {
        if index >= 64
            || !pin.has_tag_name("datapoint")
            || pin.children().any(|node| node.is_element())
            || pin.attributes().any(|attribute| {
                attribute.namespace().is_some() || !matches!(attribute.name(), "key" | "pos")
            })
        {
            return Err(refuse("unqualified raw function pin"));
        }
        if pin
            .attribute("pos")
            .is_some_and(|pos| pos.parse::<usize>().ok() != Some(index))
        {
            return Err(refuse("noncanonical function pin order"));
        }
        let key = pin
            .attribute("key")
            .map(|key| number(Some(key)))
            .transpose()?;
        if key.is_some() && pin.attribute("pos").is_none() {
            return Err(refuse("keyed function pin lacks its explicit position"));
        }
        result.push(key);
    }
    Ok(result)
}

fn validate_function_data(component: roxmltree::Node<'_, '_>) -> Result<(), MfdError> {
    if component.attributes().any(|attribute| {
        attribute.namespace().is_some()
            || !matches!(
                attribute.name(),
                "name" | "library" | "uid" | "kind" | "growable" | "growablebasename"
            )
    }) {
        return Err(refuse("unqualified function component metadata"));
    }
    for properties in component
        .children()
        .filter(|node| node.has_tag_name("properties"))
    {
        if properties.attributes().len() != 0 || properties.children().any(|node| node.is_element())
        {
            return Err(refuse("unqualified function properties"));
        }
    }
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    if data.len() > 1 {
        return Err(refuse("duplicate function data"));
    }
    let elements = data
        .first()
        .map(|data| {
            data.children()
                .filter(roxmltree::Node::is_element)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match (
        component.attribute("library"),
        component.attribute("kind"),
        component.attribute("name"),
    ) {
        (Some("core"), Some("2"), Some("constant")) => {
            let literals = elements
                .iter()
                .filter(|node| node.has_tag_name("constant"))
                .collect::<Vec<_>>();
            let [literal] = literals.as_slice() else {
                return Err(refuse("ambiguous literal data"));
            };
            if literal.attribute("datatype") != Some("string")
                || literal.attribute("value").is_none()
                || literal
                    .attributes()
                    .any(|attribute| !matches!(attribute.name(), "datatype" | "value"))
                || literal.children().any(|node| node.is_element())
                || elements.iter().any(|node| {
                    !node.has_tag_name("constant")
                        && !(node.has_tag_name("wsdl")
                            && node.attributes().len() == 0
                            && !node.children().any(|child| child.is_element()))
                })
            {
                return Err(refuse("unqualified literal data"));
            }
        }
        (Some("core"), Some("6"), _) => {
            if elements.len() != 2 {
                return Err(refuse("unqualified input parameter data"));
            }
            let input = elements
                .iter()
                .find(|node| node.has_tag_name("input"))
                .ok_or_else(|| refuse("missing parameter input declaration"))?;
            let parameter = elements
                .iter()
                .find(|node| node.has_tag_name("parameter"))
                .ok_or_else(|| refuse("missing parameter declaration"))?;
            if input.attribute("datatype") != Some("string")
                || input.attributes().any(|attribute| {
                    !matches!(
                        attribute.name(),
                        "datatype" | "previewvalue" | "usepreviewvalue"
                    )
                })
                || input
                    .attribute("usepreviewvalue")
                    .is_some_and(|value| !matches!(value, "0" | "1"))
                || parameter.attribute("usageKind") != Some("input")
                || parameter.attribute("name").is_none_or(str::is_empty)
                || parameter
                    .attributes()
                    .any(|attribute| !matches!(attribute.name(), "usageKind" | "name"))
                || elements
                    .iter()
                    .any(|node| node.children().any(|child| child.is_element()))
            {
                return Err(refuse("unqualified input parameter metadata"));
            }
        }
        _ if !elements.is_empty() => return Err(refuse("unexpected function data")),
        _ => {}
    }
    Ok(())
}

fn endpoint(
    component: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    source: bool,
) -> Result<Endpoint, MfdError> {
    if component.attribute("kind") != Some("16")
        || !attributes(component, &["name", "library", "uid", "kind"])
        || component
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|node| !matches!(node.tag_name().name(), "properties" | "view" | "data"))
    {
        return Err(refuse("unqualified CSV component metadata"));
    }
    let properties = component
        .children()
        .filter(|node| node.has_tag_name("properties"))
        .collect::<Vec<_>>();
    let [properties] = properties.as_slice() else {
        return Err(refuse("ambiguous CSV properties"));
    };
    if !attributes(*properties, &["DynamicFilenames", "XSLTDefaultOutput"])
        || properties.attribute("DynamicFilenames") != Some("1")
        || properties
            .attribute("XSLTDefaultOutput")
            .is_some_and(|value| value != "1")
        || properties.children().any(|node| node.is_element())
    {
        return Err(refuse("unqualified dynamic CSV properties"));
    }
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    else {
        return Err(refuse("CSV schema must be a flat group"));
    };
    if schema.repeating
        || children.is_empty()
        || children.len() > 256
        || !alternatives.is_empty()
        || dynamic.is_some()
        || children.iter().any(|field| {
            field.repeating
                || !matches!(
                    field.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                )
        })
    {
        return Err(refuse(
            "requires singular String columns in a closed CSV row schema",
        ));
    }
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return Err(refuse("ambiguous CSV data declaration"));
    };
    if data.attributes().len() != 0
        || data.children().filter(roxmltree::Node::is_element).count() != 2
    {
        return Err(refuse("unqualified CSV data entries"));
    }
    let texts = data
        .children()
        .filter(|node| node.has_tag_name("text"))
        .collect::<Vec<_>>();
    let [text] = texts.as_slice() else {
        return Err(refuse("ambiguous CSV text declaration"));
    };
    if text.attribute("type") != Some("csv")
        || text.attribute("encoding") != Some("1000")
        || text
            .attribute("byteordermark")
            .is_some_and(|value| value != "0")
        || !attributes(
            *text,
            &[
                "type",
                "encoding",
                "byteorder",
                "byteordermark",
                "inputinstance",
            ],
        )
    {
        return Err(refuse(
            "requires the declared UTF-8 CSV profile without an output BOM",
        ));
    }
    let root = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let [root] = root.as_slice() else {
        return Err(refuse("ambiguous CSV root"));
    };
    if !attributes(*root, &["scrollposition"])
        || root
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|node| !matches!(node.tag_name().name(), "header" | "entry"))
    {
        return Err(refuse("unqualified CSV root metadata"));
    }
    let headers = root
        .children()
        .filter(|node| node.has_tag_name("header"))
        .collect::<Vec<_>>();
    if headers.len() > 1
        || headers
            .iter()
            .any(|header| !super::protocol_namespace_header(*header))
    {
        return Err(refuse("unqualified CSV protocol namespace header"));
    }
    let wrappers = root
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let [wrapper] = wrappers.as_slice() else {
        return Err(refuse("requires a single CSV FileInstance"));
    };
    if wrapper.attribute("name") != Some("FileInstance")
        || wrapper.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "name" | "ns" | "inpkey" | "expanded")
        })
    {
        return Err(refuse("unqualified CSV FileInstance"));
    }
    let documents = wrapper
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [document] = documents.as_slice() else {
        return Err(refuse("requires one document entry"));
    };
    if !attributes(*document, &["name", "ns", "expanded"])
        || !super::super::schema::is_xml_document_wrapper(document, *root)
    {
        return Err(refuse("invalid protocol document wrapper"));
    }
    let rows = document
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return Err(refuse("requires one flat row entry"));
    };
    let side = if source { "outkey" } else { "inpkey" };
    if !row.has_tag_name("entry")
        || row.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || (attribute.name() != side
                    && !matches!(attribute.name(), "name" | "expanded" | "ns"))
        })
    {
        return Err(refuse("unqualified CSV row metadata"));
    }
    let rows_port = number(row.attribute(side))?;
    let fields = row
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|field| {
            if !field.has_tag_name("entry")
                || field.children().any(|child| child.is_element())
                || field.attributes().any(|attribute| {
                    attribute.namespace().is_some()
                        || (attribute.name() != side
                            && !matches!(attribute.name(), "name" | "expanded" | "ns"))
                })
            {
                return Err(refuse("nested CSV column declaration"));
            }
            Ok((
                field
                    .attribute("name")
                    .ok_or_else(|| refuse("unnamed CSV field"))?
                    .to_owned(),
                number(field.attribute(side))?,
            ))
        })
        .collect::<Result<Vec<_>, MfdError>>()?;
    if fields
        .iter()
        .map(|(name, _)| name.as_str())
        .ne(children.iter().map(|field| field.name.as_str()))
    {
        return Err(refuse(
            "target/source declaration order differs from its CSV schema",
        ));
    }
    let settings = text
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [settings] = settings.as_slice() else {
        return Err(refuse("ambiguous CSV settings"));
    };
    if !settings.has_tag_name("settings")
        || !attributes(
            *settings,
            &["separator", "quote", "firstrownames", "removeempty"],
        )
    {
        return Err(refuse("unqualified CSV settings"));
    }
    let names = settings
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [names] = names.as_slice() else {
        return Err(refuse("ambiguous CSV field names"));
    };
    if !names.has_tag_name("names")
        || !attributes(*names, &["root", "block"])
        || names.attribute("root") != Some(schema.name.as_str())
        || names.attribute("block") != row.attribute("name")
    {
        return Err(refuse("CSV declared field owner differs"));
    }
    let declared = names
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if declared.len() != fields.len()
        || declared
            .iter()
            .zip(&fields)
            .enumerate()
            .any(|(index, (node, (name, _)))| {
                node.tag_name().name() != format!("field{index}")
                    || !attributes(*node, &["name", "type"])
                    || node.attribute("name") != Some(name.as_str())
                    || node.attribute("type") != Some("string")
            })
    {
        return Err(refuse("CSV field metadata differs from the ordered ports"));
    }
    Ok(Endpoint {
        name: schema.name.clone(),
        filename: number(wrapper.attribute("inpkey"))?,
        rows: rows_port,
        fields,
    })
}

fn attributes(node: roxmltree::Node<'_, '_>, allowed: &[&str]) -> bool {
    node.attributes()
        .all(|attribute| attribute.namespace().is_none() && allowed.contains(&attribute.name()))
}

fn text_value(value: Expr) -> Result<(), MfdError> {
    if value.ty != ScalarType::String {
        return Err(refuse("a text expression has a non-String domain"));
    }
    Ok(())
}

impl Builder {
    fn alloc(&mut self, node: Node) -> Result<NodeId, MfdError> {
        let id = self.next;
        self.next = id
            .checked_add(1)
            .ok_or_else(|| refuse("node identities exhausted"))?;
        self.graph.nodes.insert(id, node);
        Ok(id)
    }
    fn constant(&mut self, value: Value) -> Result<NodeId, MfdError> {
        self.alloc(Node::Const { value })
    }
    fn call(&mut self, function: &str, args: Vec<NodeId>) -> Result<NodeId, MfdError> {
        self.alloc(Node::Call {
            function: function.into(),
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
            .ok_or_else(|| refuse("required input feed is absent"))
    }
    fn function(&self, output: u32) -> Result<(usize, &Function), MfdError> {
        let index = self
            .owners
            .get(&output)
            .copied()
            .ok_or_else(|| refuse("unknown function output"))?;
        Ok((index, &self.functions[index]))
    }
    fn inputs(&self, function: &Function, count: usize) -> Result<Vec<u32>, MfdError> {
        if function.inputs.len() != count {
            return Err(refuse("unqualified function arity"));
        }
        function
            .inputs
            .iter()
            .map(|input| {
                self.feed(input.ok_or_else(|| refuse("required function pin is keyless"))?)
            })
            .collect()
    }
    fn group_key(&self, index: usize, items_output: Option<u32>) -> Result<u32, MfdError> {
        let f = &self.functions[index];
        if f.library != "core"
            || f.kind != 5
            || f.name != "group-by"
            || f.outputs.len() != 2
            || items_output.is_some_and(|output| f.outputs[0] != Some(output))
        {
            return Err(refuse("unqualified grouping owner or output role"));
        }
        let inputs = self.inputs(f, 2)?;
        if inputs[0] != self.rows {
            return Err(refuse("grouping belongs to another source collection"));
        }
        Ok(inputs[1])
    }
    fn filename_groups(
        &self,
        feed: u32,
        seen: &mut BTreeSet<u32>,
        groups: &mut BTreeSet<usize>,
    ) -> Result<(), MfdError> {
        if !seen.insert(feed) {
            return Ok(());
        }
        if seen.len() > 256 {
            return Err(refuse("filename graph exceeds the bounded profile"));
        }
        let (index, function) = self.function(feed)?;
        if function.name == "group-by" {
            self.group_key(index, None)?;
            if function.outputs[1] != Some(feed) {
                return Err(refuse(
                    "filename reads group items instead of a grouping key",
                ));
            }
            groups.insert(index);
            return Ok(());
        }
        for input in &function.inputs {
            if let Some(feed) = input.and_then(|input| self.feeds.get(&input)) {
                self.filename_groups(*feed, seen, groups)?;
            }
        }
        Ok(())
    }
    fn contains_counter(&self, feed: u32, seen: &mut BTreeSet<u32>) -> Result<bool, MfdError> {
        if feed == self.rows || self.fields.contains_key(&feed) || !seen.insert(feed) {
            return Ok(false);
        }
        if seen.len() > 256 {
            return Err(refuse("field graph exceeds the bounded profile"));
        }
        let (_, function) = self.function(feed)?;
        if function.name == "auto-number" {
            return Ok(true);
        }
        for input in &function.inputs {
            if let Some(feed) = input.and_then(|input| self.feeds.get(&input))
                && self.contains_counter(*feed, seen)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn source_dependent(&self, feed: u32, seen: &mut BTreeSet<u32>) -> Result<bool, MfdError> {
        if self.fields.contains_key(&feed) || feed == self.rows {
            return Ok(true);
        }
        if !seen.insert(feed) {
            return Ok(false);
        }
        if seen.len() > 256 {
            return Err(refuse("field graph exceeds the bounded profile"));
        }
        let (_, function) = self.function(feed)?;
        if matches!(function.name.as_str(), "auto-number" | "group-by" | "sum") {
            return Ok(true);
        }
        for input in &function.inputs {
            if let Some(feed) = input.and_then(|input| self.feeds.get(&input))
                && self.source_dependent(*feed, seen)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn expression(&mut self, mode: Mode, feed: u32) -> Result<Expr, MfdError> {
        if let Some(value) = self.cache.get(&(mode, feed)) {
            return Ok(*value);
        }
        if self.active.len() >= 64 || !self.active.insert((mode, feed)) {
            return Err(refuse("cyclic or overly deep expression"));
        }
        let result = self.expression_inner(mode, feed);
        self.active.remove(&(mode, feed));
        if let Ok(value) = result {
            self.cache.insert((mode, feed), value);
        }
        result
    }
    fn expression_inner(&mut self, mode: Mode, feed: u32) -> Result<Expr, MfdError> {
        if let Some(field) = self.fields.get(&feed).cloned() {
            if mode == Mode::Filename {
                return Err(refuse(
                    "filename source fields must be represented by its grouping key",
                ));
            }
            let value = if mode == Mode::Row {
                self.alloc(Node::SourceField {
                    path: vec![field],
                    frame: Some(vec![]),
                })?
            } else {
                self.alloc(Node::Aggregate {
                    function: AggregateOp::LastPresentText,
                    collection: vec![],
                    value: vec![field],
                    expression: None,
                    arg: None,
                })?
            };
            return Ok(Expr {
                value,
                present: self.call("exists", vec![value])?,
                ty: ScalarType::String,
            });
        }
        let (index, function) = self.function(feed)?;
        let f = function.clone();
        if f.name == "group-by" {
            let key = self.group_key(index, None)?;
            if f.outputs[1] != Some(feed) || mode == Mode::Row {
                return Err(refuse("group-key context is not qualified"));
            }
            if index == self.outer {
                return self
                    .outer_key
                    .ok_or_else(|| refuse("outer group key is not initialized"));
            }
            if mode == Mode::Filename {
                return Err(refuse("filename depends on an inner grouping key"));
            }
            let key = self.expression(Mode::Row, key)?;
            text_value(key)?;
            let value = self.alloc(Node::Aggregate {
                function: AggregateOp::LastDistinctText,
                collection: vec![],
                value: vec![],
                expression: Some(key.value),
                arg: None,
            })?;
            return Ok(Expr {
                value,
                present: self.call("exists", vec![value])?,
                ty: ScalarType::String,
            });
        }
        if f.outputs.as_slice() != [Some(feed)] {
            return Err(refuse("scalar function has additional output roles"));
        }
        if f.library == "core" && f.kind == 2 && f.name == "constant" && f.inputs.is_empty() {
            let (text, ty) = f.literal.ok_or_else(|| refuse("constant has no literal"))?;
            if ty != "string" {
                return Err(refuse("only String literals are qualified"));
            }
            return Ok(Expr {
                value: self.constant(Value::String(text))?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.library == "core"
            && f.kind == 6
            && let Some(name) = f.parameter.as_ref()
        {
            if f.parameter_type != Some(ScalarType::String) || f.inputs.as_slice() != [None] {
                return Err(refuse(
                    "only keyless global String parameters are qualified",
                ));
            }
            return Ok(Expr {
                value: self.alloc(Node::RuntimeParameter {
                    name: name.clone(),
                    ty: ScalarType::String,
                    preview: f.preview,
                })?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.library == "lang"
            && f.kind == 5
            && f.name == "now"
            && f.inputs.is_empty()
            && mode == Mode::Filename
        {
            return Ok(Expr {
                value: self.alloc(Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime,
                })?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.library == "core" && f.kind == 5 && f.name == "auto-number" && mode == Mode::Reduced {
            if f.inputs.as_slice() != [None, None, None, None]
                || self.feeds.values().filter(|value| **value == feed).count() != 1
            {
                return Err(refuse(
                    "only one directly consumed default counter is qualified",
                ));
            }
            let one = self.constant(Value::Int(1))?;
            let restart = self.constant(Value::String(String::new()))?;
            let counter = self.alloc(Node::InvocationCounter {
                key: format!("mfd-auto-number:{feed}"),
                start: one,
                step: one,
                restart: Some(restart),
            })?;
            return Ok(Expr {
                value: self.call("string", vec![counter])?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.library == "core" && f.kind == 5 && f.name == "sum" && mode == Mode::Reduced {
            if f.inputs.len() != 2 || f.inputs[0].is_some() {
                return Err(refuse("only context-local one-input sums are qualified"));
            }
            let raw = self.feed(f.inputs[1].ok_or_else(|| refuse("missing sum input"))?)?;
            if !self.fields.contains_key(&raw) {
                return Err(refuse("sum requires a direct nullable String column"));
            }
            let item = self.expression(Mode::Row, raw)?;
            let decimal = self.call("decimal_from_nullable_text", vec![item.value])?;
            let sum = self.alloc(Node::Aggregate {
                function: AggregateOp::DecimalSum,
                collection: vec![],
                value: vec![],
                expression: Some(decimal),
                arg: None,
            })?;
            return Ok(Expr {
                value: self.call("string", vec![sum])?,
                present: self.constant(Value::Bool(true))?,
                ty: ScalarType::String,
            });
        }
        if f.library == "core" && f.kind == 4 && f.name == "if-else" && mode == Mode::Row {
            let inputs = self.inputs(&f, 3)?;
            let condition = self.expression(mode, inputs[0])?;
            let yes = self.expression(mode, inputs[1])?;
            let no = self.expression(mode, inputs[2])?;
            if condition.ty != ScalarType::Bool || yes.ty != no.ty {
                return Err(refuse("conditional domains differ"));
            }
            let nil = self.constant(Value::Null)?;
            let false_ = self.constant(Value::Bool(false))?;
            let selected = self.choose(condition.value, yes.value, no.value)?;
            let present = self.choose(condition.value, yes.present, no.present)?;
            return Ok(Expr {
                value: self.choose(condition.present, selected, nil)?,
                present: self.choose(condition.present, present, false_)?,
                ty: yes.ty,
            });
        }
        if f.library == "db" && f.kind == 5 && f.name == "substitute-null" && mode != Mode::Filename
        {
            let inputs = self.inputs(&f, 2)?;
            let first = self.expression(mode, inputs[0])?;
            let fallback = self.expression(mode, inputs[1])?;
            if first.ty != fallback.ty {
                return Err(refuse("substitution domains differ"));
            }
            let true_ = self.constant(Value::Bool(true))?;
            return Ok(Expr {
                value: self.choose(first.present, first.value, fallback.value)?,
                present: self.choose(first.present, true_, fallback.present)?,
                ty: first.ty,
            });
        }
        let (name, count, ty) = match (f.library.as_str(), f.kind, f.name.as_str(), mode) {
            ("core", 5, "concat", _) if (2..=16).contains(&f.inputs.len()) => {
                ("concat", f.inputs.len(), ScalarType::String)
            }
            ("core", 5, "substring-before", Mode::Row) => {
                ("substring_before", 2, ScalarType::String)
            }
            ("lang", 5, "right-trim", Mode::Row) => ("right_trim", 1, ScalarType::String),
            ("lang", 5, "empty", Mode::Row) => ("is_empty", 1, ScalarType::Bool),
            ("core", 5, "format-dateTime", Mode::Filename)
                if f.inputs.len() == 3 && f.inputs[2].is_none() =>
            {
                ("format_datetime", 2, ScalarType::String)
            }
            _ => {
                return Err(refuse(format!(
                    "unqualified {} function {} in this context",
                    f.library, f.name
                )));
            }
        };
        let mut arity = f.clone();
        arity.inputs.truncate(count);
        let inputs = self.inputs(&arity, count)?;
        if mode == Mode::Reduced && name == "concat" {
            let mut dependent = 0;
            for input in &inputs {
                dependent += usize::from(self.source_dependent(*input, &mut BTreeSet::new())?);
            }
            if dependent > 1 {
                return Err(refuse(
                    "fixed-column concatenation has multiple independent sequences",
                ));
            }
        }
        let args = inputs
            .into_iter()
            .map(|input| self.expression(mode, input))
            .collect::<Result<Vec<_>, _>>()?;
        for arg in &args {
            text_value(*arg)?;
        }
        let mut present = self.constant(Value::Bool(true))?;
        for arg in &args {
            present = self.call("and", vec![present, arg.present])?;
        }
        let value = self.call(name, args.iter().map(|arg| arg.value).collect())?;
        let nil = self.constant(Value::Null)?;
        Ok(Expr {
            value: self.choose(present, value, nil)?,
            present,
            ty,
        })
    }
}
