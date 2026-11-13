//! Independently authored acquisition/composition fixture; no saved-design import.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DocumentProject, FormatOptions, Graph, Node, PrimarySourcePlan, Project, RuntimeValue,
    Scope, ScopeIteration, TabularBoundaryKind,
};
use std::collections::BTreeMap;

pub const CLOCK: &str = "2032-04-05T06:07:08Z";
pub const INPUT_NODE: u32 = 91;
pub const ENDING_NODE: u32 = 38;
pub const PATH_NODE: u32 = 76;
pub const ROWS: &str = "Entries";
pub const LITERAL: &str = " literal-*.csv ";

pub fn records() -> BTreeMap<String, String> {
    include_str!("cases.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (key, value) = line.split_once('\t').expect("authored fixture separator");
            (
                key.into(),
                value
                    .replace("\\uFEFF", "\u{feff}")
                    .replace("\\n", "\n")
                    .replace("\\r", "\r"),
            )
        })
        .collect()
}

fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn constant(text: &str) -> Node {
    Node::Const {
        value: Value::String(text.into()),
    }
}
fn field(name: &str) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame: None,
    }
}
fn call(name: &str, args: &[u32]) -> Node {
    Node::Call {
        function: name.into(),
        args: args.to_vec(),
    }
}
fn binding(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}

pub fn document(literal: bool) -> DocumentProject {
    let mut source_options = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        ..Default::default()
    };
    let target_options = if literal {
        source_options.delimiter = Some(';');
        source_options.csv_quote = Some('\'');
        source_options.has_header_row = Some(false);
        FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some(':'),
            ..Default::default()
        }
    } else {
        FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some('|'),
            csv_quote_disabled: true,
            has_header_row: Some(false),
            csv_utf8_bom: true,
            ..Default::default()
        }
    };
    DocumentProject::new(
        Project {
            execution_contract: None,
            source: SchemaNode::group(
                "CaptureRow",
                vec![
                    text("Depot"),
                    text("Code"),
                    SchemaNode::scalar("Count", ScalarType::Int),
                    text("Words"),
                ],
            ),
            target: SchemaNode::group(
                "Packet",
                vec![
                    SchemaNode::group(
                        ROWS,
                        vec![
                            text("Entry"),
                            SchemaNode::scalar("Copies", ScalarType::Int),
                            text("Comment"),
                            text("Location"),
                        ],
                    )
                    .repeating(),
                ],
            ),
            source_path: Some("never-open-preview.csv".into()),
            target_path: None,
            source_options,
            target_options,
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [
                    (11, field("Depot")),
                    (17, field("Code")),
                    (23, field("Count")),
                    (29, field("Words")),
                    (
                        31,
                        constant(if literal {
                            "public static class GeneratedMapping\n{"
                        } else {
                            "item-"
                        }),
                    ),
                    (
                        ENDING_NODE,
                        Node::RuntimeParameter {
                            name: "Ending".into(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                    (40, call("concat", &[31, 17, ENDING_NODE])),
                    (
                        44,
                        Node::RuntimeValue {
                            value: RuntimeValue::MappingFilePath,
                        },
                    ),
                    (
                        47,
                        Node::RuntimeValue {
                            value: RuntimeValue::CurrentDateTime,
                        },
                    ),
                    (50, constant("bundles/")),
                    (51, constant("-")),
                    (52, constant(".log")),
                    (55, call("concat", &[50, 11, 51, 47, 52])),
                    (
                        58,
                        Node::RuntimeParameterDefault {
                            name: "Destination".into(),
                            ty: ScalarType::String,
                            default: 55,
                            preview: None,
                        },
                    ),
                    (
                        61,
                        Node::RuntimeParameterDefault {
                            name: "WestDestination".into(),
                            ty: ScalarType::String,
                            default: 58,
                            preview: None,
                        },
                    ),
                    (64, constant("west")),
                    (67, call("equal", &[11, 64])),
                    (
                        PATH_NODE,
                        Node::If {
                            condition: 67,
                            then: 61,
                            else_: 58,
                        },
                    ),
                    (
                        80,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (
                        81,
                        Node::RuntimeParameterDefault {
                            name: "UseCounter".into(),
                            ty: ScalarType::Bool,
                            default: 80,
                            preview: None,
                        },
                    ),
                    (
                        82,
                        Node::Const {
                            value: Value::Int(10),
                        },
                    ),
                    (
                        83,
                        Node::InvocationCounter {
                            key: "packet-sequence".into(),
                            start: 82,
                            step: 82,
                            restart: None,
                        },
                    ),
                    (
                        84,
                        Node::If {
                            condition: 81,
                            then: 83,
                            else_: 23,
                        },
                    ),
                    (
                        INPUT_NODE,
                        if literal {
                            constant(LITERAL)
                        } else {
                            Node::RuntimeParameter {
                                name: "InputName".into(),
                                ty: ScalarType::String,
                                preview: Some("never-use-parameter-preview.csv".into()),
                            }
                        },
                    ),
                ]
                .into(),
            },
            root: Scope {
                iteration: ScopeIteration::DynamicDocuments {
                    source: vec![],
                    output_path: PATH_NODE,
                },
                group_by: Some(11),
                children: vec![Scope {
                    target_field: ROWS.into(),
                    iteration: ScopeIteration::Source(vec![]),
                    bindings: vec![
                        binding("Comment", 29),
                        binding("Entry", 40),
                        binding("Location", 44),
                        binding("Copies", 84),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
        },
        PrimarySourcePlan::new(INPUT_NODE),
    )
}
