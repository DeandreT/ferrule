use super::*;

fn counter_fixture() -> Fixture {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        0,
        Node::InvocationCounter {
            key: " Key ".into(),
            start: 2,
            step: 3,
            restart: None,
        },
    );
    for id in [2, 3] {
        fx.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::Int(1),
            },
        );
    }
    fx
}

#[test]
fn invocation_counter_pins_preserve_order_and_optional_restart() {
    let mut fx = counter_fixture();
    fx.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 3);
    assert_eq!(fx.viewer().input_at(0, 0), Some(2));
    assert_eq!(fx.viewer().input_at(0, 1), Some(3));
    assert_eq!(fx.viewer().input_at(0, 2), None);
    assert!(fx.viewer().set_input(0, 2, 4));
    assert_eq!(fx.viewer().input_at(0, 2), Some(4));
    assert!(fx.viewer().clear_optional_input(0, 2));
    assert!(!fx.viewer().clear_optional_input(0, 0));
    assert!(
        matches!(&fx.graph.nodes[&0], Node::InvocationCounter { key, start: 2, step: 3, restart: None } if key == " Key ")
    );
    assert_eq!(node_inputs(&fx.graph.nodes[&0]), [2, 3]);
}

#[test]
fn invocation_counter_optional_deletion_needs_no_placeholder_id() {
    let mut fx = counter_fixture();
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    fx.graph.nodes.insert(
        NodeId::MAX,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let restart = snarl.insert_node(egui::pos2(0.0, 200.0), CanvasNode::Graph(1));
    let from = snarl.out_pin(OutPinId {
        node: restart,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 2,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    assert_eq!(fx.viewer().input_at(0, 2), Some(1));
    assert!(fx.viewer().remove_graph_node(1, restart, &mut snarl));
    assert_eq!(fx.viewer().input_at(0, 2), None);
    assert!(
        !fx.graph
            .nodes
            .values()
            .any(|n| matches!(n, Node::Unconnected))
    );
    let start = snarl.insert_node(egui::pos2(0.0, 250.0), CanvasNode::Graph(2));
    let before = serde_json::to_value(&fx.graph).unwrap();
    assert!(!fx.viewer().remove_graph_node(2, start, &mut snarl));
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), before);
}

#[test]
fn invocation_counter_repaints_do_not_change_graph_values() {
    let mut fx = counter_fixture();
    let before = serde_json::to_value(&fx.graph).unwrap();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    for _ in 0..4 {
        let _ = context.run_test_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                SnarlWidget::new().show(&mut snarl, &mut fx.viewer(), ui);
            },
        );
    }
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), before);
}
