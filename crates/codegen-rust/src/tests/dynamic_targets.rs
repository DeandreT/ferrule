use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DynamicBinding, DynamicChild, Graph, Node, Project, Scope, ScopeIteration};

use crate::{Options, RuntimeDependency, emit};

#[test]
fn generated_mapping_matches_ordered_dynamic_target_semantics() {
    let project = project();
    let source = source();
    let interpreted = engine::run(&project, &source).expect("interpreter constructs open object");
    let program = codegen::lower(&project).expect("computed target properties lower");
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path is canonical");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "dynamic-targets-generated".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .expect("dynamic target project emits");
    let directory = TempDir::new("dynamic_targets_rust");
    write_artifacts(directory.path(), &artifacts);
    fs::write(directory.path().join("src/main.rs"), HARNESS).expect("harness is written");

    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(directory.path())
        .env("CARGO_TARGET_DIR", directory.path().join("target"))
        .output()
        .expect("generated Rust project starts");
    assert!(
        run.status.success(),
        "generated Rust project failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated dynamic targets passed"
    );
    assert_eq!(
        interpreted,
        Instance::Group(vec![
            (
                "Engineering".into(),
                Instance::Repeated(vec![
                    person("Ada", "Manager", 1),
                    person("Linus", "Engineer", 2),
                ]),
            ),
            (
                "Sales".into(),
                Instance::Repeated(vec![person("Grace", "Director", 3)]),
            ),
        ])
    );
}

fn project() -> Project {
    let person_target = SchemaNode::group(
        "person",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Details", ScalarType::String),
        ],
    )
    .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::Int))
    .unwrap();
    let target = SchemaNode::group(
        "root",
        vec![SchemaNode::scalar("Reserved", ScalarType::String)],
    )
    .with_dynamic_fields(person_target.repeating())
    .unwrap();
    Project {
        source: SchemaNode::group(
            "Department",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Person",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("Title", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Float),
                    ],
                )
                .repeating(),
            ],
        )
        .repeating(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["First".into()],
                        frame: None,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Title".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String("Rank".into()),
                    },
                ),
                (
                    6,
                    Node::SourceField {
                        path: vec!["Rank".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            dynamic_children: vec![DynamicChild {
                key: 0,
                scope: Scope {
                    iteration: ScopeIteration::Source(vec!["Person".into()]),
                    bindings: vec![
                        Binding {
                            target_field: "Name".into(),
                            node: 2,
                        },
                        Binding {
                            target_field: "Details".into(),
                            node: 4,
                        },
                    ],
                    dynamic_bindings: vec![DynamicBinding { key: 5, value: 6 }],
                    ..Scope::default()
                },
            }],
            merge_dynamic_fields: true,
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    Instance::Repeated(vec![
        department(
            "Engineering",
            &[("Ada", "Manager", 1.0), ("Linus", "Engineer", 2.0)],
        ),
        department("Sales", &[("Grace", "Director", 3.0)]),
    ])
}

fn department(name: &str, people: &[(&str, &str, f64)]) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(name.into()))),
        (
            "Person".into(),
            Instance::Repeated(
                people
                    .iter()
                    .map(|(first, title, rank)| source_person(first, title, *rank))
                    .collect(),
            ),
        ),
    ])
}

fn source_person(first: &str, title: &str, rank: f64) -> Instance {
    Instance::Group(vec![
        (
            "First".into(),
            Instance::Scalar(Value::String(first.into())),
        ),
        (
            "Title".into(),
            Instance::Scalar(Value::String(title.into())),
        ),
        ("Rank".into(), Instance::Scalar(Value::Float(rank))),
    ])
}

fn person(first: &str, title: &str, rank: i64) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(first.into()))),
        (
            "Details".into(),
            Instance::Scalar(Value::String(title.into())),
        ),
        ("Rank".into(), Instance::Scalar(Value::Int(rank))),
    ])
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("artifact parent is created");
        }
        fs::write(path, &file.contents).expect("artifact is written");
    }
}

const HARNESS: &str = r#"
use codegen_runtime::{field, group, repeated, scalar, Instance, RuntimeError, Value};

fn person(first: &str, title: &str, rank: f64) -> Instance {
    group([
        field("First", scalar(Value::String(first.into()))),
        field("Title", scalar(Value::String(title.into()))),
        field("Rank", scalar(Value::Float(rank))),
    ])
}

fn department(name: Value, people: &[(&str, &str, f64)]) -> Instance {
    group([
        field("Name", scalar(name)),
        field(
            "Person",
            repeated(
                people
                    .iter()
                    .map(|(first, title, rank)| person(first, title, *rank)),
            ),
        ),
    ])
}

fn main() {
    let source = repeated([
        department(
            Value::String("Engineering".into()),
            &[("Ada", "Manager", 1.0), ("Linus", "Engineer", 2.0)],
        ),
        department(
            Value::String("Sales".into()),
            &[("Grace", "Director", 3.0)],
        ),
    ]);
    let output = dynamic_targets_generated::execute(&source).expect("mapping executes");
    let Instance::Group(fields) = output else {
        panic!("expected merged object");
    };
    assert_eq!(
        fields.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(),
        ["Engineering", "Sales"]
    );
    let Instance::Repeated(engineering) = &fields[0].1 else {
        panic!("department property is a repeated group");
    };
    let Instance::Group(first_person) = &engineering[0] else {
        panic!("person is a group");
    };
    assert!(matches!(
        first_person
            .iter()
            .find(|(name, _)| name == "Rank")
            .map(|(_, value)| value),
        Some(Instance::Scalar(Value::Int(1)))
    ));

    let duplicate = repeated([
        department(Value::String("Same".into()), &[]),
        department(Value::String("Same".into()), &[]),
    ]);
    assert_eq!(
        dynamic_targets_generated::execute(&duplicate),
        Err(RuntimeError::DuplicateDynamicProperty("Same".into()))
    );

    let non_string = repeated([department(Value::Int(1), &[])]);
    assert_eq!(
        dynamic_targets_generated::execute(&non_string),
        Err(RuntimeError::DynamicPropertyName {
            node: 0,
            found: "int",
        })
    );

    let fixed_collision =
        repeated([department(Value::String("Reserved".into()), &[])]);
    assert_eq!(
        dynamic_targets_generated::execute(&fixed_collision),
        Err(RuntimeError::DuplicateDynamicProperty("Reserved".into()))
    );
    println!("generated dynamic targets passed");
}
"#;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let path =
            std::env::temp_dir().join(format!("ferrule_{tag}_{}_{}", std::process::id(), nonce));
        fs::create_dir_all(&path).expect("temporary directory is created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
