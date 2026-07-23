use super::*;
use ir::Instance;
use mapping::{
    Binding as MappingBinding, Graph, Node, Project, RecursiveFilterPlan, Scope, ScopeConstruction,
    ScopeIteration,
};

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect())
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group(
                "file",
                vec![
                    SchemaNode::scalar("name", ScalarType::String),
                    SchemaNode::scalar("expected", ScalarType::Int),
                    SchemaNode::scalar("child_expected", ScalarType::Int),
                ],
            )
            .repeating(),
            SchemaNode::recursive_group("directory", "Directory").repeating(),
        ],
    )
}

fn document_schema() -> SchemaNode {
    let mut directories = directory_schema();
    directories.name = "Directories".into();
    directories.repeating = true;
    SchemaNode::group(
        "Document",
        vec![
            SchemaNode::scalar("global_suffix", ScalarType::String),
            directories,
        ],
    )
}

fn project() -> Project {
    let Some(plan) = RecursiveFilterPlan::new("directory".into(), "file".into(), 11) else {
        panic!("valid recursive-filter plan");
    };
    let schema = document_schema();
    Project {
        source: schema.clone(),
        target: schema,
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
                    1,
                    Node::SourceField {
                        path: vec!["name".into()],
                        frame: Some(vec!["file".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["global_suffix".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "contains".into(),
                        args: vec![1, 2],
                    },
                ),
                (
                    4,
                    Node::Position {
                        collection: vec!["file".into()],
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        path: vec!["expected".into()],
                        frame: None,
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![4, 5],
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "and".into(),
                        args: vec![3, 6],
                    },
                ),
                (
                    8,
                    Node::Position {
                        collection: vec!["directory".into()],
                    },
                ),
                (
                    9,
                    Node::SourceField {
                        path: vec!["child_expected".into()],
                        frame: Some(vec!["file".into()]),
                    },
                ),
                (
                    10,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![8, 9],
                    },
                ),
                (
                    11,
                    Node::Call {
                        function: "and".into(),
                        args: vec![7, 10],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![MappingBinding {
                target_field: "global_suffix".into(),
                node: 2,
            }],
            children: vec![Scope {
                target_field: "Directories".into(),
                iteration: ScopeIteration::Source(vec!["Directories".into()]),
                construction: ScopeConstruction::RecursiveFilter { plan },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn file(name: &str, expected: i64, child_expected: i64) -> Instance {
    group([
        field("name", scalar(Value::String(name.into()))),
        field("expected", scalar(Value::Int(expected))),
        field("child_expected", scalar(Value::Int(child_expected))),
    ])
}

fn directory(name: &str, files: Vec<Instance>, children: Vec<Instance>) -> Instance {
    group([
        field("name", scalar(Value::String(name.into()))),
        field("file", repeated(files)),
        field("directory", repeated(children)),
    ])
}

fn source() -> Instance {
    group([
        field("global_suffix", scalar(Value::String(".keep".into()))),
        field(
            "Directories",
            repeated([directory(
                "root",
                vec![file("drop.txt", 1, 1), file("root.keep", 2, 1)],
                vec![
                    directory("empty", vec![file("drop.md", 1, 1)], Vec::new()),
                    directory(
                        "nested",
                        vec![file("nested.keep", 1, 2), file("drop.log", 2, 2)],
                        Vec::new(),
                    ),
                ],
            )]),
        ),
    ])
}

fn expected() -> Instance {
    group([
        field("global_suffix", scalar(Value::String(".keep".into()))),
        field(
            "Directories",
            repeated([directory(
                "root",
                vec![file("root.keep", 2, 1)],
                vec![
                    directory("empty", Vec::new(), Vec::new()),
                    directory("nested", vec![file("nested.keep", 1, 2)], Vec::new()),
                ],
            )]),
        ),
    ])
}

#[test]
fn generated_package_matches_engine_recursive_filter_and_typed_errors() {
    let project = project();
    let source = source();
    assert_eq!(engine::run(&project, &source), Ok(expected()));
    let Ok(program) = codegen::lower(&project) else {
        panic!("recursive-filter project lowers");
    };
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap_or_else(|error| panic!("runtime path exists: {error}"));
    let output = TempDir::new("rust_recursive_filter_codegen");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "recursive-filter-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .unwrap_or_else(|error| panic!("recursive-filter program emits: {error}"));
    write_artifacts(output.path(), &artifacts);
    fs::write(output.path().join("src/main.rs"), HARNESS)
        .unwrap_or_else(|error| panic!("write generated recursive-filter harness: {error}"));

    let status = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .status()
        .unwrap_or_else(|error| panic!("run generated recursive-filter package: {error}"));
    assert!(status.success());
}

const HARNESS: &str = r#"use codegen_runtime::{
    Instance, RuntimeError, Value, field, group, repeated, scalar, string,
};

fn file(name: &str, expected: i64, child_expected: i64) -> Instance {
    group([
        field("name", scalar(string(name))),
        field("expected", scalar(Value::Int(expected))),
        field("child_expected", scalar(Value::Int(child_expected))),
    ])
}

fn directory(name: &str, files: Vec<Instance>, children: Vec<Instance>) -> Instance {
    group([
        field("name", scalar(string(name))),
        field("file", repeated(files)),
        field("directory", repeated(children)),
    ])
}

fn source() -> Instance {
    group([
        field("global_suffix", scalar(string(".keep"))),
        field(
            "Directories",
            repeated([directory(
                "root",
                vec![file("drop.txt", 1, 1), file("root.keep", 2, 1)],
                vec![
                    directory("empty", vec![file("drop.md", 1, 1)], Vec::new()),
                    directory(
                        "nested",
                        vec![file("nested.keep", 1, 2), file("drop.log", 2, 2)],
                        Vec::new(),
                    ),
                ],
            )]),
        ),
    ])
}

fn expected() -> Instance {
    group([
        field("global_suffix", scalar(string(".keep"))),
        field(
            "Directories",
            repeated([directory(
                "root",
                vec![file("root.keep", 2, 1)],
                vec![
                    directory("empty", Vec::new(), Vec::new()),
                    directory(
                        "nested",
                        vec![file("nested.keep", 1, 2)],
                        Vec::new(),
                    ),
                ],
            )]),
        ),
    ])
}

fn document(directory: Instance) -> Instance {
    group([
        field("global_suffix", scalar(string(".keep"))),
        field("Directories", repeated([directory])),
    ])
}

fn deep(groups: usize) -> Instance {
    let mut value = directory("leaf", Vec::new(), Vec::new());
    for index in 1..groups {
        value = directory(&format!("level-{index}"), Vec::new(), vec![value]);
    }
    document(value)
}

fn main() {
    assert_eq!(recursive_filter_map::execute(&source()), Ok(expected()));

    let sparse_directory = group([field("name", scalar(string("sparse")))]);
    assert_eq!(
        recursive_filter_map::execute(&document(sparse_directory.clone())),
        Ok(group([
            field("global_suffix", scalar(string(".keep"))),
            field("Directories", repeated([sparse_directory])),
        ])),
    );

    assert_eq!(
        recursive_filter_map::execute(&document(scalar(string("not a group")))),
        Err(RuntimeError::RecursiveFilterRequiresGroup { found: "scalar" }),
    );

    assert!(recursive_filter_map::execute(&deep(256)).is_ok());
    assert_eq!(
        recursive_filter_map::execute(&deep(257)),
        Err(RuntimeError::RecursiveFilterDepth { limit: 256 }),
    );
}
"#;
