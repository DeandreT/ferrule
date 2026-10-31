//! Computed-key tuples under scope controls, longer plans and joined
//! aggregates, with the key's evaluation timing and its failure modes.

use std::cell::RefCell;

use super::*;
use mapping::{AggregateOp, SequenceWindow};

/// The frozen input without its customers.
fn without_customers(xml: &str) -> Instance {
    let kept = xml
        .lines()
        .filter(|line| !line.starts_with("<Customer>"))
        .collect::<Vec<_>>()
        .join("\n");
    input(&kept)
}

fn hits(project: &Project) -> Vec<(String, String)> {
    let output = run(project, &input(INPUT)).expect("the computed join runs");
    pairs(output.field("Hit"))
}

fn named(names: &[&str]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|name| ("C2".to_string(), (*name).to_string()))
        .collect()
}

/// A root join keyed by the constant `C2`, with `controls` applied to its
/// scope: Grace, then Hopper, before any control.
fn controlled(key_first: bool, controls: impl FnOnce(&mut Keys, &mut Scope)) -> Project {
    let mut keys = Keys::new();
    let key = Key::Computed(keys.constant(Value::String("C2".into())));
    let mut root = root_scope(Context::Root, plan(&key, "Number", key_first));
    controls(&mut keys, &mut root.children[0]);
    fixture_project(keys.graph(), root)
}

#[test]
fn filter_sort_and_windows_apply_to_computed_key_tuples() {
    for key_first in [true, false] {
        let plain = controlled(key_first, |_, _| {});
        assert_eq!(hits(&plain), named(&["Grace", "Hopper"]));

        let sorted = controlled(key_first, |_, scope| {
            scope.sort_by = Some(NAME);
            scope.sort_descending = true;
        });
        assert_eq!(validate(&sorted), []);
        assert_eq!(hits(&sorted), named(&["Hopper", "Grace"]));

        let filtered = controlled(key_first, |keys, scope| {
            let hopper = keys.constant(Value::String("Hopper".into()));
            scope.filter = Some(keys.call("equal", vec![NAME, hopper]));
        });
        assert_eq!(validate(&filtered), []);
        assert_eq!(hits(&filtered), named(&["Hopper"]));

        // Tuple positions count joined tuples; the computed member adds none.
        let second = controlled(key_first, |keys, scope| {
            let position = keys.add(Node::JoinPosition { join: JOIN });
            let two = keys.constant(Value::Int(2));
            scope.filter = Some(keys.call("equal", vec![position, two]));
        });
        assert_eq!(validate(&second), []);
        assert_eq!(hits(&second), named(&["Hopper"]));

        for (first, expected) in [(true, "Grace"), (false, "Hopper")] {
            let windowed = controlled(key_first, |keys, scope| {
                let count = keys.constant(Value::Int(1));
                scope.windows = vec![if first {
                    SequenceWindow::First { count }
                } else {
                    SequenceWindow::Last { count }
                }];
            });
            assert_eq!(validate(&windowed), []);
            assert_eq!(hits(&windowed), named(&[expected]));
        }
    }
}

/// A three-source root plan reading `Order/Id` as the tuple's number.
fn three_sources(plan: JoinPlan) -> Project {
    let mut keys = Keys::new();
    keys.nodes.insert(
        NUMBER,
        Node::JoinField {
            join: JOIN,
            collection: path(&["Order"]),
            path: path(&["Id"]),
        },
    );
    let mut project = fixture_project(keys.graph(), root_scope(Context::Root, plan));
    project.graph.nodes.insert(
        20,
        Node::Const {
            value: Value::String("C2".into()),
        },
    );
    project
}

#[test]
fn computed_members_take_their_place_in_longer_plans() {
    let orders = JoinSource::new(path(&["Order"]));
    let by_customer = || {
        JoinConditions::new(JoinKey::new(
            path(&["Customer"]),
            path(&["Number"]),
            path(&["CustomerNumber"]),
        ))
    };
    // The key as a middle stage's right source filters the left customers;
    // later stages see only the collection members.
    let middle = JoinPlan::new(
        JoinSource::new(path(&["Customer"])),
        JoinSource::computed(KEY, 20),
        JoinConditions::new(JoinKey::new(
            path(&["Customer"]),
            path(&["Number"]),
            Vec::new(),
        )),
    )
    .and_then(|plan| plan.then(JoinSource::new(path(&["Order"])), by_customer()))
    .expect("a valid three-source plan");
    // The key as the first source, read again by a later stage's condition.
    let first = JoinPlan::new(
        JoinSource::computed(KEY, 20),
        JoinSource::new(path(&["Customer"])),
        JoinConditions::new(JoinKey::computed(KEY, path(&["Number"]))),
    )
    .and_then(|plan| {
        plan.then(
            orders,
            JoinConditions::new(JoinKey::computed(KEY, path(&["CustomerNumber"]))),
        )
    })
    .expect("a valid three-source plan");

    // `C2` customers exist, but no order's customer number is `C2`.
    for plan in [middle, first] {
        let mut project = three_sources(plan);
        assert_eq!(validate(&project), []);
        assert_eq!(hits(&project), []);
        project.graph.nodes.insert(
            20,
            Node::Const {
                value: Value::String("C1".into()),
            },
        );
        // Ada is the only `C1` customer and O6 the only `C1` order.
        assert_eq!(hits(&project), [("O6".to_string(), "Ada".to_string())]);
    }
}

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn key_values(project: &Project, key: NodeId) -> usize {
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    run_with_context(project, &input(INPUT), &execution).expect("the computed join runs");
    collector
        .0
        .into_inner()
        .iter()
        .filter(|event| matches!(event, TraceEvent::NodeValue { node, .. } if *node == key))
        .count()
}

#[test]
fn the_key_is_evaluated_once_per_enclosing_item_and_once_at_the_root() {
    for key_first in [true, false] {
        // Seven orders, eight customers: one evaluation per order.
        let mut keys = Keys::new();
        let key = keys.guarded(&["CustomerNumber"], upper);
        let joined = plan(&Key::Computed(key), "Number", key_first);
        let nested = fixture_project(keys.graph(), root_scope(Context::EachOrder, joined));
        assert_eq!(key_values(&nested, key), 7);

        let mut keys = Keys::new();
        let key = keys.guarded(&["Header", "Code"], upper);
        let joined = plan(&Key::Computed(key), "Number", key_first);
        let root = fixture_project(keys.graph(), root_scope(Context::Root, joined));
        assert_eq!(key_values(&root, key), 1);
    }
}

#[test]
fn key_node_errors_fail_the_join_even_when_another_source_is_empty() {
    // Ferrule's `upper` refuses the absent `CustomerNumber` of order O3.
    for key_first in [true, false] {
        let mut keys = Keys::new();
        let field = keys.field(&["CustomerNumber"]);
        let key = Key::Computed(keys.call("upper", vec![field]));
        let project = fixture_project(
            keys.graph(),
            root_scope(Context::EachOrder, plan(&key, "Number", key_first)),
        );
        assert_eq!(validate(&project), []);
        let aggregate = aggregate_project(
            |keys| {
                let field = keys.field(&["CustomerNumber"]);
                keys.call("upper", vec![field])
            },
            key_first,
            true,
        );
        assert_eq!(validate(&aggregate), []);
        for source in [input(INPUT), without_customers(INPUT)] {
            for project in [&project, &aggregate] {
                let error = run(project, &source).expect_err("the key node fails");
                assert!(error.to_string().contains("upper"), "{error}");
            }
        }
    }
}

#[test]
fn null_like_keys_yield_no_tuples_with_or_without_a_right_side() {
    for key_first in [true, false] {
        for value in [Value::Null, Value::json_null(), Value::xml_nil()] {
            let mut keys = Keys::new();
            let key = Key::Computed(keys.constant(value.clone()));
            let project = fixture_project(
                keys.graph(),
                root_scope(Context::Root, plan(&key, "Number", key_first)),
            );
            assert_eq!(validate(&project), []);
            for source in [input(INPUT), without_customers(INPUT)] {
                let output = run(&project, &source).expect("a Null-like key runs");
                assert_eq!(pairs(output.field("Hit")), [], "{value:?}");
            }
        }
    }
}

#[test]
fn decimal_keys_stay_a_typed_comparison_failure() {
    let decimal = Value::decimal(ir::Decimal96::parse_exact("7").expect("decimal"));
    for key_first in [true, false] {
        let mut keys = Keys::new();
        let key = Key::Computed(keys.constant(decimal.clone()));
        let project = fixture_project(
            keys.graph(),
            root_scope(Context::Root, plan(&key, "Number", key_first)),
        );
        assert_eq!(validate(&project), []);
        let error = run(&project, &input(INPUT)).expect_err("decimals are not compared");
        assert!(error.to_string().contains("decimal"), "{error}");
        // Without a pairing to compare, the key is evaluated but never refused.
        let output = run(&project, &without_customers(INPUT)).expect("nothing is compared");
        assert_eq!(pairs(output.field("Hit")), []);
    }
}

pub(super) const AGGREGATE: NodeId = 30;
const SEPARATOR: NodeId = 31;
pub(super) const COUNT: NodeId = 32;

/// A count and a name list of the computed-key tuples per order, or at the
/// root, each reduced by a joined aggregate.
pub(super) fn aggregate_project(
    key: fn(&mut Keys) -> NodeId,
    key_first: bool,
    each_order: bool,
) -> Project {
    let mut keys = Keys::new();
    let key = key(&mut keys);
    let joined = plan(&Key::Computed(key), "Number", key_first);
    keys.nodes.insert(
        SEPARATOR,
        Node::Const {
            value: Value::String(",".into()),
        },
    );
    keys.nodes.insert(
        AGGREGATE,
        Node::JoinAggregate {
            function: AggregateOp::Join,
            join: JOIN,
            plan: joined.clone(),
            expression: Some(NAME),
            arg: Some(SEPARATOR),
        },
    );
    keys.nodes.insert(
        COUNT,
        Node::JoinAggregate {
            function: AggregateOp::Count,
            join: JoinId::new(6),
            plan: joined,
            expression: None,
            arg: None,
        },
    );
    let summary = |field: &str| {
        vec![
            SchemaNode::scalar(field, ScalarType::String),
            SchemaNode::scalar("Names", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ]
    };
    let bindings = |id_binding: Option<Binding>| {
        id_binding
            .into_iter()
            .chain([
                Binding {
                    target_field: "Names".into(),
                    node: AGGREGATE,
                },
                Binding {
                    target_field: "Count".into(),
                    node: COUNT,
                },
            ])
            .collect()
    };
    let (target, root) = if each_order {
        (
            SchemaNode::group(
                "Report",
                vec![SchemaNode::group("Order", summary("Id")).repeating()],
            ),
            Scope {
                children: vec![Scope {
                    target_field: "Order".into(),
                    iteration: ScopeIteration::Source(path(&["Order"])),
                    bindings: bindings(Some(Binding {
                        target_field: "Id".into(),
                        node: ORDER_ID,
                    })),
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        )
    } else {
        (
            SchemaNode::group("Report", summary("Unused")),
            Scope {
                bindings: bindings(None),
                ..Scope::default()
            },
        )
    };
    let mut project = fixture_project(keys.graph(), root);
    project.target = target;
    project
}

fn summary(instance: &Instance) -> (String, i64) {
    let names = instance.field("Names").map(text).unwrap_or_default();
    let count = match instance.field("Count").and_then(Instance::as_scalar) {
        Some(Value::Int(count)) => *count,
        other => panic!("expected a count, found {other:?}"),
    };
    (names, count)
}

fn expected_summary(rows: &[(String, String)]) -> (String, i64) {
    (
        rows.iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(","),
        rows.len() as i64,
    )
}

#[test]
fn joined_aggregates_reduce_computed_key_tuples_per_consumer_context() {
    let upper_key = |keys: &mut Keys| keys.guarded(&["CustomerNumber"], upper);
    let header_key = |keys: &mut Keys| keys.guarded(&["Header", "Code"], upper);
    let source = input(INPUT);
    for key_first in [true, false] {
        let project = aggregate_project(upper_key, key_first, true);
        assert_eq!(validate(&project), []);
        let output = run(&project, &source).expect("joined aggregates run");
        let actual = output
            .field("Order")
            .and_then(Instance::as_repeated)
            .expect("orders")
            .iter()
            .map(|order| (order.field("Id").map(text).expect("id"), summary(order)))
            .collect::<BTreeMap<_, _>>();
        let expected = expected_tuples("upper")
            .into_iter()
            .map(|(order, rows)| (order, expected_summary(&rows)))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected, "key_first={key_first}");

        let project = aggregate_project(header_key, key_first, false);
        assert_eq!(validate(&project), []);
        let output = run(&project, &source).expect("root joined aggregates run");
        assert_eq!(
            summary(&output),
            expected_summary(&expected_tuples("root-header")["Hit"])
        );
    }
}

/// A joined aggregate's computed key is its first input pin; the per-tuple
/// expression and the argument follow it.
#[test]
fn joined_aggregate_inputs_are_indexed_after_the_key() {
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    let project = aggregate_project(|keys| keys.guarded(&["Header", "Code"], upper), true, false);
    let key = project
        .graph
        .nodes
        .get(&AGGREGATE)
        .and_then(|node| match node {
            Node::JoinAggregate { plan, .. } => plan.computed_source(),
            _ => None,
        })
        .and_then(|computed| computed.scalar())
        .expect("a computed key");
    run_with_context(&project, &input(INPUT), &execution).expect("joined aggregates run");
    let mut inputs = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer: AGGREGATE,
                input,
                input_index,
                ..
            } => Some((input, input_index)),
            _ => None,
        })
        .collect::<Vec<_>>();
    inputs.dedup();
    assert_eq!(inputs, [(key, 0), (NAME, 1), (SEPARATOR, 2)]);
}
