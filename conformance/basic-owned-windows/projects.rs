//! Genuine public Project definitions for owned windows. Authored source
//! only: no mapping, bound, key, predicate or position is evaluated here.
use super::*;
use mapping::{
    IterationOutput, NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef,
    ScopeConstruction, SequenceWindow, SortFilterOrder, SourceOwner,
};

pub const NAMED: &str = "Ref";

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 10] = [
    "operators",
    "filter_then_sort",
    "sort_then_filter",
    "nested_parent_bounds",
    "empty_bounds_eager",
    "zero_window_non_bool",
    "invalid_bound",
    "fts_predicate_priority",
    "stf_key_priority",
    "first_predicate_priority",
];

const PRIMARY: SourceOwner = SourceOwner::Primary;

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

fn owned_position(collection: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: PRIMARY,
        collection: Some(path(collection)),
    })
}

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn int_constant(value: i64) -> Node {
    constant(Value::Int(value))
}

fn text(value: &str) -> Node {
    constant(Value::String(value.into()))
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

fn when(condition: u32, then: u32, else_: u32) -> Node {
    Node::If {
        condition,
        then,
        else_,
    }
}

/// Every model declares one source shape: root `Value` and `Count`, an
/// embedded `Ref` group colliding with the named owner, nested repeated rows
/// with their own `Count` fields, an empty repetition and a `Details` group.
fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Value"),
            int("Count"),
            SchemaNode::group(
                NAMED,
                vec![
                    string("Value"),
                    SchemaNode::group(
                        "Rows",
                        vec![
                            int("Value"),
                            bool_("Flag"),
                            string("ID"),
                            int("Tag"),
                            SchemaNode::group("Rows", vec![int("Value"), string("ID")]).repeating(),
                        ],
                    )
                    .repeating(),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    int("Value"),
                    bool_("Flag"),
                    string("ID"),
                    int("Tag"),
                    int("Count"),
                    SchemaNode::group("Rows", vec![int("Value"), string("ID"), int("Count")])
                        .repeating(),
                ],
            )
            .repeating(),
            SchemaNode::group("EmptyRows", vec![int("Value")]).repeating(),
            SchemaNode::group("Details", vec![int("Value")]),
        ],
    )
}

/// Nodes 1–8 and 90 of every model: row value, owned position, constant 1,
/// the position > 1 predicate, root value, legacy position, row ID and Tag,
/// and the root `Count`.
fn base() -> Vec<(u32, Node)> {
    vec![
        (1, owned_field(PRIMARY, &["Rows", "Value"], &["Rows"])),
        (2, owned_position(&["Rows"])),
        (3, int_constant(1)),
        (4, call("greater_than", &[2, 3])),
        (5, owned_field(PRIMARY, &["Value"], &[])),
        (
            6,
            Node::Position {
                collection: path(&["Rows"]),
            },
        ),
        (7, owned_field(PRIMARY, &["Rows", "ID"], &["Rows"])),
        (8, owned_field(PRIMARY, &["Rows", "Tag"], &["Rows"])),
        (90, owned_field(PRIMARY, &["Count"], &[])),
    ]
}

/// `base` plus the priority controls: node 25 raises `predicate_message` from
/// the second row (true otherwise); nodes 0, 24, 26 and 28 raise "bound",
/// "predicate", "target" and "key".
fn priority(predicate_message: &str) -> Vec<(u32, Node)> {
    let mut nodes = base();
    nodes.extend([
        (20, int_constant(2)),
        (21, call("equal", &[2, 20])),
        (23, text(predicate_message)),
        (22, raise(23)),
        (27, constant(Value::Bool(true))),
        (25, when(21, 22, 27)),
        (0, raise(101)),
        (101, text("bound")),
        (24, raise(102)),
        (102, text("predicate")),
        (26, raise(103)),
        (103, text("target")),
        (28, raise(104)),
        (104, text("key")),
    ]);
    nodes
}

fn row_fields() -> Vec<SchemaNode> {
    vec![int("Value"), int("Position"), string("ID")]
}

fn rows_target(name: &str) -> SchemaNode {
    SchemaNode::group(name, row_fields()).repeating()
}

const ROW: [(&str, u32); 3] = [("Value", 1), ("Position", 2), ("ID", 7)];
/// `ROW` with the raising node 26 as the value.
const RAISING_VALUE: [(&str, u32); 3] = [("Value", 26), ("Position", 2), ("ID", 7)];

/// One owned scope over `collection` with explicit `windows`; an unsorted
/// scope keeps the filter-then-sort order.
fn windowed(
    target_field: &str,
    collection: &[&str],
    windows: Vec<SequenceWindow>,
    bindings: &[(&str, u32)],
) -> Scope {
    Scope {
        target_field: target_field.into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: PRIMARY,
                path: path(collection),
            },
            output_path: None,
        },
        sort_filter_order: SortFilterOrder::FilterThenSort,
        windows,
        bindings: bindings
            .iter()
            .map(|(target_field, node)| Binding {
                target_field: (*target_field).into(),
                node: *node,
            })
            .collect(),
        ..Scope::default()
    }
}

/// A primary `Rows` root scope binding `ROW`.
fn rows(windows: Vec<SequenceWindow>) -> Scope {
    windowed("", &["Rows"], windows, &ROW)
}

fn sorted(mut scope: Scope, key: u32, descending: bool, order: SortFilterOrder) -> Scope {
    scope.sort_by = Some(key);
    scope.sort_descending = descending;
    scope.sort_filter_order = order;
    scope
}

fn filtered(mut scope: Scope, filter: u32) -> Scope {
    scope.filter = Some(filter);
    scope
}

fn first(mut scope: Scope) -> Scope {
    scope.iteration_output = IterationOutput::First;
    scope
}

fn extra(name: &str, schema: SchemaNode, root: Scope) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: None,
        schema,
        options: Default::default(),
        root,
    }
}

fn build(
    target: SchemaNode,
    nodes: Vec<(u32, Node)>,
    root: Scope,
    extra_targets: Vec<NamedTarget>,
    named: bool,
) -> Project {
    let extra_sources = if named {
        vec![NamedSource {
            name: NAMED.into(),
            path: "unused-windows-host-input.json".into(),
            schema: input(),
            options: Default::default(),
            dynamic_path: None,
        }]
    } else {
        vec![]
    };
    Project {
        source: input(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources,
        extra_targets,
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}

/// The operator catalogue: one target per window operator and endpoint.
fn operators() -> Project {
    use SequenceWindow::{First, From, FromTo, Last, SkipFirst};
    let mut nodes = base();
    nodes.extend([
        (91, int_constant(0)),
        (92, int_constant(3)),
        (93, int_constant(999)),
        // 2.9 truncates to a count of 2.
        (
            94,
            constant(Value::Float(f64::from_bits(0x4007_3333_3333_3333))),
        ),
        (95, text(" 3 ")),
        (96, int_constant(-5)),
        (97, constant(Value::Bool(false))),
        (
            98,
            owned_field(SourceOwner::Named { name: NAMED.into() }, &["Value"], &[]),
        ),
        (99, when(97, 98, 91)),
        (100, raise(101)),
        (101, text("unreached lazy-empty target")),
    ]);
    let repeated =
        |name: &str, windows: Vec<SequenceWindow>| extra(name, rows_target(name), rows(windows));
    let single = |name: &str, windows: Vec<SequenceWindow>| {
        extra(
            name,
            SchemaNode::group(name, row_fields()),
            first(rows(windows)),
        )
    };
    let mut scalar = rows(vec![First { count: 95 }]);
    scalar.construction = ScopeConstruction::Scalar { value: 1 };
    scalar.bindings = vec![];
    let lazy = filtered(
        windowed("", &["Rows"], vec![First { count: 99 }], &[("Value", 100)]),
        97,
    );
    let extras = vec![
        repeated(
            "OrderReversed",
            vec![First { count: 90 }, SkipFirst { count: 3 }],
        ),
        repeated("From", vec![From { position: 92 }]),
        repeated("FromZero", vec![From { position: 91 }]),
        repeated(
            "Range",
            vec![FromTo {
                first: 90,
                last: 92,
            }],
        ),
        repeated(
            "ReversedRange",
            vec![FromTo {
                first: 92,
                last: 90,
            }],
        ),
        repeated("Last", vec![Last { count: 90 }]),
        repeated("ZeroFirst", vec![First { count: 91 }]),
        repeated("ZeroLast", vec![Last { count: 91 }]),
        repeated("OversizedSkip", vec![SkipFirst { count: 93 }]),
        repeated("OversizedLast", vec![Last { count: 93 }]),
        repeated("FloatFirst", vec![First { count: 94 }]),
        repeated("NegativeFirst", vec![First { count: 96 }]),
        extra(
            "Scalar",
            SchemaNode::scalar("Scalar", ScalarType::Int).repeating(),
            scalar,
        ),
        single("SelectedFirst", vec![SkipFirst { count: 3 }]),
        single("PlainFirst", vec![]),
        single("EmptyFirst", vec![First { count: 91 }]),
        extra(
            "LazyEmpty",
            SchemaNode::group("LazyEmpty", vec![int("Value")]).repeating(),
            lazy,
        ),
    ];
    build(
        rows_target("Output"),
        nodes,
        rows(vec![SkipFirst { count: 3 }, First { count: 90 }]),
        extras,
        true,
    )
}

pub fn project(model: &str) -> Project {
    use SequenceWindow::{First, From, FromTo, Last, SkipFirst};
    use SortFilterOrder::{FilterThenSort as Fts, SortThenFilter as Stf};
    match model {
        "operators" => operators(),
        // Filter, ascending value sort, then skip one and keep the last root
        // `Count`; `RawKey` sorts by descending owned position.
        "filter_then_sort" => {
            let mut nodes = base();
            nodes.extend([(91, int_constant(3)), (92, call("greater_than", &[2, 91]))]);
            let raw_key = filtered(sorted(rows(vec![From { position: 90 }]), 2, true, Fts), 92);
            build(
                rows_target("Output"),
                nodes,
                filtered(
                    sorted(
                        rows(vec![SkipFirst { count: 3 }, Last { count: 90 }]),
                        1,
                        false,
                        Fts,
                    ),
                    4,
                ),
                vec![extra("RawKey", rows_target("RawKey"), raw_key)],
                false,
            )
        }
        "sort_then_filter" => build(
            rows_target("Output"),
            base(),
            filtered(
                sorted(
                    rows(vec![SkipFirst { count: 3 }, First { count: 90 }]),
                    1,
                    false,
                    Stf,
                ),
                4,
            ),
            vec![],
            false,
        ),
        // The last root `Count` parents; `Kids` keep the parent's position
        // many children and `Identity` re-iterates the current row.
        "nested_parent_bounds" => {
            let mut nodes = base();
            nodes.extend([
                (
                    21,
                    owned_field(PRIMARY, &["Rows", "Rows", "Value"], &["Rows", "Rows"]),
                ),
                (22, owned_position(&["Rows", "Rows"])),
            ]);
            let mut fields = row_fields();
            fields.extend([
                SchemaNode::group(
                    "Kids",
                    vec![int("Value"), int("Position"), int("ParentPosition")],
                )
                .repeating(),
                SchemaNode::group("Identity", vec![int("Value"), int("Position")]).repeating(),
            ]);
            let mut root = rows(vec![Last { count: 90 }]);
            root.children = vec![
                windowed(
                    "Kids",
                    &["Rows", "Rows"],
                    vec![First { count: 2 }],
                    &[("Value", 21), ("Position", 22), ("ParentPosition", 2)],
                ),
                windowed(
                    "Identity",
                    &["Rows"],
                    vec![First { count: 3 }],
                    &[("Value", 1), ("Position", 2)],
                ),
            ];
            build(
                SchemaNode::group("Output", fields).repeating(),
                nodes,
                root,
                vec![],
                false,
            )
        }
        // Both range bounds raise, at node zero and node u32::MAX.
        "empty_bounds_eager" => {
            let mut nodes = base();
            nodes.extend([
                (91, int_constant(0)),
                (0, raise(101)),
                (101, text("first bound")),
                (u32::MAX, raise(102)),
                (102, text("last bound")),
                (26, raise(103)),
                (103, text("unreached target")),
            ]);
            build(
                rows_target("Output"),
                nodes,
                windowed(
                    "",
                    &["Rows"],
                    vec![
                        First { count: 91 },
                        FromTo {
                            first: 0,
                            last: u32::MAX,
                        },
                    ],
                    &RAISING_VALUE,
                ),
                vec![],
                false,
            )
        }
        // Node zero is an Int predicate.
        "zero_window_non_bool" => {
            let mut nodes = base();
            nodes.extend([
                (91, int_constant(0)),
                (0, int_constant(7)),
                (26, raise(101)),
                (101, text("unreached target")),
            ]);
            build(
                rows_target("Output"),
                nodes,
                filtered(
                    windowed("", &["Rows"], vec![First { count: 91 }], &RAISING_VALUE),
                    0,
                ),
                vec![],
                false,
            )
        }
        // The `Last` bound at node u32::MAX is a Bool.
        "invalid_bound" => {
            let mut nodes = base();
            nodes.extend([
                (91, int_constant(0)),
                (u32::MAX, constant(Value::Bool(false))),
                (24, raise(101)),
                (101, text("later predicate")),
                (26, raise(102)),
                (102, text("later target")),
            ]);
            build(
                rows_target("Output"),
                nodes,
                filtered(
                    windowed(
                        "",
                        &["Rows"],
                        vec![First { count: 91 }, Last { count: u32::MAX }],
                        &RAISING_VALUE,
                    ),
                    24,
                ),
                vec![],
                false,
            )
        }
        "fts_predicate_priority" => build(
            rows_target("Output"),
            priority("later predicate"),
            filtered(
                sorted(
                    windowed("", &["Rows"], vec![Last { count: 0 }], &RAISING_VALUE),
                    28,
                    false,
                    Fts,
                ),
                25,
            ),
            vec![],
            false,
        ),
        // Node 30 raises "later key" from the second row.
        "stf_key_priority" => {
            let mut nodes = priority("later key");
            nodes.push((30, when(21, 22, 1)));
            build(
                rows_target("Output"),
                nodes,
                filtered(
                    sorted(
                        windowed("", &["Rows"], vec![Last { count: 0 }], &RAISING_VALUE),
                        30,
                        false,
                        Stf,
                    ),
                    24,
                ),
                vec![],
                false,
            )
        }
        // A nonrepeating Group target takes the implicit first accepted row.
        "first_predicate_priority" => build(
            SchemaNode::group("Output", row_fields()),
            priority("later predicate"),
            first(filtered(
                windowed("", &["Rows"], vec![], &RAISING_VALUE),
                25,
            )),
            vec![],
            false,
        ),
        _ => panic!("authored model"),
    }
}
