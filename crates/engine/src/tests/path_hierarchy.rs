use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Graph, PathHierarchyPlan, Project, Scope, ScopeConstruction};

use crate::{EngineError, run, validate};

fn schema() -> (SchemaNode, SchemaNode) {
    let source = SchemaNode::group(
        "FileList",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    let target = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    );
    (source, target)
}

fn project() -> Project {
    let (source, target) = schema();
    let plan = PathHierarchyPlan::new(
        vec!["File".into()],
        "\\".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    )
    .unwrap();
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
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::PathHierarchy { plan },
            ..Scope::default()
        },
    }
}

fn source(paths: &[&str]) -> Instance {
    Instance::Group(vec![(
        "File".into(),
        Instance::Repeated(
            paths
                .iter()
                .map(|path| Instance::Scalar(Value::String((*path).into())))
                .collect(),
        ),
    )])
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> &'a str {
    let Some(Instance::Scalar(Value::String(value))) = instance.field(field) else {
        panic!("missing string field {field}");
    };
    value
}

#[test]
fn groups_paths_in_first_seen_order_and_preserves_duplicate_files() {
    let project = project();
    assert!(validate(&project).is_empty());
    let output = run(
        &project,
        &source(&[
            "Root\\a.txt",
            "Root\\src\\main.rs",
            "Root\\a.txt",
            "Root\\src\\lib.rs",
        ]),
    )
    .unwrap();

    assert_eq!(scalar(&output, "name"), "Root");
    let files = output
        .field("file")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(scalar(&files[0], "name"), "a.txt");
    assert_eq!(scalar(&files[1], "name"), "a.txt");
    let directories = output
        .field("directory")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(directories.len(), 1);
    assert_eq!(scalar(&directories[0], "name"), "src");
    let source_files = directories[0]
        .field("file")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(scalar(&source_files[0], "name"), "main.rs");
    assert_eq!(scalar(&source_files[1], "name"), "lib.rs");
}

#[test]
fn rejects_multiple_public_root_directories() {
    let error = run(&project(), &source(&["One\\a", "Two\\b"])).unwrap_err();
    assert_eq!(error, EngineError::PathHierarchyRootCount { count: 2 });
}

#[test]
fn validation_rejects_non_repeating_inputs_and_non_recursive_targets() {
    let mut invalid_source = project();
    invalid_source.source = SchemaNode::group(
        "FileList",
        vec![SchemaNode::scalar("File", ScalarType::String)],
    );
    assert!(
        validate(&invalid_source)
            .iter()
            .any(|issue| { issue.message.contains("must be a repeating scalar") })
    );

    let mut invalid_target = project();
    invalid_target.target = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::group("directory", Vec::new()).repeating(),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    );
    assert!(
        validate(&invalid_target)
            .iter()
            .any(|issue| { issue.message.contains("must recursively reference") })
    );
}
