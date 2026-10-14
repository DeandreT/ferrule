// Independent complete Project oracle, authored without importer output.
use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use std::collections::BTreeMap;

pub fn expected(ty: ScalarType, literal: Option<Value>) -> Project {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Key", ty),
            SchemaNode::scalar("Name", ScalarType::String),
        ],
    )
    .repeating()
    .with_required_fields(vec!["Key".into(), "Name".into()])
    .unwrap()
    .with_item_count_range(ItemCountRange::new(0, Some(8)).unwrap())
    .unwrap();
    let source = SchemaNode::group("Feed", vec![rows])
        .with_required_fields(vec!["Rows".into()])
        .unwrap();
    let key = match literal {
        Some(value) => Node::Const { value },
        None => Node::SourceField {
            path: vec!["Key".into()],
            frame: Some(vec!["Rows".into()]),
        },
    };
    let mut nodes = BTreeMap::from([(0, key)]);
    let adapted = ty != ScalarType::String;
    let group_key = if adapted {
        nodes.insert(
            1,
            Node::Call {
                function: "string".into(),
                args: vec![0],
            },
        );
        1
    } else {
        0
    };
    let name = if adapted { 2 } else { 1 };
    nodes.insert(
        name,
        Node::SourceField {
            path: vec!["Name".into()],
            frame: Some(vec!["Rows".into()]),
        },
    );
    Project {
        source,
        target: SchemaNode::group(
            "Summary",
            vec![
                SchemaNode::group(
                    "Group",
                    vec![
                        SchemaNode::scalar("Key", ScalarType::String),
                        SchemaNode::group(
                            "Member",
                            vec![SchemaNode::scalar("Name", ScalarType::String)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("source.json".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                group_by: Some(group_key),
                bindings: vec![Binding {
                    target_field: "Key".into(),
                    node: group_key,
                }],
                children: vec![Scope {
                    target_field: "Member".into(),
                    iteration: ScopeIteration::Source(vec![]),
                    bindings: vec![Binding {
                        target_field: "Name".into(),
                        node: name,
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

pub fn legacy_xml(substring: bool) -> Project {
    let mut project = expected(ScalarType::String, None);
    project.source = SchemaNode::group(
        "Feed",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Name", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.source_path = Some("source.xml".into());
    project.source_options = FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    if substring {
        project.graph.nodes = BTreeMap::from([
            (
                0,
                Node::Call {
                    function: "substring_before".into(),
                    args: vec![1, 2],
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Key".into()],
                    frame: Some(vec!["Rows".into()]),
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String("~".into()),
                },
            ),
            (
                4,
                Node::SourceField {
                    path: vec!["Name".into()],
                    frame: Some(vec!["Rows".into()]),
                },
            ),
        ]);
        project.root.children[0].children[0].bindings[0].node = 4;
    }
    project
}

pub fn legacy_csv() -> Project {
    Project {
        source: SchemaNode::group("Feed", vec![SchemaNode::scalar("Key", ScalarType::String)]),
        target: SchemaNode::group(
            "Summary",
            vec![SchemaNode::scalar("Key", ScalarType::String)],
        ),
        source_path: Some("source.csv".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
            delimiter: Some(','),
            has_header_row: Some(true),
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Key".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Key".into(),
                node: 0,
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

pub fn legacy_filtered() -> Project {
    let mut project = legacy_xml(false);
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::Call {
                function: "equal".into(),
                args: vec![1, 2],
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Key".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("oak".into()),
            },
        ),
        (
            3,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
    ]);
    project.root.children[0].group_by = Some(1);
    project.root.children[0].filter = Some(0);
    project.root.children[0].bindings[0].node = 1;
    project.root.children[0].children[0].bindings[0].node = 3;
    project
}

pub fn legacy_json(ty: ScalarType, key_required: bool) -> Project {
    let mut project = expected(ty, None);
    if !key_required {
        let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
            unreachable!()
        };
        assert!(children[0].set_required_fields(vec!["Name".into()]));
    }
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Key".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
    ]);
    project.root.children[0].group_by = Some(0);
    project.root.children[0].bindings[0].node = 0;
    project.root.children[0].children[0].bindings[0].node = 1;
    project
}
