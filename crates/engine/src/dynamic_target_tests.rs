use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{DynamicBinding, DynamicChild, Graph, Node, Project, Scope};

use super::{EngineError, run, validate};

fn open_target(fixed: Vec<SchemaNode>) -> SchemaNode {
    let person = SchemaNode::group("person", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
        .unwrap();
    SchemaNode::group("root", fixed)
        .with_dynamic_fields(person.repeating())
        .unwrap()
}

fn project(target: SchemaNode) -> Project {
    let source = SchemaNode::group(
        "Department",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Person",
                vec![
                    SchemaNode::scalar("First", ScalarType::String),
                    SchemaNode::scalar("Title", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Name".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("Name".into()),
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
                3,
                Node::Const {
                    value: Value::String("Details".into()),
                },
            ),
            (
                4,
                Node::SourceField {
                    path: vec!["Title".into()],
                    frame: None,
                },
            ),
        ]),
    };
    let people = Scope {
        source: Some(vec!["Person".into()]),
        dynamic_bindings: vec![
            DynamicBinding { key: 1, value: 2 },
            DynamicBinding { key: 3, value: 4 },
        ],
        ..Scope::default()
    };
    let root = Scope {
        source: Some(Vec::new()),
        dynamic_children: vec![DynamicChild {
            key: 0,
            scope: people,
        }],
        merge_dynamic_fields: true,
        ..Scope::default()
    };
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root,
    }
}

fn department(name: &str, people: &[(&str, &str)]) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(name.into()))),
        (
            "Person".into(),
            Instance::Repeated(
                people
                    .iter()
                    .map(|(first, title)| {
                        Instance::Group(vec![
                            (
                                "First".into(),
                                Instance::Scalar(Value::String((*first).into())),
                            ),
                            (
                                "Title".into(),
                                Instance::Scalar(Value::String((*title).into())),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

#[test]
fn merges_ordered_dynamic_object_fragments_with_nested_arrays() {
    let project = project(open_target(Vec::new()));
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let source = Instance::Repeated(vec![
        department("Engineering", &[("Ada", "Manager"), ("Linus", "Engineer")]),
        department("Sales", &[("Grace", "Director")]),
    ]);

    let output = run(&project, &source).unwrap();
    let Instance::Group(fields) = output else {
        panic!("dynamic target should be an object")
    };
    assert_eq!(
        fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["Engineering", "Sales"]
    );
    let engineering = fields[0].1.as_repeated().unwrap();
    assert_eq!(engineering.len(), 2);
    assert_eq!(
        engineering[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
}

#[test]
fn rejects_duplicate_non_string_and_fixed_colliding_dynamic_keys() {
    let duplicate = Instance::Repeated(vec![
        department("Engineering", &[("Ada", "Manager")]),
        department("Engineering", &[("Linus", "Engineer")]),
    ]);
    assert!(matches!(
        run(&project(open_target(Vec::new())), &duplicate),
        Err(EngineError::DuplicateDynamicProperty(ref name)) if name == "Engineering"
    ));

    let mut non_string = project(open_target(Vec::new()));
    non_string.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::Int(1),
        },
    );
    assert!(matches!(
        run(&non_string, &Instance::Repeated(vec![department("A", &[])])),
        Err(EngineError::DynamicPropertyName {
            node: 0,
            found: "int"
        })
    ));

    let fixed = SchemaNode::scalar("Engineering", ScalarType::String);
    assert!(matches!(
        run(
            &project(open_target(vec![fixed])),
            &Instance::Repeated(vec![department("Engineering", &[])])
        ),
        Err(EngineError::DuplicateDynamicProperty(ref name)) if name == "Engineering"
    ));
}

#[test]
fn validation_rejects_invalid_dynamic_scope_combinations() {
    let mut project = project(SchemaNode::group("closed", Vec::new()));
    project.root.source = None;
    project.root.dynamic_children[0].key = 88;
    project.root.bindings.push(mapping::Binding {
        target_field: "fixed".into(),
        node: 99,
    });
    let messages = validate(&project)
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    for expected in [
        "dynamic object merge requires an iterated source",
        "dynamic object merge accepts only computed child-scope properties",
        "computed target properties require an open target group schema",
        "dynamic child key references missing node",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{messages:#?}"
        );
    }
}

#[test]
fn validation_enforces_dynamic_schema_shape_and_cardinality() {
    let mut empty_merge = project(open_target(Vec::new()));
    empty_merge.root.dynamic_children.clear();
    let empty_messages = validate(&empty_merge)
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    assert!(empty_messages.iter().any(|message| {
        message.contains("dynamic object merge requires at least one computed child-scope")
    }));

    let repeating_scalar = SchemaNode::group("person", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String).repeating())
        .unwrap();
    let repeating_scalar_target = SchemaNode::group("root", Vec::new())
        .with_dynamic_fields(repeating_scalar.repeating())
        .unwrap();
    let binding_messages = validate(&project(repeating_scalar_target))
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    assert!(binding_messages.iter().any(|message| {
        message.contains("computed scalar binding requires a non-repeating scalar")
    }));

    let scalar_child_target = SchemaNode::group("root", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String).repeating())
        .unwrap();
    let shape_messages = validate(&project(scalar_child_target))
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    assert!(shape_messages.iter().any(|message| {
        message.contains("computed child scope requires a group dynamic field schema")
    }));

    let non_repeating_person = SchemaNode::group("person", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
        .unwrap();
    let cardinality_target = SchemaNode::group("root", Vec::new())
        .with_dynamic_fields(non_repeating_person)
        .unwrap();
    let cardinality_messages = validate(&project(cardinality_target))
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    assert!(
        cardinality_messages
            .iter()
            .any(|message| { message.contains("computed child scope cardinality does not match") })
    );
}
