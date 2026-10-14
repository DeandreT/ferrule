use std::collections::BTreeMap;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, EdiBoundaryKind, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

fn string(name: &str) -> SchemaNode { SchemaNode::scalar(name, ScalarType::String) }
fn group(name: &str, children: Vec<SchemaNode>) -> SchemaNode { SchemaNode::group(name, children) }
fn required(name: &str, children: Vec<SchemaNode>, names: &[&str]) -> SchemaNode {
    group(name, children).with_required_fields(names.iter().map(|name| (*name).into()).collect()).unwrap()
}

// Independent literal expectation: no importer, config reader or design-derived
// builder supplies any schema, graph node, binding, option or occurrence here.
pub fn expected() -> Project {
    let source = group("Envelope", vec![
        group("ISA", vec![string("A01"), string("A02"), string("A03"), string("A04"), string("A05"), string("A06"), string("A07"), string("A08"), string("A09"), string("A10"), string("A11"), SchemaNode::scalar_fixed("A12", ScalarType::String, "00501"), string("A13"), string("A14"), string("A15"), string("A16")]),
        group("Group", vec![
            group("GS", vec![string("G01"), string("G02"), string("G03"), string("G04"), string("G05"), string("G06"), string("G07"), SchemaNode::scalar_fixed("G08", ScalarType::String, "005010")]),
            group("Message", vec![
                group("ST", vec![SchemaNode::scalar_fixed("S01", ScalarType::String, "990"), string("S02")]),
                group("Item", vec![group("N1", vec![string("Label")])]).repeating(),
                group("SE", vec![string("T01"), string("T02")]),
            ]).repeating(),
            group("GE", vec![string("E01"), string("E02")]),
        ]).repeating(),
        group("IEA", vec![string("I01"), string("I02")]),
    ]);
    let target = required("Target", vec![required("Groups", vec![required("Transactions", vec![required("Batches", vec![
        required("Records", vec![string("Branch"), string("Label")], &["Branch"]).repeating(),
    ], &["Records"]).repeating()], &["Batches"]).repeating()], &["Transactions"]).repeating()], &["Groups"]);
    let anchor = vec!["Group".into(), "Message".into(), "Item".into()];
    let segment = |node| Scope {
        bindings: vec![Binding { target_field: "Branch".into(), node }, Binding { target_field: "Label".into(), node: 2 }],
        ..Default::default()
    };
    Project {
        source, target,
        source_path: Some("source.x12".into()), target_path: Some("target.json".into()),
        source_options: FormatOptions { lenient_segments: true, edi_kind: Some(EdiBoundaryKind::X12), ..Default::default() },
        target_options: FormatOptions { json_document: true, ..Default::default() },
        graph: Graph { nodes: BTreeMap::from([
            (0, Node::Const { value: Value::String("left".into()) }),
            (1, Node::Const { value: Value::String("right".into()) }),
            (2, Node::SourceField { path: vec!["N1".into(), "Label".into()], frame: Some(anchor.clone()) }),
        ]) },
        root: Scope { children: vec![Scope {
            target_field: "Groups".into(), iteration: ScopeIteration::Source(vec!["Group".into()]),
            children: vec![Scope {
            target_field: "Transactions".into(), iteration: ScopeIteration::Source(vec!["Message".into()]),
            children: vec![Scope {
            target_field: "Batches".into(), iteration: ScopeIteration::Source(vec!["Item".into()]),
            children: vec![Scope { target_field: "Records".into(),
                iteration: ScopeIteration::Concatenate(ScopeSequence::new(segment(0), vec![segment(1)])),
                ..Default::default() }], ..Default::default()
        }], ..Default::default() }], ..Default::default() }], ..Default::default() },
        extra_sources: vec![], extra_targets: vec![], failure_rules: vec![], user_functions: Default::default(),
    }
}

pub fn literals() -> Project {
    let mut project = expected();
    project.target = required("Target", vec![required("Groups", vec![required("Transactions", vec![required("Batches", vec![
        required("Records", vec![string("Branch"), string("Label"), SchemaNode::scalar("Enabled", ScalarType::Bool), SchemaNode::scalar("Count", ScalarType::Int)], &["Branch", "Enabled", "Count"]).repeating(),
    ], &["Records"]).repeating()], &["Batches"]).repeating()], &["Transactions"]).repeating()], &["Groups"]);
    project.graph = Graph { nodes: BTreeMap::from([
        (0, Node::Const { value: Value::String("left".into()) }),
        (1, Node::Const { value: Value::String("right".into()) }),
        (2, Node::Const { value: Value::Bool(false) }),
        (3, Node::Const { value: Value::Int(0) }),
        (4, Node::SourceField { path: vec!["N1".into(), "Label".into()], frame: Some(vec!["Group".into(), "Message".into(), "Item".into()]) }),
    ]) };
    let segment = |node| Scope { bindings: vec![
        Binding { target_field: "Branch".into(), node }, Binding { target_field: "Label".into(), node: 4 },
        Binding { target_field: "Enabled".into(), node: 2 }, Binding { target_field: "Count".into(), node: 3 },
    ], ..Default::default() };
    project.root.children[0].children[0].children[0].children[0].iteration = ScopeIteration::Concatenate(ScopeSequence::new(segment(0), vec![segment(1)]));
    project
}
