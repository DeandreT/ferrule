use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("ferrule_mfd_datetime_add_{}", std::process::id()));
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
    let nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("2023-01-31T00:00:00Z".to_string()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("P1M".to_string()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("-P1M".to_string()),
            },
        ),
        (
            3,
            Node::Call {
                function: "datetime_add".to_string(),
                args: vec![0, 1, 2],
            },
        ),
    ]);
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Unused", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        ),
        source_path: Some("source.xml".to_string()),
        target_path: Some("target.xml".to_string()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Result".to_string(),
                node: 3,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn three_input_datetime_add_exports_as_growable_and_reimports() {
    let dir = TempDir::new();
    let design = dir.0.join("datetime-add.mfd");
    let warnings = mfd::export(&project(), &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    assert!(xml.contains("name=\"datetime-add\" library=\"lang\""));
    assert!(xml.contains("growable=\"1\" growablebasename=\"duration\""));

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let output = engine::run(
        &imported.project,
        &Instance::Group(vec![(
            "Unused".to_string(),
            Instance::Scalar(Value::String(String::new())),
        )]),
    )
    .unwrap();
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("2023-01-28T00:00:00Z".to_string()))
    );
}
