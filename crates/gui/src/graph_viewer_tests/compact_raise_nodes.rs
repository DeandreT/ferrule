use super::*;

#[test]
fn raise_headers_stay_compact_and_repaints_preserve_the_optional_message() {
    let mut fx = fixture();
    fx.graph.nodes.insert(0, Node::Raise { message: None });
    let original = serde_json::to_value(&fx.graph).unwrap();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut sizes = std::collections::BTreeMap::new();
    let mut settled = Vec::new();
    let error_icon = char::from(lucide_icons::Icon::CircleX).to_string();
    for frame in 0..8 {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut viewer = fx.viewer();
                viewer.node_sizes = Some(&mut sizes);
                SnarlWidget::new()
                    .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                    .show(&mut snarl, &mut viewer, ui);
            },
        );
        fn has_text(shape: &egui::epaint::Shape, text: &str) -> bool {
            match shape {
                egui::epaint::Shape::Text(shape) => shape.galley.text() == text,
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().any(|shape| has_text(shape, text))
                }
                _ => false,
            }
        }
        if frame >= 4 {
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| has_text(&shape.shape, &error_icon))
            );
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| has_text(&shape.shape, "message"))
            );
            settled.push(sizes[&CanvasNode::Graph(0)]);
        }
    }
    for size in &settled {
        assert!(
            size.x <= 180.0 && size.y <= 120.0,
            "error node grew: {size:?}"
        );
    }
    assert!(
        settled
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).length() < 0.5)
    );
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), original);
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
    assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
    assert!(snarl.wires().next().is_none());
    let hint = graph_node_presentation::hint(&fx.graph.nodes[&0], "Raise error");
    assert!(hint.contains("No message is set."));
    assert!(hint.contains("when this expression is reached"));
}

#[test]
fn clearing_or_deleting_raise_message_wires_restores_absence_without_allocating_null() {
    let mut fx = fixture();
    fx.graph.nodes.insert(0, Node::Raise { message: None });
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    // Clearing an optional input must work even when no new node ID exists.
    fx.graph.nodes.insert(
        NodeId::MAX,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let message = snarl.insert_node(egui::pos2(0.0, 200.0), CanvasNode::Graph(1));
    let from = snarl.out_pin(OutPinId {
        node: message,
        output: 0,
    });
    let to_id = InPinId {
        node: fx.call,
        input: 0,
    };
    let to = snarl.in_pin(to_id);
    fx.viewer().connect(&from, &to, &mut snarl);
    assert_eq!(fx.viewer().input_at(0, 0), Some(1));
    assert!(matches!(
        fx.graph.nodes[&0],
        Node::Raise { message: Some(1) }
    ));
    assert_eq!(snarl.wires().count(), 1);
    let to = snarl.in_pin(to_id);
    fx.viewer().disconnect(&from, &to, &mut snarl);
    assert!(matches!(fx.graph.nodes[&0], Node::Raise { message: None }));
    assert_eq!(fx.graph.nodes.len(), 3);
    assert!(snarl.wires().next().is_none());
    assert!(
        !fx.graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Unconnected))
    );

    let to = snarl.in_pin(to_id);
    fx.viewer().connect(&from, &to, &mut snarl);
    assert!(matches!(
        fx.graph.nodes[&0],
        Node::Raise { message: Some(1) }
    ));
    assert!(fx.viewer().remove_graph_node(1, message, &mut snarl));
    assert!(matches!(fx.graph.nodes[&0], Node::Raise { message: None }));
    assert_eq!(fx.graph.nodes.len(), 2);
    assert!(snarl.wires().next().is_none());
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
    assert!(
        !fx.graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Unconnected))
    );
}

#[test]
fn mixed_required_and_optional_delete_is_atomic_when_ids_are_exhausted() {
    let mut fx = fixture();
    fx.graph.nodes.insert(0, Node::Raise { message: Some(1) });
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("keep".into()),
        },
    );
    fx.graph.nodes.insert(
        2,
        Node::Call {
            function: "upper".into(),
            args: vec![1],
        },
    );
    fx.graph.nodes.insert(
        NodeId::MAX,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let message = snarl.insert_node(egui::pos2(0.0, 200.0), CanvasNode::Graph(1));
    let required = snarl.insert_node(egui::pos2(400.0, 200.0), CanvasNode::Graph(2));
    snarl.connect(
        OutPinId {
            node: message,
            output: 0,
        },
        InPinId {
            node: fx.call,
            input: 0,
        },
    );
    snarl.connect(
        OutPinId {
            node: message,
            output: 0,
        },
        InPinId {
            node: required,
            input: 0,
        },
    );
    let original = serde_json::to_value(&fx.graph).unwrap();
    let wires = snarl.wires().collect::<Vec<_>>();
    assert!(!fx.viewer().remove_graph_node(1, message, &mut snarl));
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), original);
    assert_eq!(snarl.wires().collect::<Vec<_>>(), wires);
}
