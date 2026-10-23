//! Genuine public Project definitions for owned nested aliases. Authored
//! source only: no mapping, predicate or position is evaluated here.
use super::*;
use mapping::{NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

pub const NAMED: &str = "Ref";

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 6] = [
    "outer_suffix",
    "nested_rows",
    "empty_root",
    "same_owner",
    "singleton",
    "raw_compact",
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

/// A legacy field read; `frame: None` resolves relative to the current scope.
fn legacy_field(field: &[&str], frame: Option<&[&str]>) -> Node {
    Node::SourceField {
        path: path(field),
        frame: frame.map(path),
    }
}

fn legacy_position(collection: &[&str]) -> Node {
    Node::Position {
        collection: path(collection),
    }
}

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn greater_than(left: u32, right: u32) -> Node {
    Node::Call {
        function: "greater_than".into(),
        args: vec![left, right],
    }
}

fn rows(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Rows", children).repeating()
}

fn input(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Input", children)
}

fn output(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Output", children).repeating()
}

fn items(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Items", children).repeating()
}

fn scope(
    target_field: &str,
    owner: SourceOwner,
    collection: &[&str],
    filter: Option<u32>,
    bindings: &[(&str, u32)],
    children: Vec<Scope>,
) -> Scope {
    Scope {
        target_field: target_field.into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: path(collection),
            },
            output_path: None,
        },
        filter,
        bindings: bindings
            .iter()
            .map(|(target_field, node)| Binding {
                target_field: (*target_field).into(),
                node: *node,
            })
            .collect(),
        children,
        ..Scope::default()
    }
}

fn build(
    source: SchemaNode,
    named_schema: SchemaNode,
    target: SchemaNode,
    nodes: Vec<(u32, Node)>,
    root: Scope,
) -> Project {
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: NAMED.into(),
            path: "unused-filter-host-input.json".into(),
            schema: named_schema,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}

pub fn project(model: &str) -> Project {
    let value_rows = || rows(vec![string("Value")]);
    match model {
        // Primary `Ref.Rows` outer rows; the inner scope iterates the named
        // owner's `Rows` filtered by the legacy `Rows` position, beside legacy
        // positions whose suffixes resolve in opposite directions.
        "outer_suffix" => build(
            input(vec![
                SchemaNode::group(NAMED, vec![value_rows()]),
                value_rows(),
            ]),
            input(vec![value_rows()]),
            output(vec![
                string("OuterValue"),
                int("OuterPosition"),
                items(vec![
                    string("InnerValue"),
                    string("LongPinValue"),
                    int("LegacyOuter"),
                    int("LegacyInner"),
                    int("LegacyNearest"),
                    int("ExactOwnedOuter"),
                    int("ExactOwnedInner"),
                ]),
            ]),
            vec![
                (
                    1,
                    owned_field(
                        SourceOwner::Primary,
                        &["Ref", "Rows", "Value"],
                        &["Ref", "Rows"],
                    ),
                ),
                (2, owned_position(SourceOwner::Primary, &["Ref", "Rows"])),
                (3, legacy_position(&["Ref", "Rows"])),
                (4, legacy_position(&["Rows"])),
                (5, legacy_position(&[])),
                (6, legacy_field(&["Value"], Some(&["Ref", "Rows"]))),
                (7, owned_field(named(), &["Rows", "Value"], &["Rows"])),
                (8, owned_position(named(), &["Rows"])),
                (9, constant(Value::Int(1))),
                (10, greater_than(4, 9)),
            ],
            scope(
                "",
                SourceOwner::Primary,
                &["Ref", "Rows"],
                None,
                &[("OuterValue", 1), ("OuterPosition", 2)],
                vec![scope(
                    "Items",
                    named(),
                    &["Rows"],
                    Some(10),
                    &[
                        ("InnerValue", 7),
                        ("LongPinValue", 6),
                        ("LegacyOuter", 3),
                        ("LegacyInner", 4),
                        ("LegacyNearest", 5),
                        ("ExactOwnedOuter", 2),
                        ("ExactOwnedInner", 8),
                    ],
                    vec![],
                )],
            ),
        ),
        // Named `Rows` with nested `Rows`; the inner scope is filtered by the
        // short legacy position and binds short and long legacy pins.
        "nested_rows" => {
            let nested = || input(vec![rows(vec![string("Value"), value_rows()])]);
            build(
                nested(),
                nested(),
                output(vec![
                    string("OuterValue"),
                    int("OuterPosition"),
                    items(vec![
                        string("InnerValue"),
                        string("ShortPinValue"),
                        string("LongPinValue"),
                        int("LegacyShort"),
                        int("LegacyLong"),
                        int("ExactOwnedOuter"),
                        int("ExactOwnedInner"),
                    ]),
                ]),
                vec![
                    (1, owned_field(named(), &["Rows", "Value"], &["Rows"])),
                    (2, owned_position(named(), &["Rows"])),
                    (3, legacy_position(&["Rows"])),
                    (4, legacy_position(&["Rows", "Rows"])),
                    (5, legacy_field(&["Value"], Some(&["Rows"]))),
                    (6, legacy_field(&["Value"], Some(&["Rows", "Rows"]))),
                    (
                        7,
                        owned_field(named(), &["Rows", "Rows", "Value"], &["Rows", "Rows"]),
                    ),
                    (8, owned_position(named(), &["Rows", "Rows"])),
                    (9, constant(Value::Int(1))),
                    (10, greater_than(3, 9)),
                ],
                scope(
                    "",
                    named(),
                    &["Rows"],
                    None,
                    &[("OuterValue", 1), ("OuterPosition", 2)],
                    vec![scope(
                        "Items",
                        named(),
                        &["Rows", "Rows"],
                        Some(10),
                        &[
                            ("InnerValue", 7),
                            ("ShortPinValue", 5),
                            ("LongPinValue", 6),
                            ("LegacyShort", 3),
                            ("LegacyLong", 4),
                            ("ExactOwnedOuter", 2),
                            ("ExactOwnedInner", 8),
                        ],
                        vec![],
                    )],
                ),
            )
        }
        // The named root itself is the iterated endpoint, filtered by its
        // nearest legacy position.
        "empty_root" => build(
            input(vec![string("Value")]),
            input(vec![string("Value")]),
            output(vec![
                string("OwnedValue"),
                string("LegacyPinnedValue"),
                int("OwnedPosition"),
                int("LegacyNearest"),
                int("LegacyAliasPosition"),
            ]),
            vec![
                (1, owned_field(named(), &["Value"], &[])),
                (2, owned_position(named(), &[])),
                (3, legacy_position(&[])),
                (4, legacy_position(&["Ref"])),
                (5, legacy_field(&["Value"], Some(&[]))),
                (6, constant(Value::Int(1))),
                (7, greater_than(3, 6)),
            ],
            scope(
                "",
                named(),
                &[],
                Some(7),
                &[
                    ("OwnedValue", 1),
                    ("LegacyPinnedValue", 5),
                    ("OwnedPosition", 2),
                    ("LegacyNearest", 3),
                    ("LegacyAliasPosition", 4),
                ],
                vec![],
            ),
        ),
        // The inner scope re-iterates the same named `Rows` with the same
        // filter as its parent.
        "same_owner" => build(
            input(vec![value_rows()]),
            input(vec![value_rows()]),
            output(vec![
                string("OuterValue"),
                int("OuterPosition"),
                items(vec![
                    string("SameValue"),
                    int("LegacyPosition"),
                    int("OwnedPosition"),
                ]),
            ]),
            vec![
                (1, owned_field(named(), &["Rows", "Value"], &["Rows"])),
                (2, owned_position(named(), &["Rows"])),
                (3, legacy_position(&["Rows"])),
                (4, constant(Value::Int(1))),
                (5, greater_than(3, 4)),
            ],
            scope(
                "",
                named(),
                &["Rows"],
                Some(5),
                &[("OuterValue", 1), ("OuterPosition", 2)],
                vec![scope(
                    "Items",
                    named(),
                    &["Rows"],
                    Some(5),
                    &[
                        ("SameValue", 1),
                        ("LegacyPosition", 3),
                        ("OwnedPosition", 2),
                    ],
                    vec![],
                )],
            ),
        ),
        // Primary rows contain a scope over the nonrepeating named root,
        // which reads relative, framed and owned values beside legacy positions.
        "singleton" => build(
            input(vec![string("Value"), value_rows()]),
            input(vec![string("Value")]),
            output(vec![
                string("OuterValue"),
                int("OuterPosition"),
                items(vec![
                    string("OwnedSingleton"),
                    string("RelativeSingleton"),
                    string("FramedOuter"),
                    int("LegacyNearest"),
                    int("LegacyAliasPosition"),
                    int("ExactOwnedOuter"),
                ]),
            ]),
            vec![
                (
                    1,
                    owned_field(SourceOwner::Primary, &["Rows", "Value"], &["Rows"]),
                ),
                (2, owned_position(SourceOwner::Primary, &["Rows"])),
                (3, legacy_position(&["Rows"])),
                (4, constant(Value::Int(1))),
                (5, greater_than(3, 4)),
                (6, constant(Value::Bool(true))),
                (7, owned_field(named(), &["Value"], &[])),
                (8, legacy_field(&["Value"], None)),
                (9, legacy_field(&["Value"], Some(&["Rows"]))),
                (10, legacy_position(&[])),
                (11, legacy_position(&["Ref"])),
            ],
            scope(
                "",
                SourceOwner::Primary,
                &["Rows"],
                Some(5),
                &[("OuterValue", 1), ("OuterPosition", 2)],
                vec![scope(
                    "Items",
                    named(),
                    &[],
                    Some(6),
                    &[
                        ("OwnedSingleton", 7),
                        ("RelativeSingleton", 8),
                        ("FramedOuter", 9),
                        ("LegacyNearest", 10),
                        ("LegacyAliasPosition", 11),
                        ("ExactOwnedOuter", 2),
                    ],
                    vec![],
                )],
            ),
        ),
        // Named rows filtered by raw legacy position > 2, binding owned and
        // legacy positions.
        "raw_compact" => build(
            input(vec![value_rows()]),
            input(vec![value_rows()]),
            output(vec![
                string("Value"),
                int("OwnedPosition"),
                int("LegacyPosition"),
            ]),
            vec![
                (1, owned_field(named(), &["Rows", "Value"], &["Rows"])),
                (2, owned_position(named(), &["Rows"])),
                (3, legacy_position(&["Rows"])),
                (4, constant(Value::Int(2))),
                (5, greater_than(3, 4)),
            ],
            scope(
                "",
                named(),
                &["Rows"],
                Some(5),
                &[("Value", 1), ("OwnedPosition", 2), ("LegacyPosition", 3)],
                vec![],
            ),
        ),
        _ => panic!("authored model"),
    }
}
