use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Node,
    Project, Scope, ScopeIteration,
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
                take: Some(3),
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
fn exports_and_round_trips_composite_join_fields_position_and_take() {
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
    assert!(warnings[0].contains("not in the primary source schema"));
    let xml = fs::read_to_string(output).unwrap();
    assert!(!xml.contains("kind=\"32\""));
}

#[test]
fn join_aggregate_warns_once_while_the_structured_join_exports() {
    let dir = TempDir::new("aggregate");
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
    assert!(warnings[0].contains("aggregate over inner join 8 is not exported"));
    let xml = fs::read_to_string(output).unwrap();
    assert!(xml.contains("kind=\"32\""));
    assert!(!xml.contains("component name=\"string\""));
}
