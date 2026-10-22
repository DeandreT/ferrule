//! Genuine public Projects for owned collection aggregates (#447).
//!
//! Primary and the named `Ref` endpoint share one schema, so every owned
//! collection also exists under the other owner; their values are disjoint.
//! `Batch` is a repeated named root. Expected results live in `cases.rs`.
use super::*;
use mapping::{
    AggregateOp, NamedSource, NamedTarget, OwnedAggregate, OwnedAggregateValue, OwnedPosition,
    OwnedSourceField, OwnedSourceRef, SourceOwner,
};

pub const REF: &str = "Ref";
pub const BATCH: &str = "Batch";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "C01-every-operator-root",
    "C02-nested-and-scalar-collections",
    "C03-computed-values-and-parent-arguments",
    "C04-grouped-subset",
    "C05-row-anchored-walks",
    "C06-block-members",
    "C07-repeated-named-root",
    "C08-evaluation-order-and-failures",
    "C09-empty-and-value-policies",
];

/// The reducer fields beside `Value`, in target schema order.
pub const REDUCERS: &[(&str, AggregateOp)] = &[
    ("Count", AggregateOp::Count),
    ("Sum", AggregateOp::Sum),
    ("Avg", AggregateOp::Avg),
    ("Min", AggregateOp::Min),
    ("Max", AggregateOp::Max),
    ("Join", AggregateOp::Join),
    ("ItemAt", AggregateOp::ItemAt),
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
pub fn field(name: &str, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: source(name, parts),
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
pub fn aggregate(
    function: AggregateOp,
    name: &str,
    collection: &[&str],
    value: &[&str],
    arg: Option<u32>,
) -> Node {
    Node::OwnedAggregate(OwnedAggregate {
        function,
        collection: source(name, collection),
        value: OwnedAggregateValue::Path(path(value)),
        arg,
    })
}
pub fn computed(
    function: AggregateOp,
    name: &str,
    collection: &[&str],
    expression: u32,
    arg: Option<u32>,
) -> Node {
    Node::OwnedAggregate(OwnedAggregate {
        function,
        collection: source(name, collection),
        value: OwnedAggregateValue::Expression(expression),
        arg,
    })
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
            string("Note"),
            SchemaNode::group(
                "Rows",
                vec![
                    string("Code"),
                    string("Name"),
                    int("Rank"),
                    string("Price"),
                    SchemaNode::group("Lines", vec![string("Sku"), int("Qty")]).repeating(),
                    string("Tags").repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
pub fn batch_schema() -> SchemaNode {
    SchemaNode::group(BATCH, vec![string("Code"), string("Name")]).repeating()
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
/// Bind reducer `field` of the Primary target to node `92 + i` and of the
/// audit target to node `102 + i`, where `i` is its `REDUCERS` ordinal.
fn bind_reducer(project: &mut Project, field: &str, audit: bool) -> u32 {
    let ordinal = REDUCERS
        .iter()
        .position(|(name, _)| *name == field)
        .expect("authored reducer field") as u32;
    let (scope, node) = if audit {
        (&mut project.extra_targets[0].root, 102 + ordinal)
    } else {
        (&mut project.root, 92 + ordinal)
    };
    scope.bindings.push(bind(field, node));
    node
}
/// A Primary `Items` child scope iterating owned Primary rows.
fn items(bindings: Vec<Binding>) -> Scope {
    Scope {
        target_field: "Items".into(),
        iteration: ScopeIteration::OwnedSource {
            source: source("Primary", &["Rows"]),
            output_path: None,
        },
        bindings,
        ..Scope::default()
    }
}

/// Shared nodes: Primary row reads 1-4, Ref row reads 11-14, root notes 21-22,
/// legacy reads 31-32 and constants 40-49. The Primary target binds `Value` to
/// node 90 and the audit target binds `Value` to node 91; reducer fields bind
/// 92-98 and 102-108; profiles add their nodes from 50.
pub fn base() -> Project {
    let reducers = || REDUCERS.iter().map(|(name, _)| string(name));
    let target = SchemaNode::group(
        "Output",
        std::iter::once(string("Value"))
            .chain(reducers())
            .chain([SchemaNode::group("Items", vec![string("Code"), string("Value")]).repeating()])
            .collect(),
    );
    let audit = SchemaNode::group(
        "Audit",
        std::iter::once(string("Value")).chain(reducers()).collect(),
    );
    let mut project = Project {
        source: endpoint_schema("Input"),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: REF.into(),
                path: "ref.json".into(),
                schema: endpoint_schema(REF),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: BATCH.into(),
                path: "batch.json".into(),
                schema: batch_schema(),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: audit,
            options: Default::default(),
            root: Scope {
                bindings: vec![bind("Value", 91)],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            bindings: vec![bind("Value", 90)],
            ..Scope::default()
        },
    };
    for (offset, name) in [(0, "Primary"), (10, REF)] {
        insert(
            &mut project,
            offset + 1,
            field(name, &["Rows", "Code"], None),
        );
        insert(
            &mut project,
            offset + 2,
            field(name, &["Rows", "Name"], None),
        );
        insert(
            &mut project,
            offset + 3,
            field(name, &["Rows", "Rank"], Some(&["Rows"])),
        );
        insert(&mut project, offset + 4, position(name, &["Rows"]));
    }
    insert(&mut project, 21, field("Primary", &["Note"], None));
    insert(&mut project, 22, field(REF, &["Note"], None));
    insert(&mut project, 31, legacy(&["Code"]));
    insert(&mut project, 32, legacy(&["Name"]));
    for (id, value) in [
        (40, Value::String(",".into())),
        (41, Value::String("+".into())),
        (42, Value::String("/".into())),
        (43, Value::String("|".into())),
        (44, Value::Int(2)),
        (45, Value::Float(2.6)),
        (46, Value::String("-".into())),
        (47, Value::Int(10)),
        (48, Value::String("X".into())),
        (49, Value::String("bad".into())),
    ] {
        insert(&mut project, id, Node::Const { value });
    }
    project
}

/// Count, Sum, Avg, Min, Max, Join and ItemAt over one endpoint's rows.
fn every_reducer(project: &mut Project, name: &str, audit: bool) {
    for (field, function) in REDUCERS {
        let value: &[&str] = match function {
            AggregateOp::Count => &[],
            AggregateOp::Sum | AggregateOp::Avg => &["Rank"],
            AggregateOp::Min | AggregateOp::Max => &["Price"],
            AggregateOp::Join | AggregateOp::ItemAt => &["Name"],
            AggregateOp::DecimalSum
            | AggregateOp::LastPresentText
            | AggregateOp::LastDistinctText => {
                unreachable!("the owned corpus includes only its seven qualified reducers")
            }
        };
        let arg = match function {
            AggregateOp::Join => Some(40),
            AggregateOp::ItemAt => Some(44),
            _ => None,
        };
        let node = bind_reducer(project, field, audit);
        insert(
            project,
            node,
            aggregate(*function, name, &["Rows"], value, arg),
        );
    }
}

/// Count (`60`), named count (`61`), line quantities (`62`) and names (`63`)
/// of the active Primary context, joined by `/` in node 64.
fn context_summary(project: &mut Project) {
    insert(
        project,
        60,
        aggregate(AggregateOp::Count, "Primary", &["Rows"], &[], None),
    );
    insert(
        project,
        61,
        aggregate(AggregateOp::Count, REF, &["Rows"], &[], None),
    );
    insert(
        project,
        62,
        aggregate(
            AggregateOp::Sum,
            "Primary",
            &["Rows", "Lines"],
            &["Qty"],
            None,
        ),
    );
    insert(
        project,
        63,
        aggregate(AggregateOp::Join, "Primary", &["Rows"], &["Name"], Some(41)),
    );
    call(project, 64, "concat", &[60, 42, 61, 42, 62, 42, 63]);
}

pub fn project(profile: &str) -> Project {
    let mut p = base();
    match profile {
        // Each endpoint reduces only its own rows; numeric strings parse.
        "C01-every-operator-root" => {
            every_reducer(&mut p, "Primary", false);
            every_reducer(&mut p, REF, true);
            insert(
                &mut p,
                90,
                aggregate(AggregateOp::Sum, "Primary", &["Rows"], &["Price"], None),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::Sum, REF, &["Rows"], &["Price"], None),
            );
        }
        // Nested walks flatten every crossed row; scalar items read themselves.
        "C02-nested-and-scalar-collections" => {
            for (name, audit) in [("Primary", false), (REF, true)] {
                let count = bind_reducer(&mut p, "Count", audit);
                insert(
                    &mut p,
                    count,
                    aggregate(AggregateOp::Count, name, &["Rows", "Lines"], &[], None),
                );
                let sum = bind_reducer(&mut p, "Sum", audit);
                insert(
                    &mut p,
                    sum,
                    aggregate(AggregateOp::Sum, name, &["Rows", "Lines"], &["Qty"], None),
                );
                let join = bind_reducer(&mut p, "Join", audit);
                insert(
                    &mut p,
                    join,
                    aggregate(AggregateOp::Join, name, &["Rows", "Tags"], &[], Some(41)),
                );
            }
            insert(
                &mut p,
                90,
                aggregate(AggregateOp::Count, "Primary", &["Rows", "Tags"], &[], None),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::Count, REF, &["Rows", "Tags"], &[], None),
            );
        }
        // Item values read each Ref item (and the pinned Primary parent);
        // the index and separators are Primary parent values.
        "C03-computed-values-and-parent-arguments" => {
            insert(
                &mut p,
                50,
                aggregate(AggregateOp::ItemAt, REF, &["Rows"], &["Name"], Some(3)),
            );
            insert(
                &mut p,
                51,
                aggregate(AggregateOp::Join, REF, &["Rows"], &["Code"], Some(1)),
            );
            insert(
                &mut p,
                52,
                computed(AggregateOp::Join, REF, &["Rows"], 53, Some(40)),
            );
            call(&mut p, 53, "concat", &[11, 54, 14, 32]);
            insert(
                &mut p,
                54,
                field("Primary", &["Rows", "Code"], Some(&["Rows"])),
            );
            call(&mut p, 55, "concat", &[50, 43, 51, 43, 52]);
            p.root
                .children
                .push(items(vec![bind("Code", 1), bind("Value", 55)]));
            call(&mut p, 56, "multiply", &[3, 47]);
            call(&mut p, 57, "multiply", &[13, 47]);
            insert(
                &mut p,
                90,
                computed(AggregateOp::Sum, "Primary", &["Rows"], 56, None),
            );
            insert(
                &mut p,
                91,
                computed(AggregateOp::Sum, REF, &["Rows"], 57, None),
            );
        }
        // An owned group exposes only its members to same-owner walks.
        "C04-grouped-subset" => {
            context_summary(&mut p);
            let mut grouped = items(vec![bind("Code", 1), bind("Value", 64)]);
            grouped.group_by = Some(1);
            p.root.children.push(grouped);
            insert(
                &mut p,
                90,
                aggregate(AggregateOp::Count, "Primary", &["Rows"], &[], None),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::Count, REF, &["Rows", "Lines"], &[], None),
            );
        }
        // An ordinary owned row anchors same-owner walks at that row.
        "C05-row-anchored-walks" => {
            context_summary(&mut p);
            p.root
                .children
                .push(items(vec![bind("Code", 1), bind("Value", 64)]));
            insert(
                &mut p,
                90,
                aggregate(
                    AggregateOp::Sum,
                    "Primary",
                    &["Rows", "Lines"],
                    &["Qty"],
                    None,
                ),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::Sum, REF, &["Rows", "Lines"], &["Qty"], None),
            );
        }
        // An owned fixed block exposes only its members.
        "C06-block-members" => {
            context_summary(&mut p);
            let mut blocks = items(vec![bind("Code", 1), bind("Value", 64)]);
            blocks.group_into_blocks = Some(44);
            p.root.children.push(blocks);
            insert(
                &mut p,
                90,
                aggregate(
                    AggregateOp::ItemAt,
                    "Primary",
                    &["Rows"],
                    &["Name"],
                    Some(44),
                ),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::ItemAt, REF, &["Rows"], &["Name"], Some(44)),
            );
        }
        // A repeated named root reduces its items through the empty path.
        "C07-repeated-named-root" => {
            insert(
                &mut p,
                90,
                aggregate(AggregateOp::Join, BATCH, &[], &["Name"], Some(40)),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::ItemAt, BATCH, &[], &["Name"], Some(44)),
            );
            let count = bind_reducer(&mut p, "Count", false);
            insert(
                &mut p,
                count,
                aggregate(AggregateOp::Count, BATCH, &[], &[], None),
            );
        }
        // The walk precedes item values, which precede the parent argument.
        "C08-evaluation-order-and-failures" => {
            call(&mut p, 60, "equal", &[11, 48]);
            insert(&mut p, 61, Node::Raise { message: Some(12) });
            insert(
                &mut p,
                62,
                Node::If {
                    condition: 60,
                    then: 61,
                    else_: 12,
                },
            );
            call(&mut p, 63, "equal", &[21, 49]);
            insert(&mut p, 64, Node::Raise { message: Some(21) });
            insert(
                &mut p,
                65,
                Node::If {
                    condition: 63,
                    then: 64,
                    else_: 40,
                },
            );
            insert(
                &mut p,
                90,
                computed(AggregateOp::Join, REF, &["Rows"], 62, Some(65)),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::Sum, REF, &["Rows"], &["Rank"], None),
            );
            let sum = bind_reducer(&mut p, "Sum", true);
            insert(
                &mut p,
                sum,
                aggregate(AggregateOp::Sum, REF, &["Rows"], &["Price"], None),
            );
        }
        // Ref reducers in the Primary target: absent, empty and Null-valued
        // items keep every legacy reducer policy, and Primary never leaks in.
        "C09-empty-and-value-policies" => {
            for (field, function) in REDUCERS {
                let (value, arg): (&[&str], _) = match function {
                    AggregateOp::Count => (&["Price"], None),
                    AggregateOp::Sum | AggregateOp::Min | AggregateOp::Max => (&["Price"], None),
                    AggregateOp::Avg => (&["Rank"], None),
                    AggregateOp::Join => (&["Code"], Some(46)),
                    AggregateOp::ItemAt => (&["Code"], Some(45)),
                    AggregateOp::DecimalSum
                    | AggregateOp::LastPresentText
                    | AggregateOp::LastDistinctText => {
                        unreachable!("the owned corpus includes only its seven qualified reducers")
                    }
                };
                let node = bind_reducer(&mut p, field, false);
                insert(
                    &mut p,
                    node,
                    aggregate(*function, REF, &["Rows"], value, arg),
                );
            }
            insert(
                &mut p,
                90,
                aggregate(AggregateOp::Avg, "Primary", &["Rows"], &["Price"], None),
            );
            insert(
                &mut p,
                91,
                aggregate(AggregateOp::Count, "Primary", &["Rows"], &[], None),
            );
        }
        _ => panic!("authored profile {profile}"),
    }
    p
}
