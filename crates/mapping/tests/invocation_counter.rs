use mapping::{Node, NodeInputRole, NodeInputSlot};

#[test]
fn invocation_counter_retains_key_and_ordered_dependencies() {
    let node = Node::InvocationCounter {
        key: "A\0é".into(),
        start: 30,
        step: 10,
        restart: Some(20),
    };
    assert_eq!(node.dependencies(), [30, 10, 20]);
    let mut inputs = Vec::new();
    node.visit_inputs(&mut |input| inputs.push((input.node, input.slot, input.role)));
    assert_eq!(
        inputs,
        [
            (30, NodeInputSlot::Start, NodeInputRole::Parent),
            (10, NodeInputSlot::Step, NodeInputRole::Parent),
            (20, NodeInputSlot::Restart, NodeInputRole::Parent),
        ]
    );
    let wire = serde_json::to_value(&node).unwrap();
    assert_eq!(wire["kind"], "invocation_counter");
    assert_eq!(wire["key"], "A\0é");
    assert_eq!(wire["restart"], 20);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.dependencies(), [30, 10, 20]);
    assert!(matches!(decoded, Node::InvocationCounter { key, .. } if key == "A\0é"));
}

#[test]
fn invocation_counter_absent_restart_is_not_an_input() {
    let node = Node::InvocationCounter {
        key: "".into(),
        start: 1,
        step: 2,
        restart: None,
    };
    assert_eq!(node.dependencies(), [1, 2]);
    let wire = serde_json::to_value(node).unwrap();
    assert!(wire.get("restart").is_none());
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        decoded,
        Node::InvocationCounter { restart: None, .. }
    ));
}
