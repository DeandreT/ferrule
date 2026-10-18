//! Genuine public Projects; no Program or implementation results construct these.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef, SequenceExpr,
    SourceOwner,
};
pub const EXACT: &str = " Cfg💠 ";
pub const AUDIT: &str = " audit💠 ";
pub const PROFILES: &[&str] = &[
    "literal",
    "exact",
    "owned-text",
    "generate",
    "generate-default",
    "float-target",
    "by-length",
    "regex-none",
    "regex-flags",
    "shared-node",
    "nested-index",
    "nested-producer",
    "mixed-legacy",
    "rows-index",
    "rows-position",
    "rows-root-pin",
    "grouped",
    "cross-owner",
    "controls",
    "text-index-raise",
    "range-index-raise",
    "regex-index-raise",
    "failures",
    "range",
    "empty-index-raise",
    "input-raise",
    "inactive-index",
    "empty",
];
pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
pub fn owner(name: &str) -> SourceOwner {
    if name == "Primary" {
        SourceOwner::Primary
    } else {
        SourceOwner::Named { name: name.into() }
    }
}
pub fn field(name: &str, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
        frame: pin.map(path),
    })
}
fn position(name: &str, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(name),
        collection: Some(path(parts)),
    })
}
pub fn mixed(name: &str) -> SchemaNode {
    SchemaNode::scalar_union(
        name,
        ir::ScalarTypeSet::new([
            ScalarType::String,
            ScalarType::Int,
            ScalarType::Float,
            ScalarType::Bool,
        ])
        .unwrap(),
    )
}
fn rows() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![string("Key"), mixed("Index"), int("Rank"), bool_("Reject")],
    )
    .repeating()
}
pub fn endpoint() -> SchemaNode {
    SchemaNode::group(
        "Endpoint",
        vec![
            SchemaNode::group(
                "Cfg",
                [
                    "Index",
                    "Text",
                    "Delimiter",
                    "From",
                    "To",
                    "Width",
                    "Pattern",
                    "Flags",
                ]
                .into_iter()
                .map(mixed)
                .collect(),
            ),
            bool_("GlobalReject"),
            rows(),
        ],
    )
}
fn output() -> SchemaNode {
    SchemaNode::group("Output", vec![mixed("Result")])
}
pub fn bindings(fields: &[(&str, u32)]) -> Vec<Binding> {
    fields
        .iter()
        .map(|(name, node)| Binding {
            target_field: (*name).into(),
            node: *node,
        })
        .collect()
}
pub fn iteration(name: &str) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(&["Rows"]),
        },
        output_path: None,
    }
}
pub fn tokenize(input: u32, index: u32, item: u32) -> Node {
    Node::SequenceItemAt {
        sequence: SequenceExpr::Tokenize {
            input,
            delimiter: 11,
            item,
        },
        index,
    }
}
pub fn base() -> Project {
    let mut nodes = BTreeMap::from([
        (
            10,
            Node::Const {
                value: Value::String("a|β|終".into()),
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            12,
            Node::Const {
                value: Value::Int(10),
            },
        ),
        (
            13,
            Node::Const {
                value: Value::Int(12),
            },
        ),
        (
            14,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            15,
            Node::Const {
                value: Value::String("[,;]".into()),
            },
        ),
        (
            16,
            Node::Const {
                value: Value::String(String::new()),
            },
        ),
        (
            17,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            18,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (30, field("Cfg", &["Cfg", "Index"], Some(&[]))),
        (31, field("Cfg", &["Rows", "Key"], Some(&["Rows"]))),
        (32, field("Cfg", &["Rows", "Rank"], Some(&["Rows"]))),
        (33, position("Cfg", &["Rows"])),
        (40, tokenize(10, 30, 90)),
        (70, field(EXACT, &["Cfg", "Index"], Some(&[]))),
        (
            80,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Tokenize {
                    input: 81,
                    delimiter: 82,
                    item: 190,
                },
                index: 70,
            },
        ),
        (
            81,
            Node::Const {
                value: Value::String("u-a|u-b|u-c".into()),
            },
        ),
        (
            82,
            Node::Const {
                value: Value::String("|".into()),
            },
        ),
        (
            85,
            Node::SourceField {
                path: path(&["Legacy", "Text"]),
                frame: None,
            },
        ),
        (
            110,
            Node::Const {
                value: Value::String("PRODUCER".into()),
            },
        ),
        (
            111,
            Node::Const {
                value: Value::String("LATER".into()),
            },
        ),
        (
            112,
            Node::Const {
                value: Value::String("FLAGS".into()),
            },
        ),
        (
            113,
            Node::Const {
                value: Value::String("INDEX".into()),
            },
        ),
        (
            900,
            Node::Const {
                value: Value::String("unused.json".into()),
            },
        ),
    ]);
    for id in [90, 190] {
        nodes.insert(
            id,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
    }
    Project {
        source: endpoint(),
        target: output(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Cfg".into(),
                path: "unused/Cfg.json".into(),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: EXACT.into(),
                path: "unused/exact.json".into(),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Legacy".into(),
                path: "unused/Legacy.json".into(),
                schema: SchemaNode::group("Legacy", vec![string("Text")]),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "UnusedDynamic".into(),
                path: "unused/dynamic.json".into(),
                schema: endpoint(),
                options: Default::default(),
                dynamic_path: Some(mapping::DynamicSourcePath {
                    node: 900,
                    iteration: vec![],
                }),
            },
        ],
        extra_targets: vec![
            NamedTarget {
                name: AUDIT.into(),
                path: None,
                schema: output(),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Result", 80)]),
                    ..Default::default()
                },
            },
            NamedTarget {
                name: "legacy".into(),
                path: None,
                schema: output(),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("Result", 85)]),
                    ..Default::default()
                },
            },
        ],
        graph: Graph { nodes },
        root: Scope {
            bindings: bindings(&[("Result", 40)]),
            ..Default::default()
        },
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
fn range(from: Option<u32>, to: u32, index: u32, item: u32) -> Node {
    Node::SequenceItemAt {
        sequence: SequenceExpr::Generate { from, to, item },
        index,
    }
}
pub fn project(profile: &str) -> Project {
    assert!(
        PROFILES.contains(&profile),
        "unknown authored profile {profile}"
    );
    let mut p = base();
    match profile {
        "literal" => {}
        "exact" => {
            p.graph
                .nodes
                .insert(30, field(EXACT, &["Cfg", "Index"], Some(&[])));
        }
        "owned-text" => {
            p.graph
                .nodes
                .insert(10, field("Cfg", &["Cfg", "Text"], Some(&[])));
            p.graph
                .nodes
                .insert(11, field("Cfg", &["Cfg", "Delimiter"], Some(&[])));
        }
        "generate" | "float-target" => {
            p.graph.nodes.insert(40, range(Some(12), 13, 30, 90));
            if profile == "float-target" {
                p.target = SchemaNode::group(
                    "Output",
                    vec![SchemaNode::scalar("Result", ScalarType::Float)],
                );
            }
        }
        "generate-default" => {
            p.graph
                .nodes
                .insert(13, field("Cfg", &["Cfg", "To"], Some(&[])));
            p.graph.nodes.insert(40, range(None, 13, 30, 90));
        }
        "range" => {
            p.graph
                .nodes
                .insert(12, field("Cfg", &["Cfg", "From"], Some(&[])));
            p.graph
                .nodes
                .insert(13, field("Cfg", &["Cfg", "To"], Some(&[])));
            p.graph.nodes.insert(40, range(Some(12), 13, 30, 90));
        }
        "empty-index-raise" => {
            p.graph.nodes.insert(
                12,
                Node::Const {
                    value: Value::Int(4),
                },
            );
            p.graph.nodes.insert(
                13,
                Node::Const {
                    value: Value::Int(3),
                },
            );
            p.graph.nodes.insert(30, Node::Raise { message: Some(113) });
            p.graph.nodes.insert(40, range(Some(12), 13, 30, 90));
        }
        "empty" => {
            p.graph.nodes.insert(
                12,
                Node::Const {
                    value: Value::Int(4),
                },
            );
            p.graph.nodes.insert(
                13,
                Node::Const {
                    value: Value::Int(3),
                },
            );
            p.graph.nodes.insert(40, range(Some(12), 13, 30, 90));
        }
        "input-raise" => {
            p.graph.nodes.insert(10, Node::Raise { message: Some(110) });
        }
        "by-length" => {
            p.graph
                .nodes
                .insert(10, field("Cfg", &["Cfg", "Text"], Some(&[])));
            p.graph
                .nodes
                .insert(14, field("Cfg", &["Cfg", "Width"], Some(&[])));
            p.graph.nodes.insert(
                40,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::TokenizeByLength {
                        input: 10,
                        length: 14,
                        item: 90,
                    },
                    index: 30,
                },
            );
        }
        "regex-none" | "regex-flags" => {
            p.graph
                .nodes
                .insert(10, field("Cfg", &["Cfg", "Text"], Some(&[])));
            p.graph
                .nodes
                .insert(15, field("Cfg", &["Cfg", "Pattern"], Some(&[])));
            p.graph
                .nodes
                .insert(16, field("Cfg", &["Cfg", "Flags"], Some(&[])));
            p.graph.nodes.insert(
                40,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::TokenizeRegex {
                        input: 10,
                        pattern: 15,
                        flags: if profile == "regex-flags" {
                            Some(16)
                        } else {
                            None
                        },
                        item: 90,
                    },
                    index: 30,
                },
            );
        }
        "shared-node" => {
            p.graph.nodes.insert(40, range(None, 30, 30, 90));
        }
        "nested-index" => {
            p.graph.nodes.insert(50, range(None, 14, 30, 91));
            p.graph.nodes.insert(
                91,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            p.graph.nodes.insert(40, tokenize(10, 50, 90));
        }
        "nested-producer" => {
            p.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::String("a,b|c,d|e,f".into()),
                },
            );
            p.graph.nodes.insert(50, tokenize(10, 30, 91));
            p.graph.nodes.insert(
                91,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                51,
                Node::Const {
                    value: Value::String(",".into()),
                },
            );
            p.graph.nodes.insert(
                40,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::Tokenize {
                        input: 50,
                        delimiter: 51,
                        item: 90,
                    },
                    index: 14,
                },
            );
        }
        "mixed-legacy" => {
            p.graph.nodes.insert(
                10,
                Node::If {
                    condition: 17,
                    then: 20,
                    else_: 21,
                },
            );
            p.graph
                .nodes
                .insert(20, field("Cfg", &["Cfg", "Text"], Some(&[])));
            p.graph.nodes.insert(
                21,
                Node::SourceField {
                    path: path(&["Legacy", "Text"]),
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                34,
                Node::ValueMap {
                    input: 30,
                    input_type: None,
                    table: vec![(Value::Int(3), Value::Int(2))],
                    default: Some(Value::Int(1)),
                },
            );
            p.graph.nodes.insert(40, tokenize(10, 34, 90));
        }
        "rows-index" | "rows-position" | "rows-root-pin" | "grouped" | "controls"
        | "inactive-index" => {
            p.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::String(
                        if profile == "grouped" {
                            "x|y|z"
                        } else {
                            "a|b|c"
                        }
                        .into(),
                    ),
                },
            );
            if profile != "rows-root-pin" {
                p.graph.nodes.insert(
                    30,
                    if profile == "rows-position" {
                        position("Cfg", &["Rows"])
                    } else {
                        field("Cfg", &["Rows", "Index"], Some(&["Rows"]))
                    },
                );
            }
            let mut scope = Scope {
                target_field: if profile == "grouped" {
                    "Groups"
                } else {
                    "Rows"
                }
                .into(),
                iteration: iteration("Cfg"),
                bindings: bindings(&[("Result", 40)]),
                ..Default::default()
            };
            if profile == "inactive-index" {
                scope.iteration = iteration("Primary");
            }
            let mut fields = vec![mixed("Result")];
            if profile == "grouped" {
                p.graph.nodes.insert(42, tokenize(10, 33, 92));
                p.graph.nodes.insert(
                    92,
                    Node::SourceField {
                        path: vec![],
                        frame: None,
                    },
                );
                scope.group_by = Some(31);
                scope.bindings.push(Binding {
                    target_field: "ByPosition".into(),
                    node: 42,
                });
                fields.push(mixed("ByPosition"));
                fields.push(
                    SchemaNode::group("Members", vec![mixed("Result"), mixed("ByPosition")])
                        .repeating(),
                );
                scope.children.push(Scope {
                    target_field: "Members".into(),
                    iteration: iteration("Cfg"),
                    bindings: bindings(&[("Result", 40), ("ByPosition", 42)]),
                    ..Default::default()
                });
            }
            if profile == "controls" {
                scope.filter = Some(17);
                scope.sort_by = Some(32);
                scope.windows = vec![mapping::SequenceWindow::Last { count: 14 }];
                scope.group_by = Some(40);
                scope.post_group_filter = Some(17);
            }
            let name = scope.target_field.clone();
            p.target =
                SchemaNode::group("Output", vec![SchemaNode::group(name, fields).repeating()]);
            p.root = Scope {
                children: vec![scope],
                ..Default::default()
            };
        }
        "cross-owner" => {
            p.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::String("a|b|c".into()),
                },
            );
            p.graph
                .nodes
                .insert(30, field("Cfg", &["Rows", "Index"], Some(&["Rows"])));
            p.graph
                .nodes
                .insert(61, field("Primary", &["Rows", "Index"], Some(&["Rows"])));
            p.graph.nodes.insert(60, tokenize(10, 61, 94));
            p.graph.nodes.insert(
                94,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
            p.target = SchemaNode::group(
                "Output",
                vec![
                    SchemaNode::group(
                        "Parents",
                        vec![
                            mixed("OuterResult"),
                            SchemaNode::group("Rows", vec![mixed("Result")]).repeating(),
                        ],
                    )
                    .repeating(),
                ],
            );
            p.root = Scope {
                children: vec![Scope {
                    target_field: "Parents".into(),
                    iteration: iteration("Primary"),
                    bindings: bindings(&[("OuterResult", 60)]),
                    children: vec![Scope {
                        target_field: "Rows".into(),
                        iteration: iteration("Cfg"),
                        bindings: bindings(&[("Result", 40)]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            };
        }
        "text-index-raise" => {
            p.graph
                .nodes
                .insert(10, field("Cfg", &["Cfg", "Text"], Some(&[])));
            p.graph.nodes.insert(11, Node::Raise { message: Some(111) });
            p.graph.nodes.insert(30, Node::Raise { message: Some(113) });
        }
        "range-index-raise" => {
            p.graph
                .nodes
                .insert(12, field("Cfg", &["Cfg", "From"], Some(&[])));
            p.graph.nodes.insert(13, Node::Raise { message: Some(111) });
            p.graph.nodes.insert(30, Node::Raise { message: Some(113) });
            p.graph.nodes.insert(40, range(Some(12), 13, 30, 90));
        }
        "regex-index-raise" => {
            p.graph
                .nodes
                .insert(10, field("Cfg", &["Cfg", "Text"], Some(&[])));
            p.graph
                .nodes
                .insert(15, field("Cfg", &["Cfg", "Pattern"], Some(&[])));
            p.graph.nodes.insert(16, Node::Raise { message: Some(112) });
            p.graph.nodes.insert(30, Node::Raise { message: Some(113) });
            p.graph.nodes.insert(
                40,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::TokenizeRegex {
                        input: 10,
                        pattern: 15,
                        flags: Some(16),
                        item: 90,
                    },
                    index: 30,
                },
            );
        }
        "failures" => {
            p.graph.nodes.insert(10, Node::Raise { message: Some(110) });
            p.graph.nodes.insert(30, Node::Raise { message: Some(113) });
            p.graph.nodes.insert(
                200,
                Node::SourceField {
                    path: path(&["GlobalReject"]),
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                201,
                Node::Const {
                    value: Value::String("global.reject".into()),
                },
            );
            p.graph
                .nodes
                .insert(202, field("Primary", &["Rows", "Reject"], Some(&["Rows"])));
            p.graph.nodes.insert(
                203,
                Node::Const {
                    value: Value::String("row.reject".into()),
                },
            );
            p.failure_rules = vec![
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source { collection: vec![] },
                    selection: mapping::FailureSelection::WhenTrue { predicate: 200 },
                    message: Some(201),
                },
                mapping::FailureRule {
                    iteration: mapping::FailureIteration::Source {
                        collection: path(&["Rows"]),
                    },
                    selection: mapping::FailureSelection::WhenTrue { predicate: 202 },
                    message: Some(203),
                },
            ];
        }
        _ => unreachable!(),
    }
    p
}
