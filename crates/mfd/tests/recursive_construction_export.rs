use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration, SequenceExpr,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_recursive_construction_export_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn recursive_collect_round_trips_and_executes_identically() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let project = project();
    let source = directory(
        "root",
        &["top.txt"],
        vec![directory("child", &["nested.txt"], Vec::new())],
    );
    let expected = engine::run(&project, &source)?;
    let design = dir.0.join("recursive-collect.mfd");

    assert!(mfd::export(&project, &design)?.is_empty());
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(matches!(
        imported
            .project
            .root
            .children
            .first()
            .map(|scope| &scope.construction),
        Some(ScopeConstruction::Scalar { .. })
    ));
    assert_eq!(engine::run(&imported.project, &source)?, expected);
    Ok(())
}

#[test]
fn recursive_collect_item_at_round_trips_and_executes_identically()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let mut project = project();
    let sequence = project
        .root
        .children
        .first()
        .and_then(Scope::sequence)
        .cloned()
        .ok_or("missing recursive sequence")?;
    project.target = SchemaNode::group(
        "FileList",
        vec![SchemaNode::scalar("File", ScalarType::String)],
    );
    project.graph.nodes.extend([
        (
            3,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (4, Node::SequenceItemAt { sequence, index: 3 }),
    ]);
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "File".into(),
            node: 4,
        }],
        ..Scope::default()
    };
    assert!(engine::validate(&project).is_empty());

    let source = directory(
        "root",
        &["top.txt"],
        vec![directory("child", &["nested.txt"], Vec::new())],
    );
    let expected = engine::run(&project, &source)?;
    let design = dir.0.join("recursive-item-at.mfd");
    assert!(mfd::export(&project, &design)?.is_empty());
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            Node::SequenceItemAt {
                sequence: SequenceExpr::RecursiveCollect { .. },
                ..
            }
        )
    }));
    assert_eq!(engine::run(&imported.project, &source)?, expected);
    Ok(())
}

#[test]
fn malformed_recursive_metadata_warns_and_is_not_applied() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let design = dir.0.join("recursive-collect.mfd");
    assert!(mfd::export(&project(), &design)?.is_empty());
    let malformed = std::fs::read_to_string(&design)?.replacen(
        "<ferrule-recursive version=\"1\"",
        "<ferrule-recursive version=\"2\"",
        1,
    );
    std::fs::write(&design, malformed)?;

    let imported = mfd::import(&design)?;
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("requires ferrule recursive metadata version 1"));
    assert!(imported.project.root.children.is_empty());
    Ok(())
}

fn project() -> Project {
    let source = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
        ],
    );
    let target = SchemaNode::group(
        "FileList",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String(String::new()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("\\".into()),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "File".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::RecursiveCollect {
                    collection: Vec::new(),
                    children: vec!["directory".into()],
                    descent_value: vec!["name".into()],
                    values: vec!["file".into()],
                    value: vec!["name".into()],
                    prefix: 0,
                    separator: 1,
                    item: 2,
                }),
                construction: ScopeConstruction::Scalar { value: 2 },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn directory(name: &str, files: &[&str], directories: Vec<Instance>) -> Instance {
    Instance::Group(vec![
        (
            "name".into(),
            Instance::Scalar(Value::String(name.to_string())),
        ),
        (
            "file".into(),
            Instance::Repeated(
                files
                    .iter()
                    .map(|name| {
                        Instance::Group(vec![(
                            "name".into(),
                            Instance::Scalar(Value::String((*name).to_string())),
                        )])
                    })
                    .collect(),
            ),
        ),
        ("directory".into(), Instance::Repeated(directories)),
    ])
}
