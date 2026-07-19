use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_collection_find_export_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Metadata",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Active", ScalarType::Bool),
                    SchemaNode::scalar("Value", ScalarType::String),
                ],
            )
            .repeating(),
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Wanted", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Result",
                vec![SchemaNode::scalar("Found", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    Project {
        source,
        target,
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Wanted".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Key".into()],
                        frame: Some(vec!["Metadata".into()]),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![1, 0],
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Active".into()],
                        frame: Some(vec!["Metadata".into()]),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![3, 4],
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "and".into(),
                        args: vec![2, 5],
                    },
                ),
                (
                    7,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Metadata".into()]),
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::String("!".into()),
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![7, 8],
                    },
                ),
                (
                    10,
                    Node::CollectionFind {
                        collection: vec!["Metadata".into()],
                        predicate: 6,
                        value: 9,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Result".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                bindings: vec![Binding {
                    target_field: "Found".into(),
                    node: 10,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn row(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), Instance::Scalar(value)))
            .collect(),
    )
}

#[test]
fn scalar_filter_roundtrip_rebuilds_collection_find_context() {
    let directory = TempDir::new();
    let design = directory.0.join("mapping.mfd");
    let project = project();
    let source = Instance::Group(vec![
        (
            "Metadata".into(),
            Instance::Repeated(vec![
                row([
                    ("Key", Value::String("A".into())),
                    ("Active", Value::Bool(true)),
                    ("Value", Value::String("alpha".into())),
                ]),
                row([
                    ("Key", Value::String("B".into())),
                    ("Active", Value::Bool(true)),
                    ("Value", Value::String("bravo".into())),
                ]),
            ]),
        ),
        (
            "Item".into(),
            Instance::Repeated(vec![
                row([("Wanted", Value::String("A".into()))]),
                row([("Wanted", Value::String("B".into()))]),
            ]),
        ),
    ]);

    assert!(mfd::export(&project, &design).unwrap().is_empty());
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::CollectionFind { .. }))
    );
    assert_eq!(
        engine::run(&project, &source).unwrap(),
        engine::run(&imported.project, &source).unwrap()
    );
}
