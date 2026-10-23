//! Genuine public Project definitions for owned filters. Authored source only:
//! no mapping, predicate or position is evaluated here.
use super::*;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, NamedSource, NamedTarget, OwnedPosition,
    OwnedSourceField, OwnedSourceRef, SourceOwner,
};

pub const NAMED: &str = "Ref";
/// A second declared source whose name matches only exactly: untrimmed and
/// non-ASCII. No case supplies it.
pub const UNSUPPLIED: &str = " Ref 雪 ";

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 6] = [
    "phases",
    "nested_identity",
    "lazy_false",
    "non_bool",
    "order",
    "global_rules",
];

fn named() -> SourceOwner {
    SourceOwner::Named { name: NAMED.into() }
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn owned_field(owner: SourceOwner, field: &[&str], frame: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(field),
        },
        frame: Some(path(frame)),
    })
}

fn owned_position(owner: SourceOwner, collection: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: Some(path(collection)),
    })
}

fn legacy_position(collection: &[&str]) -> Node {
    Node::Position {
        collection: path(collection),
    }
}

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}

fn raise(message: u32) -> Node {
    Node::Raise {
        message: Some(message),
    }
}

fn row() -> Vec<SchemaNode> {
    vec![int("Value"), bool_("Flag")]
}

/// Every source declares the same shape: a root `Value`, an embedded `Ref`
/// group whose names collide with the named owner, and nested repeated rows.
fn input() -> SchemaNode {
    let mut rows = row();
    rows.push(SchemaNode::group("Rows", row()).repeating());
    SchemaNode::group(
        "Input",
        vec![
            string("Value"),
            SchemaNode::group(
                NAMED,
                vec![
                    string("Value"),
                    SchemaNode::group("Rows", row()).repeating(),
                ],
            ),
            SchemaNode::group("Rows", rows).repeating(),
        ],
    )
}

fn binds(bindings: &[(&str, u32)]) -> Vec<Binding> {
    bindings
        .iter()
        .map(|(target_field, node)| Binding {
            target_field: (*target_field).into(),
            node: *node,
        })
        .collect()
}

fn filtered(
    target_field: &str,
    owner: SourceOwner,
    rows: &[&str],
    filter: u32,
    bindings: &[(&str, u32)],
) -> Scope {
    Scope {
        target_field: target_field.into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: path(rows),
            },
            output_path: None,
        },
        filter: Some(filter),
        bindings: binds(bindings),
        ..Scope::default()
    }
}

/// The four output fields of the phase-sensitive row targets, then `children`.
fn row_target_with(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    let mut fields = vec![
        int("Value"),
        int("OwnedPosition"),
        int("LegacyPosition"),
        string("Root"),
    ];
    fields.extend(children);
    SchemaNode::group(name, fields).repeating()
}

fn row_target(name: &str) -> SchemaNode {
    row_target_with(name, vec![])
}

const ROW_BINDINGS: [(&str, u32); 4] = [
    ("Value", 1),
    ("OwnedPosition", 2),
    ("LegacyPosition", 6),
    ("Root", 5),
];

/// Nodes 1–6 filter primary rows by owned position > 1 and bind the row
/// value, its owned and legacy positions and the primary root value.
fn primary_rows() -> Vec<(u32, Node)> {
    vec![
        (
            1,
            owned_field(SourceOwner::Primary, &["Rows", "Value"], &["Rows"]),
        ),
        (2, owned_position(SourceOwner::Primary, &["Rows"])),
        (3, constant(Value::Int(1))),
        (4, call("greater_than", &[2, 3])),
        (5, owned_field(SourceOwner::Primary, &["Value"], &[])),
        (6, legacy_position(&["Rows"])),
    ]
}

fn build(
    target: SchemaNode,
    nodes: Vec<(u32, Node)>,
    root: Scope,
    extra_targets: Vec<NamedTarget>,
    failure_rules: Vec<FailureRule>,
) -> Project {
    let named_source = |name: &str| NamedSource {
        name: name.into(),
        path: "unused-filter-host-input.json".into(),
        schema: input(),
        options: Default::default(),
        dynamic_path: None,
    };
    Project {
        source: input(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![named_source(NAMED), named_source(UNSUPPLIED)],
        extra_targets,
        failure_rules,
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}

pub fn project(model: &str) -> Project {
    match model {
        // Primary rows filter by owned position > 1; the `Filtered` target
        // filters the named owner's rows by owned position > 2 and binds its
        // legacy root position.
        "phases" => {
            let mut nodes = primary_rows();
            nodes.extend([
                (11, owned_field(named(), &["Rows", "Value"], &["Rows"])),
                (12, owned_position(named(), &["Rows"])),
                (13, constant(Value::Int(2))),
                (14, call("greater_than", &[12, 13])),
                (15, owned_field(named(), &["Value"], &[])),
                (16, legacy_position(&[])),
            ]);
            let extra = NamedTarget {
                name: "Filtered".into(),
                path: None,
                schema: row_target("Filtered"),
                options: Default::default(),
                root: filtered(
                    "",
                    named(),
                    &["Rows"],
                    14,
                    &[
                        ("Value", 11),
                        ("OwnedPosition", 12),
                        ("LegacyPosition", 16),
                        ("Root", 15),
                    ],
                ),
            };
            build(
                row_target("Output"),
                nodes,
                filtered("", SourceOwner::Primary, &["Rows"], 4, &ROW_BINDINGS),
                vec![extra],
                vec![],
            )
        }
        // Nested `Kids` filter by their own owned position > 1 and bind the
        // parent's; `Identity` re-iterates the current row under a constant
        // true filter.
        "nested_identity" => {
            let mut nodes = primary_rows();
            nodes.extend([
                (
                    21,
                    owned_field(
                        SourceOwner::Primary,
                        &["Rows", "Rows", "Value"],
                        &["Rows", "Rows"],
                    ),
                ),
                (22, owned_position(SourceOwner::Primary, &["Rows", "Rows"])),
                (23, call("greater_than", &[22, 3])),
                (24, constant(Value::Bool(true))),
            ]);
            let target = row_target_with(
                "Output",
                vec![
                    SchemaNode::group(
                        "Kids",
                        vec![int("Value"), int("Position"), int("ParentPosition")],
                    )
                    .repeating(),
                    SchemaNode::group("Identity", vec![int("Value"), int("ParentPosition")])
                        .repeating(),
                ],
            );
            let mut root = filtered("", SourceOwner::Primary, &["Rows"], 4, &ROW_BINDINGS);
            root.children = vec![
                filtered(
                    "Kids",
                    SourceOwner::Primary,
                    &["Rows", "Rows"],
                    23,
                    &[("Value", 21), ("Position", 22), ("ParentPosition", 2)],
                ),
                filtered(
                    "Identity",
                    SourceOwner::Primary,
                    &["Rows"],
                    24,
                    &[("Value", 1), ("ParentPosition", 2)],
                ),
            ];
            build(target, nodes, root, vec![], vec![])
        }
        // A constant false predicate guards the named flag through a lazy
        // `If`; the raising target binding must never run.
        "lazy_false" => build(
            SchemaNode::group("Output", vec![int("Value"), string("Raise")]).repeating(),
            vec![
                (1, owned_field(named(), &["Rows", "Value"], &["Rows"])),
                (2, owned_field(named(), &["Rows", "Flag"], &["Rows"])),
                (3, constant(Value::Bool(false))),
                (
                    4,
                    Node::If {
                        condition: 3,
                        then: 2,
                        else_: 3,
                    },
                ),
                (5, constant(Value::String("target must stay lazy".into()))),
                (6, raise(5)),
            ],
            filtered(
                "",
                SourceOwner::Primary,
                &["Rows"],
                4,
                &[("Value", 1), ("Raise", 6)],
            ),
            vec![],
            vec![],
        ),
        // Node zero is an Int predicate.
        "non_bool" => build(
            SchemaNode::group("Output", vec![int("Value")]).repeating(),
            vec![
                (
                    1,
                    owned_field(SourceOwner::Primary, &["Rows", "Value"], &["Rows"]),
                ),
                (0, constant(Value::Int(7))),
            ],
            filtered("", SourceOwner::Primary, &["Rows"], 0, &[("Value", 1)]),
            vec![],
            vec![],
        ),
        // The predicate raises from the second row, the target binding on
        // every accepted row: the first row's target raises first.
        "order" => build(
            SchemaNode::group("Output", vec![string("Value")]).repeating(),
            vec![
                (1, owned_position(SourceOwner::Primary, &["Rows"])),
                (2, constant(Value::Int(1))),
                (3, call("equal", &[1, 2])),
                (4, raise(7)),
                (
                    5,
                    Node::If {
                        condition: 3,
                        then: 9,
                        else_: 4,
                    },
                ),
                (6, raise(8)),
                (7, constant(Value::String("later predicate".into()))),
                (8, constant(Value::String("target first".into()))),
                (
                    9,
                    owned_field(SourceOwner::Primary, &["Rows", "Flag"], &["Rows"]),
                ),
            ],
            filtered("", SourceOwner::Primary, &["Rows"], 5, &[("Value", 6)]),
            vec![],
            vec![],
        ),
        // Two legacy failure rules over primary rows: the first never fires
        // and keeps its raising message lazy; the second fires on the second
        // flagged row with that row's value.
        "global_rules" => {
            let mut nodes = primary_rows();
            nodes.extend([
                (20, constant(Value::Bool(false))),
                (
                    21,
                    constant(Value::String("false rule message must stay lazy".into())),
                ),
                (22, raise(21)),
                (30, legacy_position(&["Rows"])),
                (31, constant(Value::Int(2))),
                (32, call("equal", &[30, 31])),
                (
                    33,
                    Node::SourceField {
                        path: path(&["Flag"]),
                        frame: Some(path(&["Rows"])),
                    },
                ),
                (34, call("and", &[32, 33])),
                (
                    35,
                    Node::SourceField {
                        path: path(&["Value"]),
                        frame: Some(path(&["Rows"])),
                    },
                ),
            ]);
            let rule = |predicate, message| FailureRule {
                iteration: FailureIteration::Source {
                    collection: path(&["Rows"]),
                },
                selection: FailureSelection::WhenTrue { predicate },
                message: Some(message),
            };
            build(
                row_target("Output"),
                nodes,
                filtered("", SourceOwner::Primary, &["Rows"], 4, &ROW_BINDINGS),
                vec![],
                vec![rule(20, 22), rule(34, 35)],
            )
        }
        _ => panic!("authored model"),
    }
}
