use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope};

use crate::{EngineError, SelectedTargetOutput, TargetSelection, run_outputs, run_selected_target};

fn schema(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

fn project() -> Project {
    Project {
        source: schema("Source"),
        target: schema("Primary"),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![
            NamedTarget {
                name: "selected".into(),
                path: None,
                schema: schema("Selected"),
                options: Default::default(),
                root: scope(1),
            },
            NamedTarget {
                name: "broken".into(),
                path: None,
                schema: schema("Broken"),
                options: Default::default(),
                root: scope(3),
            },
        ],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("primary".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("selected".into()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "normalize_space".into(),
                        args: vec![2],
                    },
                ),
            ]
            .into(),
        },
        root: scope(0),
    }
}

fn source() -> Instance {
    Instance::Group(Vec::new())
}

#[test]
fn selected_targets_do_not_evaluate_other_target_scopes() -> Result<(), EngineError> {
    let project = project();

    let SelectedTargetOutput::Primary(primary) =
        run_selected_target(&project, &source(), TargetSelection::Primary)?
    else {
        panic!("primary selection must produce the primary variant");
    };
    assert_eq!(
        primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("primary".into()))
    );

    let SelectedTargetOutput::Named(selected) =
        run_selected_target(&project, &source(), TargetSelection::Named("selected"))?
    else {
        panic!("named selection must produce the named variant");
    };
    assert_eq!(selected.name, "selected");
    assert_eq!(
        selected
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("selected".into()))
    );

    assert!(run_outputs(&project, &source()).is_err());
    Ok(())
}

#[test]
fn selected_target_names_are_validated_before_evaluation() {
    assert_eq!(
        run_selected_target(&project(), &source(), TargetSelection::Named("missing")),
        Err(EngineError::UnknownTarget {
            name: "missing".into()
        })
    );
}

#[test]
fn duplicate_selected_target_names_are_ambiguous() {
    let mut project = project();
    let duplicate = project.extra_targets[0].clone();
    project.extra_targets.push(duplicate);

    assert_eq!(
        run_selected_target(&project, &source(), TargetSelection::Named("selected")),
        Err(EngineError::AmbiguousTarget {
            name: "selected".into()
        })
    );
}
