use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    NamedSource, Node, Project, Scope, SequenceExpr,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-item-at-ports-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(computed: bool, function: AggregateOp) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Item", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Selected", ScalarType::Int)],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: Default::default(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(100),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "add".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    4,
                    Node::Aggregate {
                        function,
                        collection: vec!["Item".into()],
                        value: if computed {
                            vec![]
                        } else {
                            vec!["Value".into()]
                        },
                        expression: computed.then_some(2),
                        arg: (function == AggregateOp::ItemAt).then_some(3),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Selected".into(),
                node: 4,
            }],
            ..Default::default()
        },
    }
}

fn source() -> Instance {
    Instance::Group(
        vec![(
            "Item".into(),
            Instance::Repeated(
                [10, 20, 30]
                    .into_iter()
                    .map(|value| {
                        Instance::Group(
                            vec![("Value".into(), Instance::Scalar(Value::Int(value)))].into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn assert_item_at_roundtrip(mut project: Project, source: Instance, selected: i64) {
    let directory = Scratch::new();
    let issues = engine::validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    let expected = engine::run(&project, &source).unwrap();
    assert_eq!(
        expected.field("Selected"),
        Some(&Instance::Scalar(Value::Int(selected)))
    );
    for round in 0..2 {
        let path = directory.0.join(format!("round-{round}.mfd"));
        let report =
            mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let text = std::fs::read_to_string(&path).unwrap();
        let document = roxmltree::Document::parse(&text).unwrap();
        let component = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("item-at")
            })
            .unwrap();
        let inputs = component
            .children()
            .find(|node| node.has_tag_name("sources"))
            .unwrap()
            .children()
            .filter(|node| node.has_tag_name("datapoint"))
            .collect::<Vec<_>>();
        assert_eq!(inputs.len(), 2, "{text}");
        for (position, input) in inputs.iter().enumerate() {
            assert_eq!(input.attribute("pos"), Some(position.to_string().as_str()));
            assert!(input.attribute("key").is_some());
        }
        let imported = mfd::import(&path).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            engine::run(&imported.project, &source).unwrap(),
            expected,
            "imported: {:#?}\n{text}",
            imported.project
        );
        project = imported.project;
    }
}

#[test]
fn direct_and_computed_item_at_export_native_nodes_and_position_ports() {
    for computed in [false, true] {
        assert_item_at_roundtrip(
            project(computed, AggregateOp::ItemAt),
            source(),
            if computed { 120 } else { 20 },
        );
    }
}

#[test]
fn item_at_without_a_collection_dependent_expression_is_refused_before_publication() {
    for root_field in [false, true] {
        let directory = Scratch::new();
        let path = directory.0.join("unchanged.mfd");
        let mut project = project(true, AggregateOp::ItemAt);
        let expression = if root_field {
            let SchemaNode {
                kind: ir::SchemaKind::Group { children, .. },
                ..
            } = &mut project.source
            else {
                unreachable!()
            };
            children.push(SchemaNode::scalar("Header", ScalarType::Int));
            project.graph.nodes.insert(
                5,
                Node::SourceField {
                    path: vec!["Header".into()],
                    frame: None,
                },
            );
            5
        } else {
            1
        };
        if let Some(Node::Aggregate {
            expression: target, ..
        }) = project.graph.nodes.get_mut(&4)
        {
            *target = Some(expression);
        }
        let input = if root_field {
            Instance::Group(
                vec![
                    ("Header".into(), Instance::Scalar(Value::Int(100))),
                    ("Item".into(), source().field("Item").unwrap().clone()),
                ]
                .into(),
            )
        } else {
            source()
        };
        assert_eq!(
            engine::run(&project, &input).unwrap().field("Selected"),
            Some(&Instance::Scalar(Value::Int(100)))
        );
        std::fs::write(&path, "original").unwrap();
        let error =
            mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd).unwrap_err();
        assert!(matches!(error, mfd::MfdError::Unsupported(_)), "{error:?}");
        assert!(
            error
                .to_string()
                .contains("cannot preserve the collection's cardinality")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
    }
}

fn assert_cardinality_refusal(project: &Project, source: &Instance, expected: i64) {
    let issues = engine::validate(project);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(
        engine::run(project, source).unwrap().field("Selected"),
        Some(&Instance::Scalar(Value::Int(expected)))
    );
    let directory = Scratch::new();
    let path = directory.0.join("existing.mfd");
    std::fs::write(&path, "original").unwrap();
    let error =
        mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot preserve the collection's cardinality"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn deeper_repetition_cannot_prove_the_outer_item_at_cardinality() {
    let mut project = project(true, AggregateOp::ItemAt);
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Item",
                vec![
                    SchemaNode::group("Child", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Child".into(), "Value".into()],
            frame: None,
        },
    );
    let source = Instance::Group(
        vec![(
            "Item".into(),
            Instance::Repeated(
                [[10, 11], [20, 21]]
                    .into_iter()
                    .map(|values| {
                        Instance::Group(
                            vec![(
                                "Child".into(),
                                Instance::Repeated(
                                    values
                                        .into_iter()
                                        .map(|value| {
                                            Instance::Group(
                                                vec![(
                                                    "Value".into(),
                                                    Instance::Scalar(Value::Int(value)),
                                                )]
                                                .into(),
                                            )
                                        })
                                        .collect(),
                                ),
                            )]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    );
    assert_cardinality_refusal(&project, &source, 120);
}

#[test]
fn reducer_inputs_do_not_prove_item_at_cardinality() {
    let mut project = project(true, AggregateOp::ItemAt);
    project.graph.nodes.insert(
        6,
        Node::Aggregate {
            function: AggregateOp::Sum,
            collection: vec!["Item".into()],
            value: vec![],
            expression: Some(0),
            arg: None,
        },
    );
    if let Some(Node::Aggregate { expression, .. }) = project.graph.nodes.get_mut(&4) {
        *expression = Some(6);
    }
    assert_cardinality_refusal(&project, &source(), 60);
}

#[test]
fn one_lazy_branch_is_not_a_collection_cardinality_witness() {
    let mut project = project(true, AggregateOp::ItemAt);
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    project.graph.nodes.insert(
        2,
        Node::If {
            condition: 6,
            then: 1,
            else_: 0,
        },
    );
    assert_cardinality_refusal(&project, &source(), 100);
}

#[test]
fn a_collection_dependent_condition_retains_item_at_cardinality() {
    let mut project = project(true, AggregateOp::ItemAt);
    project.graph.nodes.extend([
        (
            6,
            Node::Call {
                function: "greater_than".into(),
                args: vec![0, 7],
            },
        ),
        (
            7,
            Node::Const {
                value: Value::Int(15),
            },
        ),
        (
            8,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            2,
            Node::If {
                condition: 6,
                then: 1,
                else_: 8,
            },
        ),
    ]);
    assert_item_at_roundtrip(project, source(), 100);
}

#[test]
fn computed_named_collection_is_refused_without_losing_its_owner() {
    let mut project = project(true, AggregateOp::ItemAt);
    let named_schema = project.source.clone();
    project.source =
        SchemaNode::group("Source", vec![SchemaNode::scalar("Other", ScalarType::Int)]);
    project.extra_sources.push(NamedSource {
        name: "Beta".into(),
        path: "beta.xml".into(),
        schema: named_schema,
        options: Default::default(),
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: Some(vec!["Beta".into(), "Item".into()]),
        },
    );
    if let Some(Node::Aggregate { collection, .. }) = project.graph.nodes.get_mut(&4) {
        *collection = vec!["Beta".into(), "Item".into()];
    }
    let primary = Instance::Group(vec![("Other".into(), Instance::Scalar(Value::Int(1)))].into());
    let output =
        engine::run_with_sources(&project, &primary, vec![("Beta".into(), source())]).unwrap();
    assert_eq!(
        output.field("Selected"),
        Some(&Instance::Scalar(Value::Int(120)))
    );
    let directory = Scratch::new();
    let path = directory.0.join("existing.mfd");
    std::fs::write(&path, "original").unwrap();
    let error =
        mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot preserve the collection's cardinality"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn direct_generated_sequence_item_at_keeps_its_two_ports() {
    let mut project = project(false, AggregateOp::ItemAt);
    project.graph.nodes.extend([
        (
            5,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            4,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 6,
                    item: 5,
                },
                index: 3,
            },
        ),
    ]);
    assert_item_at_roundtrip(project, source(), 2);
}

#[test]
fn generated_aggregate_item_at_refuses_unproved_sequence_ownership() {
    for (filtered, computed) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut project = project(false, AggregateOp::ItemAt);
        project.graph.nodes.extend([
            (
                5,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            ),
            (
                6,
                Node::Const {
                    value: Value::Int(3),
                },
            ),
            (
                7,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                8,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![5, 7],
                },
            ),
            (
                2,
                Node::Call {
                    function: "add".into(),
                    args: vec![5, 1],
                },
            ),
            (
                4,
                Node::SequenceAggregate {
                    function: AggregateOp::ItemAt,
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 6,
                        item: 5,
                    },
                    predicate: filtered.then_some(8),
                    expression: computed.then_some(2),
                    arg: Some(3),
                },
            ),
        ]);
        let value = if filtered { 3 } else { 2 };
        let expected = value + if computed { 100 } else { 0 };
        assert_eq!(
            engine::run(&project, &source()).unwrap().field("Selected"),
            Some(&Instance::Scalar(Value::Int(expected)))
        );
        assert_transformed_sequence_refusal(&project);
    }
}

#[test]
fn generated_item_at_refuses_broadcast_only_expressions_before_publication() {
    let mut project = project(false, AggregateOp::ItemAt);
    project.graph.nodes.extend([
        (
            5,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            4,
            Node::SequenceAggregate {
                function: AggregateOp::ItemAt,
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 6,
                    item: 5,
                },
                predicate: None,
                expression: Some(1),
                arg: Some(3),
            },
        ),
    ]);
    assert_eq!(
        engine::run(&project, &source()).unwrap().field("Selected"),
        Some(&Instance::Scalar(Value::Int(100)))
    );
    assert_transformed_sequence_refusal(&project);
}

fn assert_transformed_sequence_refusal(project: &Project) {
    let directory = Scratch::new();
    let path = directory.0.join("existing.mfd");
    std::fs::write(&path, "original").unwrap();
    let error =
        mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot preserve its private sequence ownership"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn generated_aggregate_item_at_refuses_outer_generated_index() {
    let mut project = project(false, AggregateOp::ItemAt);
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Selected", ScalarType::Int)],
            )
            .repeating(),
        ],
    );
    project.graph.nodes.extend([
        (
            5,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            9,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            4,
            Node::SequenceAggregate {
                function: AggregateOp::ItemAt,
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 6,
                    item: 5,
                },
                predicate: None,
                expression: None,
                arg: Some(9),
            },
        ),
    ]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: mapping::ScopeIteration::Sequence(SequenceExpr::Generate {
                from: None,
                to: 6,
                item: 9,
            }),
            bindings: vec![Binding {
                target_field: "Selected".into(),
                node: 4,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let issues = engine::validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    let output = engine::run(&project, &source()).unwrap();
    assert_eq!(
        output.field("Rows"),
        Some(&Instance::Repeated(
            (1..=3)
                .map(|value| {
                    Instance::Group(
                        vec![("Selected".into(), Instance::Scalar(Value::Int(value)))].into(),
                    )
                })
                .collect()
        ))
    );
    assert_transformed_sequence_refusal(&project);
}

#[test]
fn joined_item_at_uses_the_same_two_ports() {
    let mut project = project(false, AggregateOp::ItemAt);
    let join = JoinId::new(12);
    let plan = JoinPlan::new(
        JoinSource::new(vec!["Item".into()]),
        JoinSource::new(vec!["Match".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Item".into()],
            vec!["Value".into()],
            vec!["Id".into()],
        )),
    )
    .unwrap();
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Item", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
            SchemaNode::group("Match", vec![SchemaNode::scalar("Id", ScalarType::Int)]).repeating(),
        ],
    );
    project.graph.nodes.extend([
        (
            0,
            Node::JoinField {
                join,
                collection: vec!["Item".into()],
                path: vec!["Value".into()],
            },
        ),
        (
            4,
            Node::JoinAggregate {
                function: AggregateOp::ItemAt,
                join,
                plan,
                expression: Some(0),
                arg: Some(3),
            },
        ),
    ]);
    let source = Instance::Group(
        vec![
            ("Item".into(), source().field("Item").unwrap().clone()),
            (
                "Match".into(),
                Instance::Repeated(
                    [10, 20, 30]
                        .into_iter()
                        .map(|id| {
                            Instance::Group(
                                vec![("Id".into(), Instance::Scalar(Value::Int(id)))].into(),
                            )
                        })
                        .collect(),
                ),
            ),
        ]
        .into(),
    );
    assert_item_at_roundtrip(project, source, 20);
}

#[test]
fn other_collection_aggregates_keep_their_parent_context_slot() {
    let directory = Scratch::new();
    let project = project(true, AggregateOp::Sum);
    let path = directory.0.join("sum.mfd");
    mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let component = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("sum"))
        .unwrap();
    let positions = component
        .children()
        .find(|node| node.has_tag_name("sources"))
        .unwrap()
        .children()
        .filter(|node| node.has_tag_name("datapoint"))
        .map(|node| node.attribute("pos"))
        .collect::<Vec<_>>();
    assert_eq!(positions, vec![Some("0"), Some("1")]);
    let imported = mfd::import(&path).unwrap();
    assert_eq!(
        engine::run(&imported.project, &source()).unwrap(),
        engine::run(&project, &source()).unwrap()
    );
}
