use super::*;

fn text_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
}

fn header_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    protected_output: Option<NodeId>,
    name: &str,
) -> egui::FullOutput {
    let node = fx.call;
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.protected_output = protected_output;
            viewer
                .function_names
                .insert(FunctionId::new(7), name.into());
            viewer
                .parameter_names
                .insert(FunctionParameterId::new(8), name.into());
            viewer.show_header(node, &[], &[], ui, snarl);
        },
    )
}

#[test]
fn output_marks_follow_node_identity_and_preserve_suffixes_and_repeated_prefixes() {
    let mut fx = fixture();
    fx.graph
        .nodes
        .insert(42, Node::Const { value: Value::Null });
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let cases = [
        (
            Node::Const {
                value: Value::String("keep (output)".into()),
            },
            "unused",
            "keep (output)",
        ),
        (
            Node::UserFunctionCall {
                function: FunctionId::new(7),
                args: vec![],
            },
            "keep (output)",
            "keep (output)",
        ),
        (
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(8),
            },
            "keep (output)",
            "keep (output)",
        ),
        (
            Node::UserFunctionCall {
                function: FunctionId::new(7),
                args: vec![],
            },
            "Call: Name",
            "Call: Name",
        ),
        (
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(8),
            },
            "Input: x",
            "Input: x",
        ),
    ];
    for (node, name, summary) in cases {
        fx.graph.nodes.insert(0, node);
        let marked = format!("{summary} · output");
        for protected in [None, Some(42), Some(0)] {
            let output = header_frame(&mut fx, &mut snarl, &context, protected, name);
            if protected == Some(0) {
                assert!(
                    text_position(&output, &marked).is_some(),
                    "selected output lost content: {name:?}"
                );
                assert!(text_position(&output, summary).is_none());
            } else {
                assert!(
                    text_position(&output, summary).is_some(),
                    "ordinary content was stripped: {name:?}"
                );
                assert!(
                    text_position(&output, &marked).is_none(),
                    "ordinary node gained an output mark: {name:?}"
                );
            }
        }
    }
}

fn property_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, egui::Id) {
    let pin = snarl.out_pin(OutPinId {
        node: fx.call,
        output: 0,
    });
    let mut edit_id = egui::Id::NULL;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            edit_id = ui.next_auto_id();
            fx.viewer().show_output(&pin, ui, snarl);
        },
    );
    (output, edit_id)
}

fn click_properties(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    pos: egui::Pos2,
) {
    for pressed in [true, false] {
        property_frame(
            fx,
            snarl,
            context,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn compact_call_popover_edits_arguments_without_changing_existing_wires() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("one".into()),
        },
    );
    fx.graph.nodes.insert(
        0,
        Node::Call {
            function: "concat".into(),
            args: vec![1],
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let constant = snarl.insert_node(egui::pos2(0.0, 150.0), CanvasNode::Graph(1));
    snarl.connect(
        OutPinId {
            node: constant,
            output: 0,
        },
        InPinId {
            node: fx.call,
            input: 0,
        },
    );
    let wires_before = snarl.wires().collect::<Vec<_>>();
    let graph_before = serde_json::to_value(&fx.graph).unwrap();
    let context = egui::Context::default();
    crate::icons::install(&context);

    let (closed, _) = property_frame(&mut fx, &mut snarl, &context, Vec::new());
    assert!(text_position(&closed, "+arg").is_none());
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let edit = text_position(&closed, &pencil).expect("compact node exposes its editor");
    click_properties(&mut fx, &mut snarl, &context, edit);
    let mut opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), graph_before);
    let add = text_position(&opened, "+arg").expect("popover retains argument controls");
    click_properties(&mut fx, &mut snarl, &context, add);

    let Node::Call { args, .. } = &fx.graph.nodes[&0] else {
        panic!("operation changed while editing its arguments");
    };
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], 1);
    assert!(matches!(
        fx.graph.nodes.get(&args[1]),
        Some(Node::Unconnected)
    ));
    assert_eq!(snarl.wires().collect::<Vec<_>>(), wires_before);
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 2);

    property_frame(
        &mut fx,
        &mut snarl,
        &context,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let closed = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    assert!(text_position(&closed, "+arg").is_none());
}

#[test]
fn compact_node_editor_can_be_opened_with_the_keyboard() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let (_, edit_id) = property_frame(&mut fx, &mut snarl, &context, Vec::new());
    context.memory_mut(|memory| memory.request_focus(edit_id));
    property_frame(
        &mut fx,
        &mut snarl,
        &context,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let mut opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    assert!(text_position(&opened, "+arg").is_some());
}

#[test]
fn compact_node_function_selector_survives_its_nested_popup() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let (closed, _) = property_frame(&mut fx, &mut snarl, &context, Vec::new());
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    click_properties(
        &mut fx,
        &mut snarl,
        &context,
        text_position(&closed, &pencil).unwrap(),
    );
    let mut opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    click_properties(
        &mut fx,
        &mut snarl,
        &context,
        text_position(&opened, "Concat").expect("function selector is visible"),
    );
    let mut choices = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        choices = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    click_properties(
        &mut fx,
        &mut snarl,
        &context,
        text_position(&choices, "Uppercase").expect("nested selector remains open"),
    );
    let Node::Call { function, args } = &fx.graph.nodes[&0] else {
        panic!("call disappeared while its selector was open");
    };
    assert_eq!(function, "upper");
    assert_eq!(args.len(), 1);
    assert!(matches!(
        fx.graph.nodes.get(&args[0]),
        Some(Node::Unconnected)
    ));
    assert!(snarl.wires().next().is_none());
    let mut updated = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        updated = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    assert!(
        text_position(&updated, "Call: Uppercase").is_some(),
        "selecting a function keeps its properties open"
    );
    assert!(text_position(&updated, "+arg").is_some());
}

#[test]
fn compact_nodes_settle_at_small_sizes_and_keep_schema_endpoints_readable() {
    let mut fx = fixture();
    let source_width = crate::app::endpoint_block_size(
        &fx.source_blocks[0].title,
        &fx.source_blocks[0].pin_labels,
    )
    .x;
    let target_width = crate::app::endpoint_block_size(
        &fx.target_blocks[0].title,
        &fx.target_blocks[0].pin_labels,
    )
    .x;
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("long literal value ".repeat(100)),
        },
    );
    fx.graph.nodes.insert(
        0,
        Node::Call {
            function: "concat".into(),
            args: vec![1, 1],
        },
    );
    fx.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    fx.graph.nodes.insert(
        2,
        Node::If {
            condition: 3,
            then: 0,
            else_: 1,
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    snarl.insert_node(egui::pos2(100.0, 250.0), CanvasNode::Graph(1));
    snarl.insert_node(egui::pos2(300.0, 250.0), CanvasNode::Graph(2));
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut sizes = std::collections::BTreeMap::new();
    let mut recent = Vec::new();
    for _ in 0..8 {
        let _ = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut viewer = fx.viewer();
                viewer.node_sizes = Some(&mut sizes);
                SnarlWidget::new()
                    .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                    .show(&mut snarl, &mut viewer, ui);
                assert!(viewer.pin_interaction_ids.iter().all(|id| {
                    ui.ctx()
                        .read_response(*id)
                        .is_some_and(|response| response.sense.senses_drag())
                }));
            },
        );
        recent.push([
            sizes[&CanvasNode::Graph(0)],
            sizes[&CanvasNode::Graph(1)],
            sizes[&CanvasNode::Graph(2)],
        ]);
    }
    for sizes in &recent[4..] {
        for size in sizes {
            assert!(
                size.x <= 220.0 && size.y <= 160.0,
                "ordinary node uses too much canvas space: {size:?}"
            );
        }
    }
    for pair in recent[4..].windows(2) {
        for (before, after) in pair[0].iter().zip(&pair[1]) {
            assert!(
                (*before - *after).length() < 0.5,
                "compact node grows across repaints"
            );
        }
    }
    assert!(sizes[&CanvasNode::SourceBlock(0)].x >= source_width - 1.0);
    assert!(sizes[&CanvasNode::TargetBlock(0)].x >= target_width - 1.0);
}
