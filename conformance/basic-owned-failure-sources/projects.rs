//! Genuine public Projects for owned failure-rule iteration (#471).
//!
//! Every Project declares the same Primary and `Lookup` endpoint schema, so each
//! owned path also exists under the other owner. `Batch` is a repeated named
//! root. Expected results live separately in `cases.rs`.
use super::*;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, NamedSource, NamedTarget, OwnedPosition,
    OwnedSourceField, OwnedSourceRef, SourceOwner,
};

pub const LOOKUP: &str = "Lookup";
pub const BATCH: &str = "Batch";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "F01-primary-when-true",
    "F02-named-when-true",
    "F03-primary-when-false",
    "F04-named-when-false",
    "F05-named-all-ordinal",
    "F06-primary-all-absent-message",
    "F07-legacy-row-reads",
    "F08-rule-order",
    "F09-lazy-predicate-and-message",
    "F10-absent-owner",
    "F11-named-repeated-root",
    "F12-primary-root-identity",
    "F13-nested-lines",
    "F14-legacy-alias-beside-owned",
    "F15-primary-row-rule-beside-owned",
    "F16-cross-owner-root-pins",
    "F17-ending-groups-beside-walk",
];

pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
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
pub fn position(name: &str, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(name),
        collection: Some(path(parts)),
    })
}
pub fn legacy(parts: &[&str]) -> Node {
    Node::SourceField {
        path: path(parts),
        frame: None,
    }
}
pub fn rows(name: &str, parts: &[&str]) -> FailureIteration {
    FailureIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
    }
}
pub fn rule(
    iteration: FailureIteration,
    selection: FailureSelection,
    message: Option<u32>,
) -> FailureRule {
    FailureRule {
        iteration,
        selection,
        message,
    }
}
pub fn when_true(predicate: u32) -> FailureSelection {
    FailureSelection::WhenTrue { predicate }
}
pub fn when_false(predicate: u32) -> FailureSelection {
    FailureSelection::WhenFalse { predicate }
}
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn boolean(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Bool)
}
pub fn row_schema(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![boolean("Flag"), string("Code"), string("Message")]).repeating()
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Note"),
            row_schema("Rows"),
            SchemaNode::group(
                "Orders",
                vec![
                    string("Id"),
                    SchemaNode::group("Lines", vec![boolean("Flag"), string("Code")]).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn constant(project: &mut Project, id: u32, value: Value) {
    project.graph.nodes.insert(id, Node::Const { value });
}
fn call(project: &mut Project, id: u32, function: &str, args: &[u32]) {
    project.graph.nodes.insert(
        id,
        Node::Call {
            function: function.into(),
            args: args.to_vec(),
        },
    );
}
fn output(name: &str, value: u32) -> (SchemaNode, Scope) {
    (
        SchemaNode::group(name, vec![string("Value")]),
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: value,
            }],
            ..Scope::default()
        },
    )
}

/// Shared nodes: Primary 1-4 and 21, Lookup 11-14 and 22, legacy 31-32,
/// root pins 23-24 and constants 40-46. Profiles add nodes from 50.
pub fn base() -> Project {
    let (target, root) = output("Output", 21);
    let (audit, audit_root) = output("Audit", 22);
    let mut project = Project {
        source: endpoint_schema("Input"),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: LOOKUP.into(),
                path: "lookup.json".into(),
                schema: endpoint_schema(LOOKUP),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: BATCH.into(),
                path: "batch.json".into(),
                schema: row_schema(BATCH),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: audit,
            options: Default::default(),
            root: audit_root,
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root,
    };
    for (offset, name) in [(0, "Primary"), (10, LOOKUP)] {
        let nodes = &mut project.graph.nodes;
        nodes.insert(offset + 1, field(name, &["Rows", "Flag"], None));
        nodes.insert(offset + 2, field(name, &["Rows", "Message"], None));
        nodes.insert(offset + 3, field(name, &["Rows", "Code"], None));
        nodes.insert(offset + 4, position(name, &["Rows"]));
    }
    let nodes = &mut project.graph.nodes;
    nodes.insert(21, field("Primary", &["Note"], None));
    nodes.insert(22, field(LOOKUP, &["Note"], None));
    nodes.insert(23, field("Primary", &["Note"], Some(&[])));
    nodes.insert(24, field(LOOKUP, &["Note"], Some(&[])));
    nodes.insert(31, legacy(&["Flag"]));
    nodes.insert(32, legacy(&["Message"]));
    for (id, value) in [
        (40, Value::String(":".into())),
        (41, Value::String("X".into())),
        (42, Value::Bool(true)),
        (43, Value::Bool(false)),
        (44, Value::String("not boolean".into())),
        (45, Value::String("legacy".into())),
        (46, Value::String("P:".into())),
    ] {
        constant(&mut project, id, value);
    }
    project
}

pub fn project(profile: &str) -> Project {
    let mut p = base();
    let rules = match profile {
        "F01-primary-when-true" => vec![rule(rows("Primary", &["Rows"]), when_true(1), Some(2))],
        "F02-named-when-true" => vec![rule(rows(LOOKUP, &["Rows"]), when_true(11), Some(12))],
        "F03-primary-when-false" => {
            vec![rule(rows("Primary", &["Rows"]), when_false(1), Some(2))]
        }
        "F04-named-when-false" => vec![rule(rows(LOOKUP, &["Rows"]), when_false(11), Some(12))],
        "F05-named-all-ordinal" => {
            call(&mut p, 50, "concat", &[14, 40, 12, 40, 21]);
            vec![rule(rows(LOOKUP, &["Rows"]), FailureSelection::All, Some(50))]
        }
        "F06-primary-all-absent-message" => {
            vec![rule(rows("Primary", &["Rows"]), FailureSelection::All, None)]
        }
        "F07-legacy-row-reads" => vec![rule(rows(LOOKUP, &["Rows"]), when_true(31), Some(32))],
        "F08-rule-order" => {
            call(&mut p, 50, "equal", &[13, 41]);
            call(&mut p, 51, "concat", &[46, 2]);
            vec![
                rule(rows(LOOKUP, &["Rows"]), when_true(50), Some(12)),
                rule(rows("Primary", &["Rows"]), when_true(1), Some(51)),
                rule(
                    FailureIteration::Source {
                        collection: path(&["Rows"]),
                    },
                    FailureSelection::All,
                    Some(45),
                ),
            ]
        }
        "F09-lazy-predicate-and-message" => {
            p.graph.nodes.insert(
                50,
                Node::If {
                    condition: 11,
                    then: 11,
                    else_: 44,
                },
            );
            p.graph.nodes.insert(52, Node::Raise { message: Some(12) });
            p.graph.nodes.insert(
                51,
                Node::If {
                    condition: 11,
                    then: 12,
                    else_: 52,
                },
            );
            vec![rule(rows(LOOKUP, &["Rows"]), when_true(50), Some(51))]
        }
        "F10-absent-owner" => vec![
            rule(rows(LOOKUP, &["Rows"]), when_true(11), Some(12)),
            rule(rows("Primary", &["Rows"]), when_true(1), Some(2)),
        ],
        "F11-named-repeated-root" => {
            let nodes = &mut p.graph.nodes;
            nodes.insert(50, field(BATCH, &["Flag"], None));
            nodes.insert(51, field(BATCH, &["Message"], None));
            nodes.insert(52, position(BATCH, &[]));
            call(&mut p, 53, "concat", &[51, 40, 52]);
            vec![rule(rows(BATCH, &[]), when_false(50), Some(53))]
        }
        "F12-primary-root-identity" => {
            call(&mut p, 50, "concat", &[21, 40, 23]);
            vec![rule(rows("Primary", &[]), FailureSelection::All, Some(50))]
        }
        "F13-nested-lines" => {
            let nodes = &mut p.graph.nodes;
            nodes.insert(50, field(LOOKUP, &["Orders", "Lines", "Flag"], None));
            nodes.insert(51, field(LOOKUP, &["Orders", "Id"], None));
            nodes.insert(52, position(LOOKUP, &["Orders"]));
            nodes.insert(53, position(LOOKUP, &["Orders", "Lines"]));
            nodes.insert(
                54,
                field(LOOKUP, &["Orders", "Lines", "Code"], Some(&["Orders", "Lines"])),
            );
            call(&mut p, 55, "concat", &[51, 40, 52, 40, 53, 40, 54]);
            vec![rule(rows(LOOKUP, &["Orders", "Lines"]), when_true(50), Some(55))]
        }
        "F14-legacy-alias-beside-owned" => vec![
            rule(rows("Primary", &["Rows"]), when_true(1), Some(2)),
            rule(
                FailureIteration::Source {
                    collection: path(&[LOOKUP, "Rows"]),
                },
                when_true(31),
                Some(32),
            ),
        ],
        "F15-primary-row-rule-beside-owned" => vec![
            rule(
                FailureIteration::Source {
                    collection: path(&["Rows"]),
                },
                when_true(1),
                Some(2),
            ),
            rule(rows(LOOKUP, &["Rows"]), when_true(11), Some(12)),
        ],
        "F16-cross-owner-root-pins" => {
            call(&mut p, 50, "concat", &[12, 40, 23, 40, 24, 40, 14]);
            vec![rule(rows(LOOKUP, &["Rows"]), when_true(11), Some(50))]
        }
        "F17-ending-groups-beside-walk" => {
            // The audit target groups Lookup rows, closing a group at a true flag.
            p.extra_targets[0].schema = SchemaNode::group(
                "Audit",
                vec![SchemaNode::group("Groups", vec![string("First")]).repeating()],
            );
            p.extra_targets[0].root = Scope {
                children: vec![Scope {
                    target_field: "Groups".into(),
                    iteration: mapping::ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: owner(LOOKUP),
                            path: path(&["Rows"]),
                        },
                        output_path: None,
                    },
                    group_ending_with: Some(11),
                    bindings: vec![Binding {
                        target_field: "First".into(),
                        node: 12,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            };
            call(&mut p, 50, "equal", &[13, 41]);
            vec![rule(rows(LOOKUP, &["Rows"]), when_true(50), Some(12))]
        }
        _ => panic!("authored profile {profile}"),
    };
    p.failure_rules = rules;
    p
}
