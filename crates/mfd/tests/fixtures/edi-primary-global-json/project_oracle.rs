use std::collections::BTreeMap;

use ir::{IntegerRange, ItemCountRange, NumericRange, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, EdiBoundaryKind, FormatOptions, Graph, IterationOutput, Node, Project,
    Scope, ScopeIteration,
};

const PREVIEW: &str = r#"{"Header":{"Replacement":"PREVIEW"}}"#;
const ROOT_ROWS_ERROR: &str =
    "JSON parser root object presence does not support multiple primary rows";

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
        .nullable()
        .unwrap()
}

fn payload_schema() -> SchemaNode {
    SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::group(
                "Header",
                vec![
                    string("Replacement"),
                    SchemaNode::scalar("Unused", ScalarType::Int)
                        .with_numeric_range(NumericRange::Integer(
                            IntegerRange::new(Some(0), Some(9)).unwrap(),
                        ))
                        .unwrap(),
                ],
            )
            .nullable_container()
            .unwrap(),
            SchemaNode::scalar("Tail", ScalarType::Bool),
        ],
    )
}

fn rows(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Rows", children)
        .repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap()
}

pub fn expected(design: &str) -> Project {
    if design == "optional-root" {
        return optional_root();
    }
    let row = design == "rows";
    let root_presence = design == "root";
    let scalar_only = design == "scalar";
    let source = SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::group(
                "ISA",
                (1..=16)
                    .map(|index| SchemaNode::scalar(format!("E{index:02}"), ScalarType::String))
                    .collect(),
            ),
            SchemaNode::group("NTE", vec![SchemaNode::scalar("Text", ScalarType::String)]),
            SchemaNode::group(
                "Loop",
                vec![SchemaNode::group(
                    "N1",
                    vec![SchemaNode::scalar("Text", ScalarType::String)],
                )],
            )
            .repeating(),
            SchemaNode::group(
                "IEA",
                vec![
                    SchemaNode::scalar("Count", ScalarType::Int),
                    SchemaNode::scalar("Control", ScalarType::String),
                ],
            ),
        ],
    );
    let target = SchemaNode::group(
        "Report",
        if root_presence || scalar_only {
            vec![string("Label")]
        } else {
            let envelope = SchemaNode::group("Envelope", vec![string("Label")]);
            if row {
                vec![rows(vec![envelope])]
            } else {
                vec![envelope]
            }
        },
    );
    let descriptor = serde_json::to_string(&payload_schema()).unwrap();
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::Call {
                function: "substitute_missing".into(),
                args: vec![1, 5],
            },
        ),
        (
            1,
            Node::SourceField {
                path: if row {
                    vec!["N1".into(), "Text".into()]
                } else {
                    vec!["NTE".into(), "Text".into()]
                },
                frame: row.then(|| vec!["Loop".into()]),
            },
        ),
        (
            2,
            Node::RuntimeParameter {
                name: "payload".into(),
                ty: ScalarType::String,
                preview: Some(PREVIEW.into()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String(descriptor.clone()),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String(r#"["Header","Replacement"]"#.into()),
            },
        ),
        (
            5,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![2, 3, 4],
            },
        ),
        (
            6,
            Node::Const {
                value: Value::String(descriptor),
            },
        ),
        (
            7,
            Node::Const {
                value: Value::String(if root_presence { "[]" } else { r#"["Header"]"# }.into()),
            },
        ),
        (
            8,
            Node::Call {
                function: "json_parse_object_exists".into(),
                args: vec![2, 6, 7],
            },
        ),
    ]);
    if scalar_only {
        for id in [6, 7, 8] {
            nodes.remove(&id);
        }
    }
    if root_presence {
        nodes.extend([
            (
                9,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec![],
                    value: vec![],
                    expression: None,
                    arg: None,
                },
            ),
            (
                10,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                11,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![9, 10],
                },
            ),
            (
                12,
                Node::Const {
                    value: Value::String(ROOT_ROWS_ERROR.into()),
                },
            ),
            (13, Node::Raise { message: Some(12) }),
            (
                14,
                Node::If {
                    condition: 11,
                    then: 13,
                    else_: 8,
                },
            ),
        ]);
    }
    let presence = Scope {
        target_field: if root_presence {
            String::new()
        } else {
            "Envelope".into()
        },
        iteration: ScopeIteration::Source(vec![]),
        filter: Some(if root_presence { 14 } else { 8 }),
        iteration_output: if root_presence {
            IterationOutput::First
        } else {
            IterationOutput::MappedSequence
        },
        bindings: vec![Binding {
            target_field: "Label".into(),
            node: 0,
        }],
        ..Default::default()
    };
    let root = if scalar_only {
        Scope {
            bindings: vec![Binding {
                target_field: "Label".into(),
                node: 0,
            }],
            ..Default::default()
        }
    } else if root_presence {
        presence
    } else if row {
        Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Loop".into()]),
                children: vec![presence],
                ..Default::default()
            }],
            ..Default::default()
        }
    } else {
        Scope {
            children: vec![presence],
            ..Default::default()
        }
    };
    Project {
        source,
        target,
        source_path: Some("input.x12".into()),
        target_path: Some("report.json".into()),
        source_options: FormatOptions {
            lenient_segments: true,
            edi_kind: Some(EdiBoundaryKind::X12),
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        graph: Graph { nodes },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

// Whole-Project oracle for the composed required-EDI / optional-parser boundary.
// The complete physical schemas, paths and format options are the authored root
// design above; only the parser input adapter and its resulting node IDs differ.
fn optional_root() -> Project {
    let mut project = expected("root");
    let descriptor = serde_json::to_string(&payload_schema()).unwrap();
    project.graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::Call {
                    function: "substitute_missing".into(),
                    args: vec![1, 6],
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["NTE".into(), "Text".into()],
                    frame: None,
                },
            ),
            (2, Node::Const { value: Value::Null }),
            (
                3,
                Node::RuntimeParameterDefault {
                    name: "payload".into(),
                    ty: ScalarType::String,
                    default: 2,
                    preview: Some(PREVIEW.into()),
                },
            ),
            (
                4,
                Node::Const {
                    value: Value::String(descriptor.clone()),
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::String(r#"["Header","Replacement"]"#.into()),
                },
            ),
            (
                6,
                Node::Call {
                    function: "json_parse_field".into(),
                    args: vec![3, 4, 5],
                },
            ),
            (
                7,
                Node::Const {
                    value: Value::String(descriptor),
                },
            ),
            (
                8,
                Node::Const {
                    value: Value::String("[]".into()),
                },
            ),
            (
                9,
                Node::Call {
                    function: "json_parse_object_exists".into(),
                    args: vec![3, 7, 8],
                },
            ),
            (
                10,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec![],
                    value: vec![],
                    expression: None,
                    arg: None,
                },
            ),
            (
                11,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                12,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![10, 11],
                },
            ),
            (
                13,
                Node::Const {
                    value: Value::String(ROOT_ROWS_ERROR.into()),
                },
            ),
            (14, Node::Raise { message: Some(13) }),
            (
                15,
                Node::If {
                    condition: 12,
                    then: 14,
                    else_: 9,
                },
            ),
        ]),
    };
    project.root = Scope {
        target_field: String::new(),
        iteration: ScopeIteration::Source(vec![]),
        filter: Some(15),
        iteration_output: IterationOutput::First,
        bindings: vec![Binding {
            target_field: "Label".into(),
            node: 0,
        }],
        ..Default::default()
    };
    project.source_options.mfd_optional_parser_input_names =
        BTreeMap::from([(3, "payload".into())]);
    project
}
