//! Singleton join sources select one scalar. A singleton path crossing an
//! inactive repeated level is refused; root and active-parent singletons join.

use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, ScopeIteration};

const JOIN: JoinId = JoinId::new(7);

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn record(fields: &[(&str, &str)]) -> Instance {
    Instance::Group(
        (fields
            .iter()
            .map(|(name, value)| ((*name).into(), Instance::Scalar(string(value))))
            .collect::<Vec<_>>())
        .into(),
    )
}

fn source_schema() -> SchemaNode {
    let strings = |names: &[&str]| {
        names
            .iter()
            .map(|name| SchemaNode::scalar(*name, ScalarType::String))
            .collect::<Vec<_>>()
    };
    SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Header", strings(&["CustomerNr"])),
            SchemaNode::group("Orders", strings(&["Id", "CustomerNr"])).repeating(),
            SchemaNode::group("Customer", strings(&["Number", "Name"])).repeating(),
            SchemaNode::group("Item", strings(&["CustomerNumber", "Sku"])).repeating(),
        ],
    )
}

fn source(orders: &[(&str, &str)]) -> Instance {
    Instance::Group(
        (vec![
            ("Header".into(), record(&[("CustomerNr", "C2")])),
            (
                "Orders".into(),
                Instance::Repeated(
                    orders
                        .iter()
                        .map(|(id, customer)| record(&[("Id", id), ("CustomerNr", customer)]))
                        .collect(),
                ),
            ),
            (
                "Customer".into(),
                Instance::Repeated(vec![
                    record(&[("Number", "C1"), ("Name", "Ada")]),
                    record(&[("Number", "C2"), ("Name", "Grace")]),
                ]),
            ),
            (
                "Item".into(),
                Instance::Repeated(vec![
                    record(&[("CustomerNumber", "C2"), ("Sku", "S1")]),
                    record(&[("CustomerNumber", "C1"), ("Sku", "S2")]),
                    record(&[("CustomerNumber", "C2"), ("Sku", "S3")]),
                ]),
            ),
        ])
        .into(),
    )
}

/// Joins `singleton` and `Customer` in the requested order, optionally
/// followed by a left-deep `Item` stage keyed by the customer.
fn plan(singleton: &[String], singleton_first: bool, with_items: bool) -> JoinPlan {
    let singleton_source = JoinSource::singleton(singleton.to_vec());
    let customer = JoinSource::new(path(&["Customer"]));
    let plan = if singleton_first {
        JoinPlan::new(
            singleton_source,
            customer,
            JoinConditions::new(JoinKey::new(
                singleton.to_vec(),
                Vec::new(),
                path(&["Number"]),
            )),
        )
    } else {
        JoinPlan::new(
            customer,
            singleton_source,
            JoinConditions::new(JoinKey::new(
                path(&["Customer"]),
                path(&["Number"]),
                Vec::new(),
            )),
        )
    }
    .unwrap();
    if !with_items {
        return plan;
    }
    plan.then(
        JoinSource::new(path(&["Item"])),
        JoinConditions::new(JoinKey::new(
            path(&["Customer"]),
            path(&["Number"]),
            path(&["CustomerNumber"]),
        )),
    )
    .unwrap()
}

fn join_nodes(singleton: &[String]) -> Graph {
    Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::JoinField {
                    join: JOIN,
                    collection: singleton.to_vec(),
                    path: Vec::new(),
                },
            ),
            (
                1,
                Node::JoinField {
                    join: JOIN,
                    collection: path(&["Customer"]),
                    path: path(&["Name"]),
                },
            ),
            (
                2,
                Node::JoinField {
                    join: JOIN,
                    collection: path(&["Item"]),
                    path: path(&["Sku"]),
                },
            ),
            (
                3,
                Node::SourceField {
                    path: path(&["Id"]),
                    frame: None,
                },
            ),
        ]),
    }
}

fn row_scope(field: &str, plan: JoinPlan, with_items: bool) -> Scope {
    let mut bindings = vec![
        Binding {
            target_field: "Nr".into(),
            node: 0,
        },
        Binding {
            target_field: "Name".into(),
            node: 1,
        },
    ];
    if with_items {
        bindings.push(Binding {
            target_field: "Sku".into(),
            node: 2,
        });
    }
    Scope {
        target_field: field.into(),
        iteration: ScopeIteration::InnerJoin { id: JOIN, plan },
        bindings,
        ..Scope::default()
    }
}

fn row_target(field: &str) -> SchemaNode {
    SchemaNode::group(
        field,
        ["Nr", "Name", "Sku"]
            .into_iter()
            .map(|name| SchemaNode::scalar(name, ScalarType::String))
            .collect(),
    )
    .repeating()
}

fn project(target: SchemaNode, graph: Graph, root: Scope) -> Project {
    Project {
        source: source_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root,
    }
}

/// A root-context join over `singleton`.
fn root_project(singleton: &[String], singleton_first: bool, with_items: bool) -> Project {
    project(
        SchemaNode::group("Target", vec![row_target("Row")]),
        join_nodes(singleton),
        Scope {
            children: vec![row_scope(
                "Row",
                plan(singleton, singleton_first, with_items),
                with_items,
            )],
            ..Scope::default()
        },
    )
}

fn rows<'a>(output: &'a Instance, field: &str) -> Vec<Vec<Option<&'a Value>>> {
    output
        .field(field)
        .and_then(Instance::as_repeated)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    ["Nr", "Name", "Sku"]
                        .into_iter()
                        .map(|name| row.field(name).and_then(Instance::as_scalar))
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn validation_refuses_singleton_joins_crossing_inactive_repetition() {
    let crossing = path(&["Orders", "CustomerNr"]);
    let expected = "join 7 singleton collection `Orders/CustomerNr` crosses a repeating source level; join the repeating collection with a key path instead";
    for singleton_first in [true, false] {
        for with_items in [false, true] {
            let project = root_project(&crossing, singleton_first, with_items);
            let messages = validate(&project)
                .into_iter()
                .map(|issue| issue.message)
                .collect::<Vec<_>>();
            assert_eq!(
                messages,
                vec![expected.to_string()],
                "singleton first: {singleton_first}, items: {with_items}"
            );
        }
    }

    let mut aggregate = root_project(&path(&["Header", "CustomerNr"]), true, false);
    aggregate.graph.nodes.insert(
        4,
        Node::JoinAggregate {
            function: mapping::AggregateOp::Count,
            join: JoinId::new(8),
            plan: plan(&crossing, true, false),
            expression: None,
            arg: None,
        },
    );
    aggregate.root.children[0].bindings[1].node = 4;
    let messages = validate(&aggregate)
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("join 8 singleton collection `Orders/CustomerNr`")),
        "{messages:?}"
    );
}

#[test]
fn root_singleton_joins_produce_complete_tuples_in_both_source_orders() {
    let singleton = path(&["Header", "CustomerNr"]);
    let input = source(&[("O1", "C1")]);
    let c2 = string("C2");
    let grace = string("Grace");
    let (s1, s3) = (string("S1"), string("S3"));
    for singleton_first in [true, false] {
        let project = root_project(&singleton, singleton_first, false);
        assert!(validate(&project).is_empty(), "{:?}", validate(&project));
        let output = run(&project, &input).unwrap();
        assert_eq!(
            rows(&output, "Row"),
            vec![vec![Some(&c2), Some(&grace), None]],
            "singleton first: {singleton_first}"
        );

        let project = root_project(&singleton, singleton_first, true);
        assert!(validate(&project).is_empty(), "{:?}", validate(&project));
        let output = run(&project, &input).unwrap();
        assert_eq!(
            rows(&output, "Row"),
            vec![
                vec![Some(&c2), Some(&grace), Some(&s1)],
                vec![Some(&c2), Some(&grace), Some(&s3)],
            ],
            "singleton first with items: {singleton_first}"
        );
    }
}

#[test]
fn active_parent_singleton_joins_correlate_each_order() {
    let singleton = path(&["CustomerNr"]);
    let input = source(&[("O1", "C2"), ("O2", "C1")]);
    for singleton_first in [true, false] {
        let order_target = SchemaNode::group(
            "Order",
            vec![
                SchemaNode::scalar("Id", ScalarType::String),
                row_target("Match"),
            ],
        )
        .repeating();
        let project = project(
            SchemaNode::group("Target", vec![order_target]),
            join_nodes(&singleton),
            Scope {
                children: vec![Scope {
                    target_field: "Order".into(),
                    iteration: ScopeIteration::Source(path(&["Orders"])),
                    bindings: vec![Binding {
                        target_field: "Id".into(),
                        node: 3,
                    }],
                    children: vec![row_scope(
                        "Match",
                        plan(&singleton, singleton_first, false),
                        false,
                    )],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        );
        assert!(validate(&project).is_empty(), "{:?}", validate(&project));
        let output = run(&project, &input).unwrap();
        let orders = output
            .field("Order")
            .and_then(Instance::as_repeated)
            .unwrap();
        let summary = orders
            .iter()
            .map(|order| {
                (
                    order.field("Id").and_then(Instance::as_scalar).cloned(),
                    rows(order, "Match")
                        .into_iter()
                        .map(|row| row.into_iter().map(|value| value.cloned()).collect())
                        .collect::<Vec<Vec<_>>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                (
                    Some(string("O1")),
                    vec![vec![Some(string("C2")), Some(string("Grace")), None]],
                ),
                (
                    Some(string("O2")),
                    vec![vec![Some(string("C1")), Some(string("Ada")), None]],
                ),
            ],
            "singleton first: {singleton_first}"
        );
    }
}

fn messages(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

#[test]
fn validation_refuses_root_singletons_missing_from_the_root_context() {
    // `CustomerNr` exists only beneath repeated `Orders`, so a root-context
    // join would silently find no singleton and produce no tuples.
    let relative = path(&["CustomerNr"]);
    let expected = "join 7 singleton collection `CustomerNr` does not resolve from the root source context; use a path from the source root or a named source";
    for singleton_first in [true, false] {
        for with_items in [false, true] {
            assert_eq!(
                messages(&root_project(&relative, singleton_first, with_items)),
                vec![expected.to_string()],
                "singleton first: {singleton_first}, items: {with_items}"
            );
        }
    }

    let aggregate = |singleton: &[String]| {
        let mut project = root_project(&path(&["Header", "CustomerNr"]), true, false);
        project.graph.nodes.insert(
            4,
            Node::JoinAggregate {
                function: mapping::AggregateOp::Count,
                join: JoinId::new(8),
                plan: plan(singleton, true, false),
                expression: None,
                arg: None,
            },
        );
        project.root.bindings.push(Binding {
            target_field: "Count".into(),
            node: 4,
        });
        let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
            panic!("singleton join target is a group");
        };
        children.push(SchemaNode::scalar("Count", ScalarType::Int));
        project
    };
    assert_eq!(
        messages(&aggregate(&relative)),
        vec![
            "join 8 singleton collection `CustomerNr` does not resolve from the root source context; use a path from the source root or a named source"
                .to_string()
        ]
    );
    let rooted = aggregate(&path(&["Header", "CustomerNr"]));
    assert!(validate(&rooted).is_empty(), "{:?}", validate(&rooted));
    assert_eq!(
        run(&rooted, &source(&[("O1", "C1")]))
            .unwrap()
            .field("Count")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
}

#[test]
fn named_and_active_parent_singletons_remain_valid() {
    let mut named = root_project(&path(&["Settings", "CustomerNr"]), true, false);
    named.extra_sources.push(mapping::NamedSource {
        name: "Settings".into(),
        path: "settings.json".into(),
        schema: SchemaNode::group(
            "Settings",
            vec![SchemaNode::scalar("CustomerNr", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    assert!(validate(&named).is_empty(), "{:?}", validate(&named));

    // Inside an `Orders` scope the relative singleton reads the active order.
    let mut correlated = root_project(&path(&["Header", "CustomerNr"]), true, false);
    correlated.graph.nodes.insert(
        4,
        Node::JoinAggregate {
            function: mapping::AggregateOp::Count,
            join: JoinId::new(8),
            plan: plan(&path(&["CustomerNr"]), true, false),
            expression: None,
            arg: None,
        },
    );
    correlated.target = SchemaNode::group(
        "Target",
        vec![
            row_target("Row"),
            SchemaNode::group(
                "Order",
                vec![SchemaNode::scalar("Matches", ScalarType::Int)],
            )
            .repeating(),
        ],
    );
    correlated.root.children.push(Scope {
        target_field: "Order".into(),
        iteration: ScopeIteration::Source(path(&["Orders"])),
        bindings: vec![Binding {
            target_field: "Matches".into(),
            node: 4,
        }],
        ..Scope::default()
    });
    assert!(
        validate(&correlated).is_empty(),
        "{:?}",
        validate(&correlated)
    );
    let output = run(&correlated, &source(&[("O1", "C2"), ("O2", "C3")])).unwrap();
    let matches = output
        .field("Order")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|order| {
            order
                .field("Matches")
                .and_then(Instance::as_scalar)
                .cloned()
        })
        .collect::<Vec<_>>();
    assert_eq!(matches, vec![Some(Value::Int(1)), Some(Value::Int(0))]);
}
