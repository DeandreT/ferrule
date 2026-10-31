//! Tuple frames are ordinary groups in the mapping graph. The JSON reader
//! gives an absent frame and an absent position `Scalar(Null)`, which the
//! engine must treat as absent data rather than as a malformed group.

use std::collections::BTreeMap;

use ir::{Instance, ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, Project, Scope, ScopeIteration};

use crate::{run, validate};

fn frame(name: &str, members: Vec<SchemaNode>, rest: Option<SchemaNode>) -> SchemaNode {
    let (children, layout) = match rest {
        Some(rest) => (
            members.into_iter().chain([rest.repeating()]).collect(),
            JsonTupleLayout {
                length: None,
                tail: JsonTupleTail::Open,
            },
        ),
        None => {
            let count = members.len() as u64;
            (
                members,
                JsonTupleLayout {
                    length: ItemCountRange::new(0, Some(count)),
                    tail: JsonTupleTail::Closed,
                },
            )
        }
    };
    SchemaNode::group(name, children)
        .with_json_tuple(layout)
        .expect("canonical test frame")
}

fn point(name: &str) -> SchemaNode {
    frame(
        name,
        vec![
            SchemaNode::scalar("item1", ScalarType::Float),
            SchemaNode::scalar("item2", ScalarType::String),
        ],
        None,
    )
}

fn codes(name: &str) -> SchemaNode {
    frame(
        name,
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
        Some(SchemaNode::scalar("rest", ScalarType::String)),
    )
}

fn path() -> SchemaNode {
    let mut path = point("path").repeating();
    path.json_object_item_nullable = Some(false);
    path
}

fn field(path: &[&str]) -> Node {
    Node::SourceField {
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        frame: None,
    }
}

fn count(collection: &[&str]) -> Node {
    Node::Aggregate {
        function: AggregateOp::Count,
        collection: collection
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect(),
        value: Vec::new(),
        expression: None,
        arg: None,
    }
}

fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn project() -> Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            point("origin"),
            point("label"),
            codes("codes"),
            codes("extra"),
            path(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            point("origin"),
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("tail", ScalarType::Int),
            SchemaNode::scalar("absent_tail", ScalarType::Int),
            SchemaNode::group("extra", vec![SchemaNode::scalar("seen", ScalarType::Int)])
                .repeating(),
            path(),
        ],
    );
    let nodes = BTreeMap::from([
        (0, field(&["origin", "item1"])),
        (1, field(&["origin", "item2"])),
        (2, field(&["label", "item2"])),
        (3, count(&["codes", "rest"])),
        (4, count(&["extra", "rest"])),
        (5, field(&["path", "item1"])),
        (6, field(&["path", "item2"])),
        (
            7,
            Node::Position {
                collection: vec!["extra".into(), "rest".into()],
            },
        ),
    ]);
    Project {
        source,
        target,
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![bind("label", 2), bind("tail", 3), bind("absent_tail", 4)],
            children: vec![
                Scope {
                    target_field: "origin".into(),
                    bindings: vec![bind("item1", 0), bind("item2", 1)],
                    ..Default::default()
                },
                Scope {
                    target_field: "extra".into(),
                    iteration: ScopeIteration::Source(vec!["extra".into(), "rest".into()]),
                    bindings: vec![bind("seen", 7)],
                    ..Default::default()
                },
                Scope {
                    target_field: "path".into(),
                    iteration: ScopeIteration::Source(vec!["path".into()]),
                    bindings: vec![bind("item1", 5), bind("item2", 6)],
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

const NULL: Instance = Instance::Scalar(Value::Null);

/// The source as the JSON reader produces it: `label` and `extra` are
/// absent frames, `codes` lacks position 2, and the second `path` tuple is
/// `[]`.
fn source(origin: Instance) -> Instance {
    group(vec![
        ("origin", origin),
        ("label", NULL),
        (
            "codes",
            group(vec![
                ("item1", text("A")),
                ("item2", NULL),
                ("rest", Instance::Repeated(vec![text("B"), text("C")])),
            ]),
        ),
        ("extra", NULL),
        (
            "path",
            Instance::Repeated(vec![
                group(vec![
                    ("item1", Instance::Scalar(Value::Float(2.0))),
                    ("item2", text("x")),
                ]),
                group(vec![("item1", NULL), ("item2", NULL)]),
            ]),
        ),
    ])
}

#[test]
fn absent_frames_and_positions_are_absent_source_data() {
    let project = project();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let source = source(group(vec![
        ("item1", Instance::Scalar(Value::Float(1.5))),
        ("item2", NULL),
    ]));
    let output = run(&project, &source).expect("the mapping runs");
    assert_eq!(
        output.field("origin"),
        Some(&group(vec![
            ("item1", Instance::Scalar(Value::Float(1.5))),
            ("item2", NULL),
        ]))
    );
    assert_eq!(output.field("label"), Some(&NULL));
    assert_eq!(output.field("tail"), Some(&Instance::Scalar(Value::Int(2))));
    assert_eq!(
        output.field("absent_tail"),
        Some(&Instance::Scalar(Value::Int(0)))
    );
    assert_eq!(output.field("extra"), Some(&Instance::Repeated(Vec::new())));
    assert_eq!(
        output.field("path"),
        Some(&Instance::Repeated(vec![
            group(vec![
                ("item1", Instance::Scalar(Value::Float(2.0))),
                ("item2", text("x"))
            ]),
            group(vec![("item1", NULL), ("item2", NULL)]),
        ]))
    );
}

#[test]
fn explicit_json_null_positions_stay_distinct_from_absence() {
    let mut project = project();
    // `origin/item2` admits null; reading it must not turn `JsonNull` into
    // absence.
    let nullable = frame(
        "origin",
        vec![
            SchemaNode::scalar("item1", ScalarType::Float),
            SchemaNode::scalar("item2", ScalarType::String)
                .nullable()
                .expect("nullable string"),
        ],
        None,
    );
    for schema in [&mut project.source, &mut project.target] {
        let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!("both sides are groups");
        };
        children[0] = nullable.clone();
    }
    let source = source(group(vec![
        ("item1", Instance::Scalar(Value::Float(1.0))),
        ("item2", Instance::Scalar(Value::json_null())),
    ]));
    let output = run(&project, &source).expect("the mapping runs");
    let Some(Instance::Group(origin)) = output.field("origin") else {
        panic!("origin is constructed: {output:?}");
    };
    assert_eq!(
        origin
            .iter()
            .find(|(name, _)| name == "item2")
            .map(|(_, value)| value),
        Some(&Instance::Scalar(Value::json_null()))
    );
}
