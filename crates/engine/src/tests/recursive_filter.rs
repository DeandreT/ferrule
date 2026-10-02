use std::collections::BTreeMap;

use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_MIXED_CONTENT_FIELD,
    XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
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

#[test]
fn recursive_filter_prunes_ordered_xml_values_and_updates_nested_children() {
    let nested = with_ordered_content(
        directory("nested", &["nested.xml", "drop.md"], Vec::new()),
        &["file", "file"],
    );
    let source = with_ordered_content(
        directory("root", &["drop.txt", "root.xml"], vec![nested]),
        &["file", "directory", "file"],
    );
    let expected_nested =
        with_ordered_content(directory("nested", &["nested.xml"], Vec::new()), &["file"]);
    let expected = with_ordered_content(
        directory("root", &["root.xml"], vec![expected_nested]),
        &["directory", "file"],
    );

    assert_eq!(run(&project(), &source), Ok(expected.clone()));
    let mut stale = source;
    let Instance::Group(fields) = &mut stale else {
        unreachable!("ordered source is a group");
    };
    let Some((_, Instance::Repeated(ordered))) = fields
        .iter_mut()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    else {
        unreachable!("ordered source has XML metadata");
    };
    ordered.push(ordered[0].clone());
    assert_eq!(run(&project(), &stale), Ok(expected));
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
    Instance::Group(
        (vec![
            ("name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "file".into(),
                Instance::Repeated(
                    files
                        .iter()
                        .map(|name| {
                            Instance::Group(
                                (vec![(
                                    "name".into(),
                                    Instance::Scalar(Value::String((*name).into())),
                                )])
                                .into(),
                            )
                        })
                        .collect(),
                ),
            ),
            ("directory".into(), Instance::Repeated(children)),
        ])
        .into(),
    )
}

fn with_ordered_content(mut directory: Instance, order: &[&str]) -> Instance {
    let Instance::Group(fields) = &mut directory else {
        unreachable!("directory helper produces a group");
    };
    let mut file_index = 0;
    let mut child_index = 0;
    let ordered = order
        .iter()
        .map(|name| {
            let index = if *name == "file" {
                let index = file_index;
                file_index += 1;
                index
            } else {
                let index = child_index;
                child_index += 1;
                index
            };
            let value = fields
                .iter()
                .find(|(field, _)| field == name)
                .and_then(|(_, value)| value.as_repeated())
                .and_then(|values| values.get(index))
                .expect("ordered XML item has a visible child")
                .clone();
            Instance::Group(
                (vec![
                    (
                        XML_NODE_NAME_FIELD.into(),
                        Instance::Scalar(Value::String((*name).into())),
                    ),
                    (
                        XML_TEXT_FIELD.into(),
                        Instance::Scalar(Value::String(String::new())),
                    ),
                    (XML_MIXED_CONTENT_VALUE_FIELD.into(), value),
                ])
                .into(),
            )
        })
        .collect();
    fields.push((XML_MIXED_CONTENT_FIELD.into(), Instance::Repeated(ordered)));
    directory
}
