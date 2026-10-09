use super::*;
use mapping::{Binding, ScopeIteration, SequenceExpr};

fn project() -> Project {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "identity-all")
        .unwrap();
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: serde_json::from_value(case["user_functions"].clone()).unwrap(),
        graph: serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(
                    serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
                ),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 11,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn admitted_composition_never_publishes_a_partial_program() {
    let project = project();
    let actual = lower(&project);
    eprintln!(
        "input={}\nactual={actual:#?}",
        serde_json::to_string(&project).unwrap()
    );
    assert_eq!(
        actual.unwrap_err().into_diagnostics(),
        [Diagnostic::UnsupportedSequence { item: 11 }]
    );
}

#[test]
fn unselected_main_graph_composition_cannot_be_pruned_into_silent_support() {
    let mut project = project();
    let sequence = project.root.children[0].sequence().unwrap().clone();
    project.root.children.clear();
    project
        .graph
        .nodes
        .insert(30, Node::SequenceItemAt { sequence, index: 1 });
    let actual = lower(&project);
    eprintln!(
        "input={}\nactual={actual:#?}",
        serde_json::to_string(&project).unwrap()
    );
    assert_eq!(
        actual.unwrap_err().into_diagnostics(),
        [Diagnostic::UnsupportedSequence { item: 11 }]
    );
}

#[test]
fn ordinary_generated_sequence_lowering_is_unchanged() {
    let mut project = project();
    project.user_functions.clear();
    project.graph.nodes.remove(&11);
    project.root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 2,
        item: 10,
    });
    project.root.children[0].bindings[0].node = 10;
    let actual = lower(&project);
    eprintln!(
        "input={}\nactual={actual:#?}",
        serde_json::to_string(&project).unwrap()
    );
    let program = actual.unwrap();
    assert_eq!(
        program.root.children[0].iteration,
        Some(IterationPlan::generated(GeneratedSequence::Range {
            from: None,
            to: 2,
            item: 10
        }))
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        [2, 10]
    );
}
