//! Genuine public Projects for owned join tuples (#449).
//!
//! Primary and the named `Lookup` endpoint share one schema, so every owned
//! collection also exists under the other owner. Rows carry an `id` join key
//! with duplicates and nulls. Expected results live in `cases.rs`.
use super::*;
use mapping::{
    JoinId, NamedSource, NamedTarget, OwnedJoinConditions, OwnedJoinField, OwnedJoinKey,
    OwnedJoinPlan, OwnedJoinSource, OwnedJoinSourcePosition, OwnedSourceField, OwnedSourceRef,
    ScopeConstruction, SequenceWindow, SortFilterOrder, SourceOwner,
};

pub const LOOKUP: &str = "Lookup";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "J01-same-path-rows",
    "J02-composite-and-singleton-stages",
    "J03-controls-and-positions",
    "J04-correlated-active-orders",
    "J05-first-and-scalar-output",
    "J06-static-descendants-and-windows",
    "J07-filter-policy",
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
pub fn source(name: &str, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(name),
        path: path(parts),
    }
}
pub fn join_field(join: u64, name: &str, collection: &[&str], relative: &[&str]) -> Node {
    Node::OwnedJoinField(OwnedJoinField {
        join: JoinId::new(join),
        collection: source(name, collection),
        path: path(relative),
    })
}
pub fn join_position(join: u64, name: &str, collection: &[&str]) -> Node {
    Node::OwnedJoinSourcePosition(OwnedJoinSourcePosition {
        join: JoinId::new(join),
        collection: source(name, collection),
    })
}
pub fn key(name: &str, collection: &[&str], left: &[&str], right: &[&str]) -> OwnedJoinKey {
    OwnedJoinKey::new(source(name, collection), path(left), path(right))
}
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::group("Header", vec![string("code")]),
            SchemaNode::group("Rows", vec![int("id"), string("code"), string("name")]).repeating(),
            SchemaNode::group(
                "Orders",
                vec![
                    int("id"),
                    string("code"),
                    SchemaNode::group("Lines", vec![int("order"), string("sku")]).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn tuple_fields() -> Vec<SchemaNode> {
    vec![
        string("left"),
        string("right"),
        int("left_at"),
        int("right_at"),
        int("tuple"),
        SchemaNode::group("Pair", vec![string("left"), string("right")]),
    ]
}
fn insert(project: &mut Project, id: u32, node: Node) {
    project.graph.nodes.insert(id, node);
}
fn call(project: &mut Project, id: u32, function: &str, args: &[u32]) {
    insert(
        project,
        id,
        Node::Call {
            function: function.into(),
            args: args.to_vec(),
        },
    );
}
fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}
/// Primary `Rows` joined to the same-path `Lookup` `Rows` by `id`, or the
/// reverse when `lookup_first`.
pub fn rows_plan(lookup_first: bool) -> OwnedJoinPlan {
    let (first, second) = if lookup_first {
        (LOOKUP, "Primary")
    } else {
        ("Primary", LOOKUP)
    };
    OwnedJoinPlan::new(
        OwnedJoinSource::new(source(first, &["Rows"])),
        OwnedJoinSource::new(source(second, &["Rows"])),
        OwnedJoinConditions::new(key(first, &["Rows"], &["id"], &["id"])),
    )
    .expect("authored owned plan")
}
/// Tuple reads for `join`: Primary and Lookup row codes at `base + 1/2`, their
/// raw positions at `base + 3/4` and the flattened position at `base + 5`.
fn tuple_nodes(project: &mut Project, join: u64, base: u32) {
    insert(
        project,
        base + 1,
        join_field(join, "Primary", &["Rows"], &["code"]),
    );
    insert(
        project,
        base + 2,
        join_field(join, LOOKUP, &["Rows"], &["code"]),
    );
    insert(project, base + 3, join_position(join, "Primary", &["Rows"]));
    insert(project, base + 4, join_position(join, LOOKUP, &["Rows"]));
    insert(
        project,
        base + 5,
        Node::JoinPosition {
            join: JoinId::new(join),
        },
    );
}
/// A `Row` scope binding the five tuple reads starting at `base + 1`.
fn rows(join: u64, plan: OwnedJoinPlan, base: u32) -> Scope {
    Scope {
        target_field: "Row".into(),
        iteration: ScopeIteration::OwnedInnerJoin {
            id: JoinId::new(join),
            plan,
        },
        bindings: ["left", "right", "left_at", "right_at", "tuple"]
            .into_iter()
            .zip(base + 1..)
            .map(|(field, node)| bind(field, node))
            .collect(),
        ..Scope::default()
    }
}

/// Shared nodes: join 1 reads 1-5 and join 2 reads 11-15; constants 40-49.
/// The Primary target fills `Row` with join 1 and the audit target with join 2;
/// profiles replace or extend them and add their nodes from 50.
pub fn base() -> Project {
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Row", tuple_fields()).repeating(),
            SchemaNode::group(
                "Order",
                vec![
                    int("id"),
                    SchemaNode::group("Row", tuple_fields()).repeating(),
                ],
            )
            .repeating(),
            SchemaNode::group("Best", tuple_fields()),
            string("Codes").repeating(),
        ],
    );
    let audit = SchemaNode::group(
        "Audit",
        vec![
            SchemaNode::group("Row", tuple_fields()).repeating(),
            string("Value"),
        ],
    );
    let mut project = Project {
        source: endpoint_schema("Input"),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: LOOKUP.into(),
            path: "lookup.json".into(),
            schema: endpoint_schema(LOOKUP),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: audit,
            options: Default::default(),
            root: Scope {
                children: vec![rows(2, rows_plan(true), 10)],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            children: vec![rows(1, rows_plan(false), 0)],
            ..Scope::default()
        },
    };
    tuple_nodes(&mut project, 1, 0);
    tuple_nodes(&mut project, 2, 10);
    for (id, value) in [
        (40, Value::String("-".into())),
        (41, Value::Int(1)),
        (42, Value::Int(2)),
        (43, Value::Int(3)),
        (44, Value::String("b".into())),
        (45, Value::Bool(true)),
        (46, Value::String("not boolean".into())),
        (47, Value::String("audit".into())),
    ] {
        insert(&mut project, id, Node::Const { value });
    }
    project
}

pub fn project(profile: &str) -> Project {
    let mut p = base();
    match profile {
        // Same-path sources in both orders keep left-major duplicate order.
        "J01-same-path-rows" => {}
        // Composite keys, then a named singleton keyed on the Primary row code;
        // audit starts from a Primary singleton.
        "J02-composite-and-singleton-stages" => {
            let plan = OwnedJoinPlan::new(
                OwnedJoinSource::new(source("Primary", &["Rows"])),
                OwnedJoinSource::new(source(LOOKUP, &["Rows"])),
                OwnedJoinConditions::new(key("Primary", &["Rows"], &["id"], &["id"])).and(key(
                    "Primary",
                    &["Rows"],
                    &["code"],
                    &["code"],
                )),
            )
            .and_then(|plan| {
                plan.then(
                    OwnedJoinSource::singleton(source(LOOKUP, &["Header", "code"])),
                    OwnedJoinConditions::new(key("Primary", &["Rows"], &["code"], &[])),
                )
            })
            .expect("authored owned plan");
            p.root.children = vec![rows(1, plan, 0)];
            let audit = OwnedJoinPlan::new(
                OwnedJoinSource::singleton(source("Primary", &["Header", "code"])),
                OwnedJoinSource::new(source(LOOKUP, &["Rows"])),
                OwnedJoinConditions::new(key("Primary", &["Header", "code"], &[], &["code"])),
            )
            .expect("authored owned plan");
            insert(
                &mut p,
                11,
                join_field(2, "Primary", &["Header", "code"], &[]),
            );
            insert(&mut p, 13, join_position(2, "Primary", &["Header", "code"]));
            p.extra_targets[0].root.children = vec![rows(2, audit, 10)];
        }
        // Sort-then-filter sees sorted positions and a parent window bound;
        // audit filters original positions before a two-key sort.
        "J03-controls-and-positions" => {
            call(&mut p, 50, "not_equal", &[5, 42]);
            call(&mut p, 51, "not_equal", &[15, 42]);
            let primary = &mut p.root.children[0];
            primary.filter = Some(50);
            primary.sort_by = Some(4);
            primary.sort_descending = true;
            primary.windows = vec![SequenceWindow::First { count: 43 }];
            let audit = &mut p.extra_targets[0].root.children[0];
            audit.filter = Some(51);
            audit.sort_filter_order = SortFilterOrder::FilterThenSort;
            audit.sort_by = Some(11);
            audit.sort_then_by = vec![mapping::SortKey {
                node: 12,
                descending: true,
            }];
        }
        // Inside an owned Primary order, a Primary `Orders` source is that
        // order and `Orders/Lines` walks only its lines; audit flattens every
        // Lookup order's lines from the root.
        "J04-correlated-active-orders" => {
            let plan = OwnedJoinPlan::new(
                OwnedJoinSource::new(source("Primary", &["Orders"])),
                OwnedJoinSource::new(source("Primary", &["Orders", "Lines"])),
                OwnedJoinConditions::new(key("Primary", &["Orders"], &["id"], &["order"])),
            )
            .and_then(|plan| {
                plan.then(
                    OwnedJoinSource::new(source(LOOKUP, &["Rows"])),
                    OwnedJoinConditions::new(key(
                        "Primary",
                        &["Orders", "Lines"],
                        &["sku"],
                        &["code"],
                    )),
                )
            })
            .expect("authored owned plan");
            insert(
                &mut p,
                1,
                join_field(1, "Primary", &["Orders", "Lines"], &["sku"]),
            );
            insert(&mut p, 3, join_position(1, "Primary", &["Orders"]));
            insert(&mut p, 4, join_position(1, "Primary", &["Orders", "Lines"]));
            insert(
                &mut p,
                50,
                Node::OwnedSourceField(OwnedSourceField {
                    source: source("Primary", &["Orders", "id"]),
                    frame: None,
                }),
            );
            p.root.children = vec![Scope {
                target_field: "Order".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: source("Primary", &["Orders"]),
                    output_path: None,
                },
                bindings: vec![bind("id", 50)],
                children: vec![rows(1, plan, 0)],
                ..Scope::default()
            }];
            let audit = OwnedJoinPlan::new(
                OwnedJoinSource::new(source(LOOKUP, &["Orders", "Lines"])),
                OwnedJoinSource::new(source(LOOKUP, &["Orders"])),
                OwnedJoinConditions::new(key(LOOKUP, &["Orders", "Lines"], &["order"], &["id"])),
            )
            .expect("authored owned plan");
            insert(
                &mut p,
                11,
                join_field(2, LOOKUP, &["Orders", "Lines"], &["sku"]),
            );
            insert(&mut p, 12, join_field(2, LOOKUP, &["Orders"], &["code"]));
            insert(&mut p, 13, join_position(2, LOOKUP, &["Orders", "Lines"]));
            insert(&mut p, 14, join_position(2, LOOKUP, &["Orders"]));
            p.extra_targets[0].root.children = vec![rows(2, audit, 10)];
        }
        // The best tuple by descending Lookup position, and one scalar per
        // tuple of an independent join over the same sources.
        "J05-first-and-scalar-output" => {
            let mut best = rows(1, rows_plan(false), 0);
            best.target_field = "Best".into();
            best.iteration_output = mapping::IterationOutput::First;
            best.sort_by = Some(4);
            best.sort_descending = true;
            tuple_nodes(&mut p, 3, 20);
            call(&mut p, 50, "concat", &[21, 40, 22]);
            p.root.children = vec![
                best,
                Scope {
                    target_field: "Codes".into(),
                    iteration: ScopeIteration::OwnedInnerJoin {
                        id: JoinId::new(3),
                        plan: rows_plan(false),
                    },
                    construction: ScopeConstruction::Scalar { value: 50 },
                    ..Scope::default()
                },
            ];
            p.extra_targets[0].root = Scope {
                bindings: vec![bind("Value", 47)],
                ..Scope::default()
            };
        }
        // A static `Pair` descendant reads the tuple; parent window bounds
        // select the trailing Primary tuples and an inclusive audit range.
        "J06-static-descendants-and-windows" => {
            for (scope, base) in [
                (&mut p.root.children[0], 0),
                (&mut p.extra_targets[0].root.children[0], 10),
            ] {
                scope.bindings = vec![bind("tuple", base + 5)];
                scope.children = vec![Scope {
                    target_field: "Pair".into(),
                    bindings: vec![bind("left", base + 1), bind("right", base + 2)],
                    ..Scope::default()
                }];
            }
            p.root.children[0].windows = vec![
                SequenceWindow::SkipFirst { count: 41 },
                SequenceWindow::Last { count: 42 },
            ];
            p.extra_targets[0].root.children[0].windows = vec![SequenceWindow::FromTo {
                first: 42,
                last: 43,
            }];
        }
        // An ordinary Boolean filter keeps two tuples; the audit filter's
        // string result at tuple 2 is a typed Boolean failure.
        "J07-filter-policy" => {
            call(&mut p, 50, "equal", &[1, 44]);
            p.root.children[0].filter = Some(50);
            call(&mut p, 51, "equal", &[15, 42]);
            insert(
                &mut p,
                52,
                Node::If {
                    condition: 51,
                    then: 46,
                    else_: 45,
                },
            );
            p.extra_targets[0].root.children[0].filter = Some(52);
        }
        _ => panic!("authored profile {profile}"),
    }
    p
}
