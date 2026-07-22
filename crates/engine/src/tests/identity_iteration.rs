use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

use crate::{run, validate};

fn group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children)
}

fn repeated_group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    group(name, children).repeating()
}

#[test]
fn nested_identity_scope_preserves_outer_collection_frames() {
    let source = group(
        "Source",
        vec![repeated_group(
            "Item",
            vec![
                SchemaNode::scalar("kind", ScalarType::String),
                repeated_group(
                    "Detail",
                    vec![SchemaNode::scalar("amount", ScalarType::Int)],
                ),
            ],
        )],
    );
    let target = group(
        "Result",
        vec![repeated_group(
            "Record",
            vec![repeated_group(
                "Item",
                vec![repeated_group(
                    "Detail",
                    vec![repeated_group(
                        "Projection",
                        vec![
                            SchemaNode::scalar("kind", ScalarType::String),
                            SchemaNode::scalar("amount", ScalarType::Int),
                        ],
                    )],
                )],
            )],
        )],
    );
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["Item".into()]),
                        path: vec!["kind".into()],
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["Item".into(), "Detail".into()]),
                        path: vec!["amount".into()],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Record".into(),
                iteration: mapping::ScopeIteration::Source(Vec::new()),
                children: vec![Scope {
                    target_field: "Item".into(),
                    iteration: mapping::ScopeIteration::Source(vec!["Item".into()]),
                    children: vec![Scope {
                        target_field: "Detail".into(),
                        iteration: mapping::ScopeIteration::Source(vec!["Detail".into()]),
                        children: vec![Scope {
                            target_field: "Projection".into(),
                            iteration: mapping::ScopeIteration::Source(Vec::new()),
                            bindings: vec![
                                Binding {
                                    target_field: "kind".into(),
                                    node: 0,
                                },
                                Binding {
                                    target_field: "amount".into(),
                                    node: 1,
                                },
                            ],
                            ..Scope::default()
                        }],
                        ..Scope::default()
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let input = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![Instance::Group(vec![
            (
                "kind".into(),
                Instance::Scalar(Value::String("Travel".into())),
            ),
            (
                "Detail".into(),
                Instance::Repeated(vec![Instance::Group(vec![(
                    "amount".into(),
                    Instance::Scalar(Value::Int(42)),
                )])]),
            ),
        ])]),
    )]);

    assert!(validate(&project).is_empty());
    let Ok(output) = run(&project, &input) else {
        panic!("nested identity scopes should execute")
    };
    let Some(projection) = output
        .field("Record")
        .and_then(Instance::as_repeated)
        .and_then(|items| items.first())
        .and_then(|record| record.field("Item"))
        .and_then(Instance::as_repeated)
        .and_then(|items| items.first())
        .and_then(|item| item.field("Detail"))
        .and_then(Instance::as_repeated)
        .and_then(|items| items.first())
        .and_then(|detail| detail.field("Projection"))
        .and_then(Instance::as_repeated)
        .and_then(|items| items.first())
    else {
        panic!("one nested projection")
    };
    assert_eq!(
        projection.field("kind"),
        Some(&Instance::Scalar(Value::String("Travel".into())))
    );
    assert_eq!(
        projection.field("amount"),
        Some(&Instance::Scalar(Value::Int(42)))
    );
}
