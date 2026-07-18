use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, IterationOutput, JoinConditions, JoinId, JoinKey, JoinPlan,
    JoinSource, JoinSourceCardinality, NamedSource, Node, Project, Scope, ScopeIteration,
    SequenceWindow,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-join-export-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scalar(name: &str, value: &str) -> (String, Instance) {
    (
        name.to_string(),
        Instance::Scalar(Value::String(value.to_string())),
    )
}

fn row(fields: &[(&str, &str)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| scalar(name, value))
            .collect(),
    )
}

fn two_way_plan(join: JoinId) -> (JoinId, JoinPlan) {
    let plan = JoinPlan::new(
        JoinSource::new(vec!["Left".into()]),
        JoinSource::new(vec!["Right".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Left".into()],
            vec!["Id".into()],
            vec!["Code".into()],
        ))
        .and(JoinKey::new(
            vec!["Left".into()],
            vec!["Tenant".into()],
            vec!["Tenant".into()],
        )),
    )
    .unwrap();
    (join, plan)
}

fn two_way_project() -> Project {
    let (join, plan) = two_way_plan(JoinId::new(8));
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Left",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::String),
                        SchemaNode::scalar("Tenant", ScalarType::String),
                        SchemaNode::scalar("Label", ScalarType::String),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "Right",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Tenant", ScalarType::String),
                        SchemaNode::scalar("Description", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Label", ScalarType::String),
                        SchemaNode::group(
                            "Details",
                            vec![SchemaNode::scalar("Description", ScalarType::String)],
                        ),
                        SchemaNode::scalar("Position", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::JoinField {
                        join,
                        collection: vec!["Left".into()],
                        path: vec!["Label".into()],
                    },
                ),
                (
                    1,
                    Node::JoinField {
                        join,
                        collection: vec!["Right".into()],
                        path: vec!["Description".into()],
                    },
                ),
                (2, Node::JoinPosition { join }),
                (
                    3,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin { id: join, plan },
                windows: vec![SequenceWindow::First { count: 3 }],
                bindings: vec![
                    Binding {
                        target_field: "Label".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Position".into(),
                        node: 2,
                    },
                ],
                children: vec![Scope {
                    target_field: "Details".into(),
                    bindings: vec![Binding {
                        target_field: "Description".into(),
                        node: 1,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn two_way_source() -> Instance {
    Instance::Group(vec![
        (
            "Left".into(),
            Instance::Repeated(vec![
                row(&[("Id", "A"), ("Tenant", "T"), ("Label", "L1")]),
                row(&[("Id", "A"), ("Tenant", "T"), ("Label", "L2")]),
                row(&[("Id", "A"), ("Tenant", "X"), ("Label", "LX")]),
            ]),
        ),
        (
            "Right".into(),
            Instance::Repeated(vec![
                row(&[("Code", "A"), ("Tenant", "T"), ("Description", "R1")]),
                row(&[("Code", "A"), ("Tenant", "T"), ("Description", "R2")]),
                row(&[("Code", "A"), ("Tenant", "Y"), ("Description", "RY")]),
            ]),
        ),
    ])
}

fn import_exported(path: &Path) -> mfd::Imported {
    let imported = mfd::import(path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");
    imported
}

#[test]
fn exports_and_round_trips_composite_join_fields_position_and_window() {
    let dir = TempDir::new("two-way");
    let output = dir.path("mapping.mfd");
    let warnings = mfd::export(&two_way_project(), &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let xml = fs::read_to_string(&output).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let joins = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("kind") == Some("32"))
        .collect::<Vec<_>>();
    assert_eq!(joins.len(), 1);
    assert_eq!(
        joins[0]
            .descendants()
            .filter(|node| node.has_tag_name("keypair"))
            .count(),
        2
    );

    let imported = import_exported(&output);
    let result = engine::run(&imported.project, &two_way_source()).unwrap();
    let rows = result.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|row| row.field("Position").and_then(Instance::as_scalar))
            .collect::<Vec<_>>(),
        [
            Some(&Value::Int(1)),
            Some(&Value::Int(2)),
            Some(&Value::Int(3)),
        ]
    );
    assert_eq!(
        rows.iter()
            .map(|row| {
                row.field("Details")
                    .and_then(|details| details.field("Description"))
                    .and_then(Instance::as_scalar)
            })
            .collect::<Vec<_>>(),
        [
            Some(&Value::String("R1".into())),
            Some(&Value::String("R2".into())),
            Some(&Value::String("R1".into())),
        ]
    );
}

#[test]
fn exports_and_round_trips_a_three_way_join() {
    let dir = TempDir::new("three-way");
    let output = dir.path("mapping.mfd");
    let join = JoinId::new(9);
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["Id".into()],
            vec!["AId".into()],
        )),
    )
    .unwrap()
    .then(
        JoinSource::new(vec!["C".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["B".into()],
            vec!["Id".into()],
            vec!["BId".into()],
        )),
    )
    .unwrap();
    let project = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("A", vec![SchemaNode::scalar("Id", ScalarType::String)])
                    .repeating(),
                SchemaNode::group(
                    "B",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::String),
                        SchemaNode::scalar("AId", ScalarType::String),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "C",
                    vec![
                        SchemaNode::scalar("BId", ScalarType::String),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::JoinField {
                    join,
                    collection: vec!["C".into()],
                    path: vec!["Value".into()],
                },
            )]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin { id: join, plan },
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let warnings = mfd::export(&project, &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported = import_exported(&output);
    let row_scope = &imported.project.root.children[0];
    let Some((_, imported_plan)) = row_scope.join() else {
        panic!("expected imported joined scope");
    };
    assert_eq!(imported_plan.sources().count(), 3);
    assert_eq!(imported_plan.stages().count(), 2);
}

#[test]
fn unsupported_join_plan_is_not_partially_exported() {
    let dir = TempDir::new("unsupported");
    let output = dir.path("mapping.mfd");
    let mut project = two_way_project();
    let join = JoinId::new(8);
    let unsupported = JoinPlan::new(
        JoinSource::new(vec!["Left".into()]),
        JoinSource::new(vec!["External".into(), "Right".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Left".into()],
            vec!["Id".into()],
            vec!["Code".into()],
        )),
    )
    .unwrap();
    project.root.children[0].iteration = ScopeIteration::InnerJoin {
        id: join,
        plan: unsupported,
    };
    let warnings = mfd::export(&project, &output).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("not in an exported source schema"));
    let xml = fs::read_to_string(output).unwrap();
    assert!(!xml.contains("kind=\"32\""));
}

fn root_aggregate_project() -> Project {
    let join = JoinId::new(41);
    let plan = JoinPlan::new(
        JoinSource::new(vec!["Left".into()]),
        JoinSource::new(vec!["Right".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Left".into()],
            vec!["Id".into()],
            vec!["Code".into()],
        )),
    )
    .unwrap();
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Left",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::String),
                        SchemaNode::scalar("Amount", ScalarType::Int),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "Right",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Quantity", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("TotalCount", ScalarType::Int),
                SchemaNode::scalar("TotalSum", ScalarType::Int),
                SchemaNode::scalar("JoinedValues", ScalarType::String),
            ],
        ),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::JoinField {
                        join,
                        collection: vec!["Left".into()],
                        path: vec!["Amount".into()],
                    },
                ),
                (
                    1,
                    Node::JoinField {
                        join,
                        collection: vec!["Right".into()],
                        path: vec!["Quantity".into()],
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "multiply".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    3,
                    Node::JoinAggregate {
                        function: AggregateOp::Sum,
                        join,
                        plan: plan.clone(),
                        expression: Some(2),
                        arg: None,
                    },
                ),
                (
                    4,
                    Node::JoinAggregate {
                        function: AggregateOp::Count,
                        join,
                        plan: plan.clone(),
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "add".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (
                    7,
                    Node::JoinAggregate {
                        function: AggregateOp::Join,
                        join,
                        plan,
                        expression: Some(5),
                        arg: Some(6),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "TotalCount".into(),
                    node: 4,
                },
                Binding {
                    target_field: "TotalSum".into(),
                    node: 3,
                },
                Binding {
                    target_field: "JoinedValues".into(),
                    node: 7,
                },
            ],
            ..Scope::default()
        },
    }
}

fn root_aggregate_source() -> Instance {
    Instance::Group(vec![
        (
            "Left".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    scalar("Id", "A"),
                    ("Amount".into(), Instance::Scalar(Value::Int(2))),
                ]),
                Instance::Group(vec![
                    scalar("Id", "A"),
                    ("Amount".into(), Instance::Scalar(Value::Int(3))),
                ]),
            ]),
        ),
        (
            "Right".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    scalar("Code", "A"),
                    ("Quantity".into(), Instance::Scalar(Value::Int(10))),
                ]),
                Instance::Group(vec![
                    scalar("Code", "A"),
                    ("Quantity".into(), Instance::Scalar(Value::Int(20))),
                ]),
            ]),
        ),
    ])
}

#[test]
fn root_join_aggregates_round_trip_raw_count_computed_values_and_parent_argument() {
    let dir = TempDir::new("root-aggregate");
    let output = dir.path("mapping.mfd");
    let project = root_aggregate_project();
    assert!(engine::validate(&project).is_empty());
    let expected = engine::run(&project, &root_aggregate_source()).unwrap();
    assert_eq!(
        expected.field("TotalCount").and_then(Instance::as_scalar),
        Some(&Value::Int(4))
    );
    assert_eq!(
        expected.field("TotalSum").and_then(Instance::as_scalar),
        Some(&Value::Int(150))
    );
    assert_eq!(
        expected.field("JoinedValues").and_then(Instance::as_scalar),
        Some(&Value::String("12|22|13|23".into()))
    );
    let warnings = mfd::export(&project, &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = fs::read_to_string(output).unwrap();
    assert!(xml.contains("kind=\"32\""));
    assert!(xml.contains("component name=\"count\""));
    assert!(xml.contains("component name=\"sum\""));
    assert!(xml.contains("component name=\"string-join\""));

    let imported = import_exported(&dir.path("mapping.mfd"));
    let aggregate_shapes = imported
        .project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::JoinAggregate {
                function,
                expression,
                arg,
                ..
            } => Some((*function, expression.is_some(), arg.is_some())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(aggregate_shapes.contains(&(AggregateOp::Count, false, false)));
    assert!(aggregate_shapes.contains(&(AggregateOp::Sum, true, false)));
    assert!(aggregate_shapes.contains(&(AggregateOp::Join, true, true)));
    assert_eq!(
        engine::run(&imported.project, &root_aggregate_source()).unwrap(),
        expected
    );
}

#[test]
fn nested_join_aggregate_is_blocked_without_partial_components_or_edges() {
    let dir = TempDir::new("nested-aggregate");
    let output = dir.path("mapping.mfd");
    let mut project = two_way_project();
    let (join, plan) = two_way_plan(JoinId::new(8));
    project.graph.nodes.insert(
        4,
        Node::JoinAggregate {
            function: AggregateOp::Count,
            join,
            plan,
            expression: None,
            arg: None,
        },
    );
    project.graph.nodes.insert(
        5,
        Node::Call {
            function: "string".into(),
            args: vec![4],
        },
    );
    project.root.children[0].children[0].bindings[0].node = 5;

    let warnings = mfd::export(&project, &output).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("nested or correlated"));
    let xml = fs::read_to_string(output).unwrap();
    assert!(xml.contains("kind=\"32\""));
    assert!(!xml.contains("component name=\"count\""));
    assert!(!xml.contains("component name=\"string\""));

    let imported = import_exported(&dir.path("mapping.mfd"));
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .all(|node| !matches!(node, Node::JoinAggregate { .. }))
    );
}

#[test]
fn unsupported_join_aggregate_expression_and_argument_are_atomic() {
    let mut external_expression = root_aggregate_project();
    external_expression.graph.nodes.insert(
        8,
        Node::SourceField {
            path: vec!["Left".into(), "Amount".into()],
            frame: None,
        },
    );
    let Some(Node::JoinAggregate { expression, .. }) = external_expression.graph.nodes.get_mut(&3)
    else {
        panic!("expected computed joined sum");
    };
    *expression = Some(8);
    external_expression
        .graph
        .nodes
        .retain(|id, _| matches!(*id, 3 | 8));
    external_expression.root.bindings = vec![Binding {
        target_field: "TotalSum".into(),
        node: 3,
    }];

    let mut joined_argument = root_aggregate_project();
    let Some(Node::JoinAggregate { arg, .. }) = joined_argument.graph.nodes.get_mut(&7) else {
        panic!("expected joined string aggregate");
    };
    *arg = Some(0);
    joined_argument
        .graph
        .nodes
        .retain(|id, _| matches!(*id, 0 | 1 | 5 | 7));
    joined_argument.root.bindings = vec![Binding {
        target_field: "JoinedValues".into(),
        node: 7,
    }];

    for (tag, project, message, component) in [
        (
            "external-expression",
            external_expression,
            "non-scalar or external context",
            "sum",
        ),
        (
            "joined-argument",
            joined_argument,
            "depends on a joined tuple",
            "string-join",
        ),
    ] {
        let dir = TempDir::new(tag);
        let output = dir.path("mapping.mfd");
        let warnings = mfd::export(&project, &output).unwrap();
        assert_eq!(warnings.len(), 1, "{tag}: {warnings:?}");
        assert!(warnings[0].contains(message), "{tag}: {warnings:?}");
        let xml = fs::read_to_string(&output).unwrap();
        assert!(!xml.contains("kind=\"32\""), "{tag}");
        assert!(
            !xml.contains(&format!("component name=\"{component}\"")),
            "{tag}"
        );
        let imported = import_exported(&output);
        assert!(
            imported
                .project
                .graph
                .nodes
                .values()
                .all(|node| !matches!(node, Node::JoinAggregate { .. }))
        );
    }
}

#[test]
fn mapped_join_sequence_round_trips_named_and_singleton_sources() {
    let dir = TempDir::new("mapped-named-singleton");
    let output = dir.path("mapping.mfd");
    let join = JoinId::new(21);
    let plan = JoinPlan::new(
        JoinSource::singleton(vec!["Order".into(), "CustomerNumber".into()]),
        JoinSource::new(vec!["Customer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Order".into(), "CustomerNumber".into()],
            Vec::new(),
            vec!["Number".into()],
        )),
    )
    .unwrap();
    let customer = SchemaNode::group(
        "Customer",
        vec![
            SchemaNode::scalar("Number", ScalarType::String),
            SchemaNode::scalar("Name", ScalarType::String),
        ],
    );
    let mut project = Project {
        source: SchemaNode::group("Customers", vec![customer.clone().repeating()]),
        target: SchemaNode::group("Result", vec![customer]),
        source_path: Some("customers.xml".into()),
        target_path: Some("result.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Order".into(),
            path: "order.xml".into(),
            schema: SchemaNode::group(
                "Order",
                vec![SchemaNode::scalar("CustomerNumber", ScalarType::String)],
            ),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::JoinField {
                        join,
                        collection: vec!["Customer".into()],
                        path: vec!["Number".into()],
                    },
                ),
                (
                    1,
                    Node::JoinField {
                        join,
                        collection: vec!["Customer".into()],
                        path: vec!["Name".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Customer".into(),
                iteration: ScopeIteration::InnerJoin {
                    id: join,
                    plan: plan.clone(),
                },
                iteration_output: IterationOutput::MappedSequence,
                bindings: vec![
                    Binding {
                        target_field: "Number".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Name".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let primary = Instance::Group(vec![(
        "Customer".into(),
        Instance::Repeated(vec![
            row(&[("Number", "A"), ("Name", "Ada")]),
            row(&[("Number", "B"), ("Name", "Bea")]),
        ]),
    )]);
    let extras = vec![(
        "Order".into(),
        Instance::Group(vec![scalar("CustomerNumber", "B")]),
    )];
    let expected = engine::run_with_sources(&project, &primary, extras.clone()).unwrap();

    let warnings = mfd::export(&project, &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported = import_exported(&output);
    let scope = &imported.project.root.children[0];
    assert_eq!(scope.iteration_output, IterationOutput::MappedSequence);
    let Some((_, imported_plan)) = scope.join() else {
        panic!("expected the mapped target to retain its join");
    };
    assert_eq!(
        imported_plan.sources().next().map(JoinSource::cardinality),
        Some(JoinSourceCardinality::Singleton)
    );
    assert_eq!(
        engine::run_with_sources(&imported.project, &primary, extras).unwrap(),
        expected
    );

    project.graph.nodes.clear();
    project.root.children[0].bindings.clear();
    let structural_output = dir.path("structural.mfd");
    let warnings = mfd::export(&project, &structural_output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = fs::read_to_string(&structural_output).unwrap();
    assert_eq!(xml.matches("<dataconnection type=\"2\"/>").count(), 1);
    let structural = import_exported(&structural_output);
    assert_eq!(
        structural.project.root.children[0].iteration_output,
        IterationOutput::MappedSequence
    );
}
