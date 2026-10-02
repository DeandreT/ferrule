use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

use super::{DynamicSourceLoader, EngineError, ExecutionContext, run_with_context, validate};

struct FixtureLoader;

impl DynamicSourceLoader for FixtureLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        if source != "document" {
            return Err(format!("unexpected source {source}"));
        }
        let value = match path {
            "first.xml" => "alpha",
            "second.xml" => "beta",
            other => return Err(format!("unexpected path {other}")),
        };
        Ok(Arc::new(Instance::Group(
            (vec![(
                "Item".into(),
                Instance::Repeated(vec![Instance::Group(
                    (vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String(value.into())),
                    )])
                    .into(),
                )]),
            )])
            .into(),
        )))
    }
}

fn dynamic_project() -> Project {
    let source = SchemaNode::group(
        "Files",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    let document = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let row = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Path", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
    )
    .repeating();
    Project {
        source,
        target: SchemaNode::group("Output", vec![row]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: document,
            options: Default::default(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["File".into()]),
                        path: Vec::new(),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Path".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn dynamic_sources_keep_each_loaded_document_in_its_driver_context() {
    let project = dynamic_project();
    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    let source = Instance::Group(
        (vec![(
            "File".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("first.xml".into())),
                Instance::Scalar(Value::String("second.xml".into())),
            ]),
        )])
        .into(),
    );
    let execution = ExecutionContext::new(Path::new("mapping.ferrule.json"))
        .with_dynamic_source_loader(&FixtureLoader);

    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("Row"),
        Some(&Instance::Repeated(vec![
            Instance::Group(
                (vec![
                    (
                        "Path".into(),
                        Instance::Scalar(Value::String("first.xml".into())),
                    ),
                    (
                        "Value".into(),
                        Instance::Scalar(Value::String("alpha".into())),
                    ),
                ])
                .into()
            ),
            Instance::Group(
                (vec![
                    (
                        "Path".into(),
                        Instance::Scalar(Value::String("second.xml".into())),
                    ),
                    (
                        "Value".into(),
                        Instance::Scalar(Value::String("beta".into())),
                    ),
                ])
                .into()
            ),
        ]))
    );
}

#[test]
fn dynamic_sources_require_a_host_loader() {
    let project = dynamic_project();
    let source = Instance::Group(
        (vec![(
            "File".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::String("first.xml".into()))]),
        )])
        .into(),
    );
    assert_eq!(
        super::run(&project, &source),
        Err(EngineError::MissingDynamicSourceLoader {
            source_name: "document".into(),
        })
    );
}

fn owned_dynamic_project() -> Project {
    use mapping::{
        AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, NamedTarget,
        SequenceExpr,
    };

    let mut project = dynamic_project();
    let sequence = |item| SequenceExpr::Generate {
        from: None,
        to: 2,
        item,
    };
    let rows = |item| Scope {
        target_field: "Generated".into(),
        iteration: ScopeIteration::Sequence(sequence(item)),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    };
    let generated = SchemaNode::group(
        "Generated",
        vec![SchemaNode::scalar("Value", ScalarType::Int)],
    )
    .repeating();
    let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!()
    };
    children.extend([
        generated.clone(),
        SchemaNode::scalar("Flag", ScalarType::Bool),
        SchemaNode::scalar("Selected", ScalarType::Int),
        SchemaNode::scalar("Count", ScalarType::Int),
    ]);
    project.root.children.push(rows(10));
    project.root.bindings.extend([
        Binding {
            target_field: "Flag".into(),
            node: 80,
        },
        Binding {
            target_field: "Selected".into(),
            node: 81,
        },
        Binding {
            target_field: "Count".into(),
            node: 82,
        },
    ]);
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: SchemaNode::group("Other", vec![generated]),
        options: Default::default(),
        root: Scope {
            children: vec![rows(20)],
            ..Scope::default()
        },
    });
    for item in [10, 20, 30, 40, 50, 60] {
        project.graph.nodes.insert(
            item,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
    }
    project.graph.nodes.extend([
        (
            2,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            5,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            6,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            80,
            Node::SequenceExists {
                sequence: sequence(30),
                predicate: 5,
            },
        ),
        (
            81,
            Node::SequenceItemAt {
                sequence: sequence(40),
                index: 4,
            },
        ),
        (
            82,
            Node::SequenceAggregate {
                function: AggregateOp::Count,
                sequence: sequence(50),
                predicate: None,
                expression: None,
                arg: None,
            },
        ),
    ]);
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: sequence(60),
        },
        selection: FailureSelection::WhenFalse { predicate: 5 },
        message: None,
    });
    project
}

fn rejects_dynamic_item(
    project: &Project,
    owner: super::ValidationOwner,
    expression: u32,
    item: u32,
) {
    let issues = validate(project);
    let expected = format!(
        "expression {expression} references generated sequence item node {item} outside its owning context"
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.owner.as_ref() == Some(&owner) && issue.message == expected),
        "{issues:#?}"
    );
}

#[test]
fn dynamic_paths_have_no_outer_generated_item_permission_for_any_owner() {
    for item in [10, 20, 30, 40, 50, 60] {
        for indirect in [false, true] {
            let mut project = owned_dynamic_project();
            project.graph.nodes.insert(
                70,
                Node::Call {
                    function: "string".into(),
                    args: vec![item],
                },
            );
            let expression = if indirect { 70 } else { item };
            project.extra_sources[0].dynamic_path.as_mut().unwrap().node = expression;
            rejects_dynamic_item(
                &project,
                super::ValidationOwner::Endpoint(super::ValidationEndpoint::NamedSource {
                    index: 0,
                    name: "document".into(),
                }),
                expression,
                item,
            );
        }
    }
}

fn use_private_dynamic_join(project: &mut Project) {
    project.graph.nodes.insert(
        82,
        Node::SequenceAggregate {
            function: mapping::AggregateOp::Join,
            sequence: mapping::SequenceExpr::Tokenize {
                input: 0,
                delimiter: 6,
                item: 50,
            },
            predicate: None,
            expression: Some(50),
            arg: Some(6),
        },
    );
    project
        .root
        .bindings
        .retain(|binding| binding.target_field != "Count");
    project.extra_sources[0].dynamic_path.as_mut().unwrap().node = 82;
}

#[test]
fn dynamic_paths_allow_driver_fields_and_reducer_private_items() {
    for private in [false, true] {
        let mut project = owned_dynamic_project();
        if private {
            use_private_dynamic_join(&mut project);
        }
        assert!(validate(&project).is_empty(), "{:#?}", validate(&project));
        let source = Instance::Group(
            (vec![(
                "File".into(),
                Instance::Repeated(vec![
                    Instance::Scalar(Value::String("first.xml".into())),
                    Instance::Scalar(Value::String("second.xml".into())),
                ]),
            )])
            .into(),
        );
        let context = ExecutionContext::new(Path::new("mapping.ferrule.json"))
            .with_dynamic_source_loader(&FixtureLoader);
        let output = run_with_context(&project, &source, &context)
            .expect("legal dynamic driver/reducer context");
        let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].field("Value"),
            Some(&Instance::Scalar(Value::String("alpha".into())))
        );
        assert_eq!(
            rows[1].field("Value"),
            Some(&Instance::Scalar(Value::String("beta".into())))
        );
        assert_eq!(
            output
                .field("Generated")
                .and_then(Instance::as_repeated)
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn dynamic_reducer_inputs_and_foreign_private_values_remain_confined() {
    for foreign_value in [false, true] {
        let mut project = owned_dynamic_project();
        use_private_dynamic_join(&mut project);
        let Node::SequenceAggregate {
            sequence,
            expression,
            ..
        } = project.graph.nodes.get_mut(&82).unwrap()
        else {
            unreachable!()
        };
        if foreign_value {
            *expression = Some(10);
        } else {
            *sequence = mapping::SequenceExpr::Tokenize {
                input: 70,
                delimiter: 6,
                item: 50,
            };
        }
        project.graph.nodes.insert(
            70,
            Node::Call {
                function: "string".into(),
                args: vec![10],
            },
        );
        rejects_dynamic_item(
            &project,
            super::ValidationOwner::GraphNode {
                function: None,
                node: 82,
            },
            82,
            10,
        );
    }
}
