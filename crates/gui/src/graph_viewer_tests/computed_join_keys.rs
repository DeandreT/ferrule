//! A computed join key is owned by its join: removing the key node is blocked
//! while a join scope or joined aggregate reads it, and a computed joined
//! aggregate's pins are read-only.

use super::*;
use mapping::{
    ComputedJoinSourceId, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, ScopeIteration,
};

const KEY_NODE: NodeId = 12;
const AGGREGATE: NodeId = 13;
const VALUES: NodeId = 14;

fn plan() -> JoinPlan {
    let key = ComputedJoinSourceId::new(1);
    JoinPlan::new(
        JoinSource::computed(key, KEY_NODE),
        JoinSource::new(vec!["row".into()]),
        JoinConditions::new(JoinKey::computed(key, vec!["name".into()])),
    )
    .unwrap()
}

fn constant(text: &str) -> Node {
    Node::Const {
        value: Value::String(text.into()),
    }
}

/// The key node on the canvas, read either by a join scope or by a joined
/// aggregate with a values expression.
fn setup(aggregate: bool) -> (Fixture, Snarl<CanvasNode>, SnarlNodeId) {
    let mut fx = fixture();
    fx.graph.nodes.insert(KEY_NODE, constant("C2"));
    if aggregate {
        fx.graph.nodes.insert(VALUES, constant("v"));
        fx.graph.nodes.insert(
            AGGREGATE,
            Node::JoinAggregate {
                function: mapping::AggregateOp::Count,
                join: JoinId::new(1),
                plan: plan(),
                expression: Some(VALUES),
                arg: None,
            },
        );
    } else {
        fx.root_scope.children.push(Scope {
            target_field: "row".into(),
            iteration: ScopeIteration::InnerJoin {
                id: JoinId::new(1),
                plan: plan(),
            },
            ..Scope::default()
        });
    }
    let mut snarl = Snarl::new();
    let key = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Graph(KEY_NODE));
    (fx, snarl, key)
}

fn state(fx: &Fixture) -> String {
    format!("{:?}\n{:?}", fx.graph, fx.root_scope)
}

#[test]
fn removing_a_computed_key_node_is_blocked_by_its_join() {
    for aggregate in [false, true] {
        for batch in [false, true] {
            let (mut fx, mut snarl, key) = setup(aggregate);
            let before = state(&fx);
            let (removed, error) = {
                let mut viewer = fx.viewer();
                let removed = if batch {
                    viewer.remove_snarl_nodes(&[key], &mut snarl)
                } else {
                    usize::from(viewer.remove_graph_node(KEY_NODE, key, &mut snarl))
                };
                (removed, viewer.error)
            };
            assert_eq!(removed, 0, "aggregate={aggregate} batch={batch}");
            let error = error.expect("removal reports its owner");
            // Canvas removal names the owner; batch removal reports it as a
            // mapping control.
            let owner = match (batch, aggregate) {
                (true, _) => "owned by a mapping control",
                (false, true) => "graph node 13",
                (false, false) => "scope row computed join key",
            };
            assert!(error.contains(owner), "{error}");
            assert_eq!(state(&fx), before);
        }
    }
}

#[test]
fn a_computed_joined_aggregate_pins_its_key_first_and_read_only() {
    let (mut fx, _, _) = setup(true);
    let node = fx.graph.nodes[&AGGREGATE].clone();
    assert!(graph_sequence::inputs_are_read_only(&node));
    assert_eq!(GraphViewer::input_count(&node), 2);
    let before = state(&fx);
    let mut viewer = fx.viewer();
    assert_eq!(viewer.input_at(AGGREGATE, 0), Some(KEY_NODE));
    assert_eq!(viewer.input_at(AGGREGATE, 1), Some(VALUES));
    assert_eq!(viewer.input_at(AGGREGATE, 2), None);
    for input in 0..2 {
        assert!(!viewer.set_input(AGGREGATE, input, 0));
    }
    drop(viewer);
    assert_eq!(state(&fx), before);
}
