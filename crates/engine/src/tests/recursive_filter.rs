use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Graph, Node, Project, RecursiveFilterPlan, Scope, ScopeConstruction};

use crate::{EngineError, run, validate};

#[test]
fn recursive_filter_preserves_shape_and_filters_items_at_every_depth() {
    let project = project();
    let source = directory(
        "root",
        &["root.xml", "notes.txt"],
        vec![directory(
            "nested",
            &["nested.xml", "readme.md"],
            Vec::new(),
        )],
    );
    let expected = directory(
        "root",
        &["root.xml"],
        vec![directory("nested", &["nested.xml"], Vec::new())],
    );

    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(run(&project, &source), Ok(expected));
}

#[test]
fn recursive_filter_has_a_typed_depth_limit() {
    let project = project();
    let mut source = directory("leaf", &[], Vec::new());
    for index in 0..256 {
        source = directory(&format!("level-{index}"), &[], vec![source]);
    }

    assert_eq!(
        run(&project, &source),
        Err(EngineError::RecursiveFilterDepth { limit: 256 })
    );
}

fn project() -> Project {
    let schema = directory_schema();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["name".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String(".xml".into()),
                },
            ),
            (
                2,
                Node::Call {
                    function: "contains".into(),
                    args: vec![0, 1],
                },
            ),
        ]),
    };
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
        user_functions: Default::default(),
        graph,
        root: Scope {
            construction: ScopeConstruction::RecursiveFilter {
                plan: RecursiveFilterPlan::new("directory".into(), "file".into(), 2).unwrap(),
            },
            ..Scope::default()
        },
    }
}

fn directory_schema() -> SchemaNode {
    let mut files = SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)]);
    files.repeating = true;
    let mut children = SchemaNode::recursive_group("directory", "Directory");
    children.repeating = true;
    SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            files,
            children,
        ],
    )
}

fn directory(name: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    Instance::Group(vec![
        ("name".into(), Instance::Scalar(Value::String(name.into()))),
        (
            "file".into(),
            Instance::Repeated(
                files
                    .iter()
                    .map(|name| {
                        Instance::Group(vec![(
                            "name".into(),
                            Instance::Scalar(Value::String((*name).into())),
                        )])
                    })
                    .collect(),
            ),
        ),
        ("directory".into(), Instance::Repeated(children)),
    ])
}
