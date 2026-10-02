//! Recover two closed temperature designs whose native numeric node functions
//! are expanded into explicit conversion calls by the importer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::ops::Range;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    AggregateOp, FunctionId, Graph, Node, NodeId, Project, Scope, ScopeIteration, UserFunction,
};

use crate::MfdError;

use super::TargetExport;
use super::schema::{KeyAlloc, RenderedSchemaComponent, SideFormat};
use super::source::SourceExports;
use super::udf;

mod group_key;

const NODE_FUNCTION: &str = "35F2ECD9-F69F-498C-BCDA-969EE7F2797C";

pub(super) fn udf_numeric_aliases(
    project: &Project,
    id: FunctionId,
    function: &UserFunction,
) -> Option<BTreeMap<NodeId, NodeId>> {
    if project.user_functions.len() != 1 || project.user_functions.get(&id)?.name != function.name {
        return None;
    }
    if annual_boundary(project) {
        annual_body_aliases(function)
    } else if group_plan(project).is_some() {
        group_body_aliases(function)
    } else {
        None
    }
}

pub(super) struct NativeGroupFahrenheit {
    absorbed: BTreeSet<NodeId>,
    products: [(NodeId, NodeId); 3],
    function: FunctionId,
    group_key: NodeId,
}

impl NativeGroupFahrenheit {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
    ) -> Option<Self> {
        let [target] = targets else { return None };
        if sources.len() != 1
            || sources.iter().next()?.format != SideFormat::Xml
            || target.format != SideFormat::Xml
            || !target.mapped_scope_plans.is_empty()
            || target.dynamic_json.is_some()
        {
            return None;
        }
        group_plan(project)
    }

    pub(super) fn absorbed_nodes(&self) -> &BTreeSet<NodeId> {
        &self.absorbed
    }

    pub(super) fn seed_aliases(&self, outputs: &mut BTreeMap<NodeId, u32>) -> Result<(), MfdError> {
        for &(binding, aggregate) in &self.products {
            let key = outputs.get(&aggregate).copied().ok_or_else(|| {
                MfdError::Unsupported("native temperature aggregate output is missing".into())
            })?;
            outputs.insert(binding, key);
        }
        Ok(())
    }

    pub(super) fn apply_to_target(
        &self,
        rendered: &mut RenderedSchemaComponent,
        target: &TargetExport<'_>,
    ) -> Result<(), MfdError> {
        let row_key = target
            .ports
            .key_for_abs(&["YearlyStats".into()])
            .ok_or_else(|| {
                MfdError::Unsupported("native temperature row port is missing".into())
            })?;
        let year_key = target
            .ports
            .key_for_abs(&["YearlyStats".into(), "Year".into()])
            .ok_or_else(|| {
                MfdError::Unsupported("native temperature year port is missing".into())
            })?;
        let document = roxmltree::Document::parse(&rendered.xml).map_err(|error| {
            MfdError::Unsupported(format!(
                "native temperature target cannot be inspected: {error}"
            ))
        })?;
        let mut changes = Vec::<(Range<usize>, String)>::new();
        for (name, key, metadata) in [
            (
                "YearlyStats",
                row_key,
                format!(
                    "<inputnodefunctions><rule applyto=\"descendants\"><function name=\"{NODE_FUNCTION}\"/><filter datatype=\"decimal\"/></rule></inputnodefunctions>"
                ),
            ),
            (
                "Year",
                year_key,
                "<inputnodefunctions inherit=\"block\"/>".into(),
            ),
        ] {
            let key = key.to_string();
            let mut entries = document.descendants().filter(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("name") == Some(name)
                    && entry.attribute("inpkey") == Some(key.as_str())
            });
            let entry = entries
                .next()
                .filter(|_| entries.next().is_none())
                .ok_or_else(|| {
                    MfdError::Unsupported(format!(
                        "native temperature {name} target entry is missing"
                    ))
                })?;
            let range = entry.range();
            let opening_end = rendered.xml[range.clone()].find('>').ok_or_else(|| {
                MfdError::Unsupported(format!("native temperature {name} entry is malformed"))
            })? + range.start;
            if rendered.xml.as_bytes().get(opening_end - 1) == Some(&b'/') {
                changes.push((
                    (opening_end - 1)..(opening_end + 1),
                    format!(">{metadata}</entry>"),
                ));
            } else {
                changes.push(((opening_end + 1)..(opening_end + 1), metadata));
            }
        }
        changes.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, replacement) in changes {
            rendered.xml.replace_range(range, &replacement);
        }
        Ok(())
    }

    pub(super) fn connect_group_key(
        &self,
        target: &TargetExport<'_>,
        sources: &SourceExports<'_>,
        outputs: &BTreeMap<NodeId, u32>,
        keys: &mut KeyAlloc,
        components: &mut String,
        edges: &mut [(u32, u32)],
    ) -> Result<(), MfdError> {
        let row = target.ports.key_for_abs(&["YearlyStats".into()]);
        let year = target
            .ports
            .key_for_abs(&["YearlyStats".into(), "Year".into()]);
        let raw = outputs.get(&self.group_key).copied();
        let driver = sources.key_for_abs(&["data".into()]);
        let (Some(row), Some(year), Some(raw), Some(driver)) = (row, year, raw, driver) else {
            return Err(MfdError::Unsupported(
                "native temperature group-key ports are missing".into(),
            ));
        };
        group_key::connect_owned(row, year, raw, driver, keys, components, edges)
    }

    pub(super) fn definition(
        &self,
        functions: &udf::Exports,
        uid: &mut u32,
    ) -> Result<String, MfdError> {
        let (parameter_component, output_component) = functions
            .one_parameter_interface(self.function)
            .ok_or_else(|| {
                MfdError::Unsupported("native temperature function interface is missing".into())
            })?;
        let definition_uid = *uid + 1;
        let input_uid = *uid + 2;
        let result_uid = *uid + 3;
        let call_uid = *uid + 4;
        let round_uid = *uid + 5;
        *uid += 5;
        let mut xml = String::new();
        let _ = write!(
            xml,
            "\t<component name=\"{NODE_FUNCTION}\" library=\"mapforce_nodefunction\" uid=\"{definition_uid}\" editable=\"1\">\n\
             \t\t<properties/><structure><children>\n\
             \t\t\t<component name=\"raw_value\" library=\"core\" uid=\"{input_uid}\" kind=\"6\"><targets><datapoint pos=\"0\" key=\"1\"/></targets><data><input datatype=\"anySimpleType\"/><parameter usageKind=\"input\" name=\"raw_value\"/></data></component>\n\
             \t\t\t<component name=\"result\" library=\"core\" uid=\"{result_uid}\" kind=\"7\"><sources><datapoint pos=\"0\" key=\"2\"/></sources><data><output datatype=\"anySimpleType\"/><parameter usageKind=\"output\" name=\"result\"/></data></component>\n\
             \t\t\t<component name=\"CelsiusToFahrenheit\" library=\"user\" uid=\"{call_uid}\" kind=\"19\"><data><root><header><namespaces><namespace/></namespaces></header><entry name=\"celsius\" inpkey=\"3\" componentid=\"{parameter_component}\"/></root><root><header><namespaces><namespace/></namespaces></header><entry name=\"fahrenheit\" outkey=\"4\" componentid=\"{output_component}\"/></root></data></component>\n\
             \t\t\t<component name=\"round\" library=\"core\" uid=\"{round_uid}\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"5\"/></sources><targets><datapoint pos=\"0\" key=\"6\"/></targets></component>\n\
             \t\t</children></structure><connections><edge from=\"1\" to=\"3\"/><edge from=\"4\" to=\"5\"/><edge from=\"6\" to=\"2\"/></connections>\n\
             \t</component>\n"
        );
        Ok(xml)
    }
}

fn group_plan(project: &Project) -> Option<NativeGroupFahrenheit> {
    if !group_boundary(project)
        || project.graph.nodes.len() != 48
        || project.user_functions.len() != 1
    {
        return None;
    }
    let (&function, body) = project.user_functions.iter().next()?;
    group_body_aliases(body)?;
    let row = &project.root.children[0];
    let mut seen = BTreeSet::new();
    let graph = &project.graph;
    let [month, hyphen] = call(take(graph, row.group_by?, &mut seen)?, "substring_before")? else {
        return None;
    };
    source_field(take(graph, *month, &mut seen)?, &["month"], &["data"])?;
    constant(take(graph, *hyphen, &mut seen)?, &Value::String("-".into()))?;
    let mut products = Vec::new();
    for (field, op) in [
        ("AverageTemp", AggregateOp::Avg),
        ("MaximumTemp", AggregateOp::Max),
        ("MinimumTemp", AggregateOp::Min),
    ] {
        let binding = row
            .bindings
            .iter()
            .find(|binding| binding.target_field == field)?
            .node;
        let aggregate = group_branch(graph, binding, op, &mut seen)?;
        products.push((binding, aggregate));
    }
    if seen.len() != graph.nodes.len() || products.len() != 3 {
        return None;
    }
    let absorbed = seen
        .iter()
        .copied()
        .filter(|id| {
            !products.iter().any(|(_, aggregate)| aggregate == id)
                && *id != row.group_by.unwrap()
                && !matches!(
                    graph.nodes.get(id),
                    Some(
                        Node::SourceField { .. }
                            | Node::Const {
                                value: Value::String(_)
                            }
                    )
                )
        })
        .collect::<BTreeSet<_>>();
    // Only the three conversion branches are represented by the target rule.
    let mut absorbed = absorbed;
    absorbed.retain(|id| !matches!(graph.nodes.get(id), Some(Node::Call { function, .. }) if function == "substring_before"));
    let products: [(NodeId, NodeId); 3] = products.try_into().ok()?;
    let mut rewritten = project.clone();
    for binding in &mut rewritten.root.children[0].bindings {
        if let Some(&(_, aggregate)) = products.iter().find(|(output, _)| *output == binding.node) {
            binding.node = aggregate;
        }
    }
    rewritten.prune_unreachable_nodes();
    if absorbed
        .iter()
        .any(|id| rewritten.graph.nodes.contains_key(id))
    {
        return None;
    }
    Some(NativeGroupFahrenheit {
        absorbed,
        products,
        function,
        group_key: row.group_by?,
    })
}

fn group_branch(
    graph: &Graph,
    result: NodeId,
    op: AggregateOp,
    seen: &mut BTreeSet<NodeId>,
) -> Option<NodeId> {
    let [rounded] = call(take(graph, result, seen)?, "round")? else {
        return None;
    };
    let sum = unary(graph, *rounded, "to_number", seen)?;
    let [quotient, thirty_two] = call(take(graph, sum, seen)?, "add")? else {
        return None;
    };
    let quotient = unary(graph, *quotient, "to_number", seen)?;
    let thirty_two = unary(graph, *thirty_two, "to_number", seen)?;
    constant(take(graph, thirty_two, seen)?, &Value::Float(32.0))?;
    let [product, five] = call(take(graph, quotient, seen)?, "divide")? else {
        return None;
    };
    let product = unary(graph, *product, "to_number", seen)?;
    let five = unary(graph, *five, "to_number", seen)?;
    constant(take(graph, five, seen)?, &Value::Float(5.0))?;
    let [aggregate, nine] = call(take(graph, product, seen)?, "multiply")? else {
        return None;
    };
    let aggregate = unary(graph, *aggregate, "to_number", seen)?;
    let nine = unary(graph, *nine, "to_number", seen)?;
    constant(take(graph, nine, seen)?, &Value::Float(9.0))?;
    match take(graph, aggregate, seen)? {
        Node::Aggregate {
            function,
            collection,
            value,
            expression,
            arg,
        } if *function == op
            && collection == &["data"]
            && value == &["temp"]
            && expression.is_none()
            && arg.is_none() =>
        {
            Some(aggregate)
        }
        _ => None,
    }
}

fn annual_body_aliases(function: &UserFunction) -> Option<BTreeMap<NodeId, NodeId>> {
    if function.library != "user"
        || function.name != "Fahrenheit_to_Celsius"
        || function.parameters.len() != 1
        || function.parameters[0].name != "Fahrenheit"
        || function.parameters[0].ty != ScalarType::String
        || function.output_name != "Celsius"
        || function.output_type != ScalarType::Float
        || function.body.nodes.len() != 17
    {
        return None;
    }
    let g = &function.body;
    let mut seen = BTreeSet::new();
    let mut aliases = BTreeMap::new();
    let [product, precision] = call(take(g, function.output, &mut seen)?, "round")? else {
        return None;
    };
    let product = alias(g, *product, &mut seen, &mut aliases)?;
    let precision = alias(g, *precision, &mut seen, &mut aliases)?;
    constant(take(g, precision, &mut seen)?, &Value::Float(1.0))?;
    let [difference, fraction] = call(take(g, product, &mut seen)?, "multiply")? else {
        return None;
    };
    let difference = alias(g, *difference, &mut seen, &mut aliases)?;
    let fraction = alias(g, *fraction, &mut seen, &mut aliases)?;
    let [five, nine] = call(take(g, fraction, &mut seen)?, "divide")? else {
        return None;
    };
    let five = alias(g, *five, &mut seen, &mut aliases)?;
    let nine = alias(g, *nine, &mut seen, &mut aliases)?;
    constant(take(g, five, &mut seen)?, &Value::Float(5.0))?;
    constant(take(g, nine, &mut seen)?, &Value::Float(9.0))?;
    let [fahrenheit, thirty_two] = call(take(g, difference, &mut seen)?, "subtract")? else {
        return None;
    };
    let fahrenheit = alias(g, *fahrenheit, &mut seen, &mut aliases)?;
    let thirty_two = alias(g, *thirty_two, &mut seen, &mut aliases)?;
    parameter(take(g, fahrenheit, &mut seen)?, function)?;
    constant(take(g, thirty_two, &mut seen)?, &Value::Float(32.0))?;
    (seen.len() == g.nodes.len() && aliases.len() == 8).then_some(aliases)
}

fn group_body_aliases(function: &UserFunction) -> Option<BTreeMap<NodeId, NodeId>> {
    if function.library != "user"
        || function.name != "CelsiusToFahrenheit"
        || function.parameters.len() != 1
        || function.parameters[0].name != "celsius"
        || function.parameters[0].ty != ScalarType::String
        || function.output_name != "fahrenheit"
        || function.output_type != ScalarType::String
        || function.body.nodes.len() != 13
    {
        return None;
    }
    let g = &function.body;
    let mut seen = BTreeSet::new();
    let mut aliases = BTreeMap::new();
    let [quotient, thirty_two] = call(take(g, function.output, &mut seen)?, "add")? else {
        return None;
    };
    let quotient = alias(g, *quotient, &mut seen, &mut aliases)?;
    let thirty_two = alias(g, *thirty_two, &mut seen, &mut aliases)?;
    constant(take(g, thirty_two, &mut seen)?, &Value::Float(32.0))?;
    let [product, five] = call(take(g, quotient, &mut seen)?, "divide")? else {
        return None;
    };
    let product = alias(g, *product, &mut seen, &mut aliases)?;
    let five = alias(g, *five, &mut seen, &mut aliases)?;
    constant(take(g, five, &mut seen)?, &Value::Float(5.0))?;
    let [celsius, nine] = call(take(g, product, &mut seen)?, "multiply")? else {
        return None;
    };
    let celsius = alias(g, *celsius, &mut seen, &mut aliases)?;
    let nine = alias(g, *nine, &mut seen, &mut aliases)?;
    parameter(take(g, celsius, &mut seen)?, function)?;
    constant(take(g, nine, &mut seen)?, &Value::Float(9.0))?;
    (seen.len() == g.nodes.len() && aliases.len() == 6).then_some(aliases)
}

fn annual_boundary(project: &Project) -> bool {
    if project.source_options.pdf.is_none()
        || project.target_options.delimiter != Some(',')
        || project.target_options.has_header_row != Some(true)
        || !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
        || !project.failure_rules.is_empty()
        || project.source.name != "Document"
        || project.target.name != "Text file"
        || project.graph.nodes.len() != 4
        || !fields(
            &project.source,
            &[
                (&["Row", "Year"], ScalarType::String),
                (&["Row", "AverageTemperature"], ScalarType::String),
            ],
        )
        || !fields(
            &project.target,
            &[
                (&["Year"], ScalarType::String),
                (&["AverageTemperature_F"], ScalarType::String),
                (&["AverageTemperature_C"], ScalarType::String),
            ],
        )
        || !group_shape(&project.source, "Row", true, 2)
        || !root_leaf_count(&project.target, 3)
        || project.source != annual_source_schema()
        || project.target != annual_target_schema()
    {
        return false;
    }
    let root = &project.root;
    if root.bindings.len() != 3 {
        return false;
    }
    let expected = Scope {
        iteration: ScopeIteration::Source(vec!["Row".into()]),
        filter: root.filter,
        bindings: root.bindings.clone(),
        ..Scope::default()
    };
    if !scope_eq(root, &expected) {
        return false;
    }
    let g = &project.graph;
    let Some(filter) = root.filter else {
        return false;
    };
    let Some(Node::Call { function, args }) = g.nodes.get(&filter) else {
        return false;
    };
    if function != "is_numeric" || args.len() != 1 {
        return false;
    }
    let Some(year) = root.bindings.iter().find(|b| b.target_field == "Year") else {
        return false;
    };
    let Some(f) = root
        .bindings
        .iter()
        .find(|b| b.target_field == "AverageTemperature_F")
    else {
        return false;
    };
    let Some(c) = root
        .bindings
        .iter()
        .find(|b| b.target_field == "AverageTemperature_C")
    else {
        return false;
    };
    if args[0] != year.node
        || !matches!(g.nodes.get(&year.node), Some(Node::SourceField { path, frame: Some(frame) }) if path == &["Year"] && frame == &["Row"])
        || !matches!(g.nodes.get(&f.node), Some(Node::SourceField { path, frame: Some(frame) }) if path == &["AverageTemperature"] && frame == &["Row"])
        || !matches!(g.nodes.get(&c.node), Some(Node::UserFunctionCall { function, args }) if project.user_functions.contains_key(function) && args == &[f.node])
    {
        return false;
    }
    BTreeSet::from([filter, year.node, f.node, c.node]).len() == g.nodes.len()
}

fn group_boundary(project: &Project) -> bool {
    if !project.source_options.xml_document
        || !project.target_options.xml_document
        || !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
        || !project.failure_rules.is_empty()
        || project.source.name != "Temperatures"
        || project.target.name != "Temperatures"
        || !fields(
            &project.source,
            &[
                (&["data", "temp"], ScalarType::Float),
                (&["data", "month"], ScalarType::String),
                (&["data", "desc"], ScalarType::String),
            ],
        )
        || !fields(
            &project.target,
            &[
                (&["YearlyStats", "MinimumTemp"], ScalarType::Float),
                (&["YearlyStats", "MaximumTemp"], ScalarType::Float),
                (&["YearlyStats", "AverageTemp"], ScalarType::Float),
                (&["YearlyStats", "Year"], ScalarType::Int),
            ],
        )
        || !group_shape(&project.source, "data", true, 3)
        || !group_shape(&project.target, "YearlyStats", true, 4)
        || !["temp", "month", "desc"].iter().all(|name| {
            project
                .source
                .child("data")
                .and_then(|data| data.child(name))
                .is_some_and(|node| node.attribute)
        })
        || !["MinimumTemp", "MaximumTemp", "AverageTemp"]
            .iter()
            .all(|name| {
                project
                    .target
                    .child("YearlyStats")
                    .and_then(|row| row.child(name))
                    .is_some_and(|node| !node.attribute)
            })
        || !project
            .target
            .child("YearlyStats")
            .and_then(|row| row.child("Year"))
            .is_some_and(|node| node.attribute)
        || !schema_matches_required_use(&project.source, &group_source_schema())
        || !schema_matches_required_use(&project.target, &group_target_schema())
    {
        return false;
    }
    let [row] = project.root.children.as_slice() else {
        return false;
    };
    let expected_row = Scope {
        target_field: "YearlyStats".into(),
        iteration: ScopeIteration::Source(vec!["data".into()]),
        group_by: row.group_by,
        bindings: row.bindings.clone(),
        ..Scope::default()
    };
    let expected_root = Scope {
        children: vec![expected_row],
        ..Scope::default()
    };
    if row.group_by.is_none() || !scope_eq(&project.root, &expected_root) {
        return false;
    }
    let names = row
        .bindings
        .iter()
        .map(|b| b.target_field.as_str())
        .collect::<BTreeSet<_>>();
    names == BTreeSet::from(["Year", "MinimumTemp", "MaximumTemp", "AverageTemp"])
        && row
            .bindings
            .iter()
            .find(|b| b.target_field == "Year")
            .is_some_and(|b| Some(b.node) == row.group_by)
}

// Required-use decorates an already matched ordinary attribute. Keep every other
// schema field exact, and preserve the original schema for artifact rendering.
fn schema_matches_required_use(actual: &SchemaNode, expected: &SchemaNode) -> bool {
    fn clear_required_use(node: &mut SchemaNode) -> bool {
        if !node.xml_attribute_required_is_valid() {
            return false;
        }
        node.xml_attribute_required = false;
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            return children.iter_mut().all(clear_required_use);
        }
        true
    }
    let mut compared = actual.clone();
    clear_required_use(&mut compared) && compared == *expected
}

fn fields(schema: &SchemaNode, paths: &[(&[&str], ScalarType)]) -> bool {
    let expected_count = paths.len();
    fn leaves(node: &SchemaNode) -> usize {
        match &node.kind {
            SchemaKind::Group { children, .. } => children.iter().map(leaves).sum(),
            _ => 1,
        }
    }
    leaves(schema) == expected_count
        && paths.iter().all(|(path, ty)| {
            let Some(node) = path.iter().try_fold(schema, |node, part| node.child(part)) else {
                return false;
            };
            !node.repeating
                && matches!(node.kind, SchemaKind::Scalar { ty: actual } if actual == *ty)
        })
}

fn group_shape(schema: &SchemaNode, child: &str, repeated: bool, fields: usize) -> bool {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    let [row] = children.as_slice() else {
        return false;
    };
    if row.name != child || row.repeating != repeated {
        return false;
    }
    matches!(&row.kind, SchemaKind::Group { children, .. } if children.len() == fields)
}

fn root_leaf_count(schema: &SchemaNode, count: usize) -> bool {
    matches!(&schema.kind, SchemaKind::Group { children, .. } if children.len() == count && children.iter().all(|child| !child.repeating && !child.attribute))
}

fn annual_source_schema() -> SchemaNode {
    SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Year", ScalarType::String),
                    SchemaNode::scalar("AverageTemperature", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}

fn annual_target_schema() -> SchemaNode {
    SchemaNode::group(
        "Text file",
        vec![
            SchemaNode::scalar("Year", ScalarType::String),
            SchemaNode::scalar("AverageTemperature_F", ScalarType::String),
            SchemaNode::scalar("AverageTemperature_C", ScalarType::String),
        ],
    )
}

fn group_source_schema() -> SchemaNode {
    SchemaNode::group(
        "Temperatures",
        vec![
            SchemaNode::group(
                "data",
                vec![
                    SchemaNode::scalar("temp", ScalarType::Float).attribute(),
                    SchemaNode::scalar("month", ScalarType::String).attribute(),
                    SchemaNode::scalar("desc", ScalarType::String).attribute(),
                ],
            )
            .repeating(),
        ],
    )
}

fn group_target_schema() -> SchemaNode {
    SchemaNode::group(
        "Temperatures",
        vec![
            SchemaNode::group(
                "YearlyStats",
                vec![
                    SchemaNode::scalar("MinimumTemp", ScalarType::Float),
                    SchemaNode::scalar("MaximumTemp", ScalarType::Float),
                    SchemaNode::scalar("AverageTemp", ScalarType::Float),
                    SchemaNode::scalar("Year", ScalarType::Int).attribute(),
                ],
            )
            .repeating(),
        ],
    )
}

fn scope_eq(a: &Scope, b: &Scope) -> bool {
    matches!((serde_json::to_value(a), serde_json::to_value(b)), (Ok(a), Ok(b)) if a == b)
}

fn take<'a>(graph: &'a Graph, id: NodeId, seen: &mut BTreeSet<NodeId>) -> Option<&'a Node> {
    seen.insert(id).then(|| graph.nodes.get(&id)).flatten()
}

fn call<'a>(node: &'a Node, name: &str) -> Option<&'a [NodeId]> {
    match node {
        Node::Call { function, args } if function == name => Some(args),
        _ => None,
    }
}

fn unary(graph: &Graph, id: NodeId, name: &str, seen: &mut BTreeSet<NodeId>) -> Option<NodeId> {
    let [inner] = call(take(graph, id, seen)?, name)? else {
        return None;
    };
    Some(*inner)
}

fn alias(
    graph: &Graph,
    id: NodeId,
    seen: &mut BTreeSet<NodeId>,
    aliases: &mut BTreeMap<NodeId, NodeId>,
) -> Option<NodeId> {
    let inner = unary(graph, id, "to_number", seen)?;
    aliases.insert(id, inner);
    Some(inner)
}

fn constant(node: &Node, value: &Value) -> Option<()> {
    match node {
        Node::Const { value: actual } if actual == value => Some(()),
        _ => None,
    }
}

fn parameter(node: &Node, function: &UserFunction) -> Option<()> {
    match node {
        Node::FunctionParameter { parameter } if *parameter == function.parameters[0].id => {
            Some(())
        }
        _ => None,
    }
}

fn source_field(node: &Node, path: &[&str], frame: &[&str]) -> Option<()> {
    match node {
        Node::SourceField {
            path: p,
            frame: Some(f),
        } if p.iter().map(String::as_str).eq(path.iter().copied())
            && f.iter().map(String::as_str).eq(frame.iter().copied()) =>
        {
            Some(())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ir::{ScalarType, SchemaKind, SchemaNode, Value};
    use mapping::{
        AggregateOp, Binding, FormatOptions, FunctionId, FunctionParameter, FunctionParameterId,
        Graph, Node, NodeId, Project, Scope, ScopeIteration, UserFunction,
    };

    use super::{group_body_aliases, group_plan};

    fn insert(nodes: &mut BTreeMap<NodeId, Node>, node: Node) -> NodeId {
        let id = nodes.keys().next_back().copied().unwrap_or(0) + 1;
        nodes.insert(id, node);
        id
    }

    fn call(nodes: &mut BTreeMap<NodeId, Node>, name: &str, args: Vec<NodeId>) -> NodeId {
        insert(
            nodes,
            Node::Call {
                function: name.into(),
                args,
            },
        )
    }

    fn number(nodes: &mut BTreeMap<NodeId, Node>, value: f64) -> NodeId {
        insert(
            nodes,
            Node::Const {
                value: Value::Float(value),
            },
        )
    }

    fn convert(nodes: &mut BTreeMap<NodeId, Node>, source: NodeId) -> NodeId {
        call(nodes, "to_number", vec![source])
    }

    fn user_function() -> UserFunction {
        let mut nodes = BTreeMap::new();
        let parameter = insert(
            &mut nodes,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(7),
            },
        );
        let parameter = convert(&mut nodes, parameter);
        let nine = number(&mut nodes, 9.0);
        let nine = convert(&mut nodes, nine);
        let product = call(&mut nodes, "multiply", vec![parameter, nine]);
        let product = convert(&mut nodes, product);
        let five = number(&mut nodes, 5.0);
        let five = convert(&mut nodes, five);
        let quotient = call(&mut nodes, "divide", vec![product, five]);
        let quotient = convert(&mut nodes, quotient);
        let thirty_two = number(&mut nodes, 32.0);
        let thirty_two = convert(&mut nodes, thirty_two);
        let output = call(&mut nodes, "add", vec![quotient, thirty_two]);
        UserFunction {
            library: "user".into(),
            name: "CelsiusToFahrenheit".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: FunctionParameterId::new(7),
                name: "celsius".into(),
                ty: ScalarType::String,
            }],
            output_name: "fahrenheit".into(),
            output_type: ScalarType::String,
            body: Graph { nodes },
            output,
        }
    }

    fn branch(nodes: &mut BTreeMap<NodeId, Node>, operation: AggregateOp) -> NodeId {
        let source = insert(
            nodes,
            Node::Aggregate {
                function: operation,
                collection: vec!["data".into()],
                value: vec!["temp".into()],
                expression: None,
                arg: None,
            },
        );
        let source = convert(nodes, source);
        let nine = number(nodes, 9.0);
        let nine = convert(nodes, nine);
        let product = call(nodes, "multiply", vec![source, nine]);
        let product = convert(nodes, product);
        let five = number(nodes, 5.0);
        let five = convert(nodes, five);
        let quotient = call(nodes, "divide", vec![product, five]);
        let quotient = convert(nodes, quotient);
        let thirty_two = number(nodes, 32.0);
        let thirty_two = convert(nodes, thirty_two);
        let sum = call(nodes, "add", vec![quotient, thirty_two]);
        let sum = convert(nodes, sum);
        call(nodes, "round", vec![sum])
    }

    fn project() -> Project {
        let mut nodes = BTreeMap::new();
        let month = insert(
            &mut nodes,
            Node::SourceField {
                path: vec!["month".into()],
                frame: Some(vec!["data".into()]),
            },
        );
        let hyphen = insert(
            &mut nodes,
            Node::Const {
                value: Value::String("-".into()),
            },
        );
        let group = call(&mut nodes, "substring_before", vec![month, hyphen]);
        let minimum = branch(&mut nodes, AggregateOp::Min);
        let maximum = branch(&mut nodes, AggregateOp::Max);
        let average = branch(&mut nodes, AggregateOp::Avg);
        let source = SchemaNode::group(
            "Temperatures",
            vec![
                SchemaNode::group(
                    "data",
                    vec![
                        SchemaNode::scalar("temp", ScalarType::Float).attribute(),
                        SchemaNode::scalar("month", ScalarType::String).attribute(),
                        SchemaNode::scalar("desc", ScalarType::String).attribute(),
                    ],
                )
                .repeating(),
            ],
        );
        let target = SchemaNode::group(
            "Temperatures",
            vec![
                SchemaNode::group(
                    "YearlyStats",
                    vec![
                        SchemaNode::scalar("MinimumTemp", ScalarType::Float),
                        SchemaNode::scalar("MaximumTemp", ScalarType::Float),
                        SchemaNode::scalar("AverageTemp", ScalarType::Float),
                        SchemaNode::scalar("Year", ScalarType::Int).attribute(),
                    ],
                )
                .repeating(),
            ],
        );
        let row = Scope {
            target_field: "YearlyStats".into(),
            iteration: ScopeIteration::Source(vec!["data".into()]),
            group_by: Some(group),
            bindings: vec![
                Binding {
                    target_field: "Year".into(),
                    node: group,
                },
                Binding {
                    target_field: "MinimumTemp".into(),
                    node: minimum,
                },
                Binding {
                    target_field: "MaximumTemp".into(),
                    node: maximum,
                },
                Binding {
                    target_field: "AverageTemp".into(),
                    node: average,
                },
            ],
            ..Scope::default()
        };
        let options = FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        };
        Project {
            source,
            target,
            source_path: Some("temperatures.xml".into()),
            target_path: Some("yearly.xml".into()),
            source_options: options.clone(),
            target_options: options,
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: BTreeMap::from([(FunctionId::new(9), user_function())]),
            graph: Graph { nodes },
            root: Scope {
                children: vec![row],
                ..Scope::default()
            },
        }
    }

    #[test]
    fn exact_group_plan_folds_only_three_private_conversion_branches() {
        let project = project();
        let plan = group_plan(&project).expect("closed synthetic graph must match");
        assert_eq!(plan.absorbed.len(), 42);
        assert_eq!(plan.products.len(), 3);
        assert_eq!(
            group_body_aliases(project.user_functions.values().next().unwrap())
                .unwrap()
                .len(),
            6
        );
    }

    #[test]
    fn changed_numeric_call_shared_branch_and_scope_control_reject_fold() {
        let base = project();
        let mut changed = base.clone();
        let wrapped = changed
            .graph
            .nodes
            .iter()
            .find_map(|(&id, node)| match node {
                Node::Call { function, .. } if function == "to_number" => Some(id),
                _ => None,
            })
            .unwrap();
        if let Some(Node::Call { function, .. }) = changed.graph.nodes.get_mut(&wrapped) {
            *function = "concat".into();
        }
        assert!(group_plan(&changed).is_none());

        let mut changed = base.clone();
        changed.root.children[0].sort_by = Some(changed.root.children[0].group_by.unwrap());
        assert!(group_plan(&changed).is_none());

        let mut changed = base.clone();
        changed.root.children[0].bindings[1].node = changed.root.children[0].bindings[2].node;
        assert!(group_plan(&changed).is_none());

        let mut changed = base.clone();
        if let SchemaKind::Group { children, .. } = &mut changed.source.kind {
            children[0].repeating = false;
        }
        assert!(group_plan(&changed).is_none());

        let mut changed = base;
        let function = changed.user_functions.values_mut().next().unwrap();
        if let Some(Node::Call { function: name, .. }) =
            function.body.nodes.get_mut(&function.output)
        {
            *name = "subtract".into();
        }
        assert!(group_plan(&changed).is_none());
    }
}
