use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_repeated_scalar_binding_{}",
            std::process::id()
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

fn source_branch(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Street", ScalarType::String),
            SchemaNode::scalar("City", ScalarType::String),
        ],
    )
    .repeating()
}

fn segment(collection: &str, street: u32, city: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec![collection.into()]),
        children: vec![Scope {
            target_field: "Address".into(),
            bindings: vec![
                Binding {
                    target_field: "line".into(),
                    node: street,
                },
                Binding {
                    target_field: "line".into(),
                    node: city,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", vec![source_branch("A"), source_branch("B")]),
        target: SchemaNode::group(
            "Contacts",
            vec![
                SchemaNode::group(
                    "Contact",
                    vec![SchemaNode::group(
                        "Address",
                        vec![SchemaNode::scalar("line", ScalarType::String).repeating()],
                    )],
                )
                .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Street".into()],
                        frame: Some(vec!["A".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["City".into()],
                        frame: Some(vec!["A".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Street".into()],
                        frame: Some(vec!["B".into()]),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["City".into()],
                        frame: Some(vec!["B".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Contact".into(),
                iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                    segment("A", 0, 1),
                    vec![segment("B", 2, 3)],
                )),
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn row(street: &str, city: &str) -> Instance {
    Instance::Group(vec![
        (
            "Street".into(),
            Instance::Scalar(Value::String(street.into())),
        ),
        ("City".into(), Instance::Scalar(Value::String(city.into()))),
    ])
}

#[test]
fn duplicate_repeating_scalar_bindings_keep_order_in_each_concatenated_branch() {
    let directory = TempDir::new();
    let design = directory.0.join("mapping.mfd");
    let project = project();
    let source = Instance::Group(vec![
        ("A".into(), Instance::Repeated(vec![row("one", "two")])),
        ("B".into(), Instance::Repeated(vec![row("three", "four")])),
    ]);

    assert!(mfd::export(&project, &design).unwrap().is_empty());
    let xml = std::fs::read_to_string(&design).unwrap();
    assert_eq!(xml.matches("name=\"line\"").count(), 4);
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        engine::run(&project, &source).unwrap(),
        engine::run(&imported.project, &source).unwrap()
    );
}
