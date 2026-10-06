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

fn source_field_fixture(
    path: Vec<String>,
    frame: Option<Vec<String>>,
) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    fx.graph.nodes.insert(0, Node::SourceField { path, frame });
    fx.graph
        .nodes
        .insert(42, Node::Const { value: Value::Null });
    fx.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 1,
    });
    let mut snarl = std::mem::take(&mut fx.snarl);
    let consumer = snarl.insert_node(egui::pos2(420.0, 200.0), CanvasNode::Graph(1));
    snarl.connect(
        OutPinId {
            node: fx.call,
            output: 0,
        },
        InPinId {
            node: consumer,
            input: 0,
        },
    );
    (fx, snarl)
}

fn source_field_state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> serde_json::Value {
    serde_json::json!({
        "graph": &fx.graph,
        "root_scope": &fx.root_scope,
        "nodes": format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
        "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    })
}

fn source_field_header_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    protected: Option<NodeId>,
    frame: &mut usize,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let node = fx.call;
    let before = source_field_state(fx, snarl);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(*frame as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.protected_output = protected;
            viewer.show_header(node, &[], &[], ui, snarl);
        },
    );
    eprintln!(
        "SourceField header original frame {frame}: before={before}; after={}; shapes={:?}; accessibility={:?}",
        source_field_state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update,
    );
    *frame += 1;
    output
}

#[test]
fn compact_source_fields_retain_full_hover_accessibility_and_frame_identity() {
    let cases = [
        (
            vec![
                "complete_untruncated_parent_資料".repeat(4),
                "keep (output)".into(),
            ],
            None,
            "keep (output)",
            "automatic".to_owned(),
        ),
        (
            vec!["根".into(), "資料📦".into()],
            Some(Vec::new()),
            "資料📦",
            "explicit (empty path)".to_owned(),
        ),
        (
            vec!["deep".into(), "name".into()],
            Some(vec!["complete_frame_parent_注文".repeat(4), "owner".into()]),
            "name",
            format!("explicit {}/owner", "complete_frame_parent_注文".repeat(4)),
        ),
        (
            vec!["keep (output)".into()],
            None,
            "keep (output)",
            "automatic".to_owned(),
        ),
        (Vec::new(), None, "current", "automatic".to_owned()),
    ];
    for (path, frame_owner, summary, frame_description) in cases {
        let full_path = if path.is_empty() {
            "<current> (empty path)".to_owned()
        } else {
            path.join("/")
        };
        let identity = format!("\nPath: {full_path}\nFrame: {frame_description}");
        for protected in [None, Some(42), Some(0)] {
            let (mut fx, mut snarl) = source_field_fixture(path.clone(), frame_owner.clone());
            let before = source_field_state(&fx, &snarl);
            let base_title = fx.viewer().title(&CanvasNode::Graph(0));
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let mut frame = 0;
            let mut titles = Vec::new();
            let mut output = source_field_header_frame(
                &mut fx,
                &mut snarl,
                &context,
                protected,
                &mut frame,
                Vec::new(),
            );
            for _ in 0..8 {
                if let Some(update) = &output.platform_output.accesskit_update {
                    titles.extend(update.nodes.iter().filter_map(|(_, node)| {
                        node.value()
                            .filter(|label| label.contains(&identity))
                            .map(str::to_owned)
                    }));
                }
                output = source_field_header_frame(
                    &mut fx,
                    &mut snarl,
                    &context,
                    protected,
                    &mut frame,
                    Vec::new(),
                );
            }
            let title = titles
                .first()
                .expect("header accessibility retains the complete path and frame");
            let marked = format!("{summary} · output");
            assert!(
                text_position(
                    &output,
                    if protected == Some(0) {
                        &marked
                    } else {
                        summary
                    }
                )
                .is_some()
            );
            assert_eq!(
                title,
                &format!(
                    "{base_title}{}{identity}",
                    if protected == Some(0) {
                        " (output)"
                    } else {
                        ""
                    }
                )
            );
            let icon = char::from(lucide_icons::Icon::ArrowRightFromLine).to_string();
            let hover = text_position(&output, &icon).expect("source icon is visible");
            for hover_frame in 0..8 {
                output = source_field_header_frame(
                    &mut fx,
                    &mut snarl,
                    &context,
                    protected,
                    &mut frame,
                    if hover_frame == 0 {
                        vec![egui::Event::PointerMoved(hover)]
                    } else {
                        Vec::new()
                    },
                );
            }
            assert!(
                text_position(&output, title).is_some(),
                "hover retains the complete accessible identity"
            );
            assert_eq!(source_field_state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
    }
}

#[test]
fn compact_source_field_popover_edits_the_path_and_preserves_frame_wires_and_reopening() {
    let frame = Some(vec!["complete".into(), "owner".into()]);
    let (mut fx, mut snarl) =
        source_field_fixture(vec!["branch".into(), "name".into()], frame.clone());
    let before = source_field_state(&fx, &snarl);
    let graph_before = fx.graph.clone();
    let context = egui::Context::default();
    crate::icons::install(&context);
    let (closed, edit_id) = property_frame(&mut fx, &mut snarl, &context, Vec::new());
    eprintln!(
        "SourceField closed property originals: state={before}; shapes={:?}",
        closed.shapes
    );
    assert!(text_position(&closed, "branch/name").is_none());
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let edit = text_position(&closed, &pencil).expect("source properties remain discoverable");
    click_properties(&mut fx, &mut snarl, &context, edit);
    let mut opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    eprintln!(
        "SourceField opened property originals: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        opened.shapes
    );
    assert_eq!(source_field_state(&fx, &snarl), before);
    let editor =
        text_position(&opened, "branch/name").expect("popover contains the existing path editor");
    click_properties(&mut fx, &mut snarl, &context, editor);
    let updated = property_frame(
        &mut fx,
        &mut snarl,
        &context,
        vec![
            egui::Event::Key {
                key: egui::Key::End,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Text("/updated".into()),
        ],
    )
    .0;
    let after = source_field_state(&fx, &snarl);
    eprintln!(
        "SourceField edited property originals: state={after}; shapes={:?}",
        updated.shapes
    );
    let mut expected_graph = graph_before;
    expected_graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["branch".into(), "name".into(), "updated".into()],
            frame,
        },
    );
    let mut expected = before.clone();
    expected["graph"] = serde_json::to_value(&expected_graph).unwrap();
    assert_eq!(after, expected);
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
    assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
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
    assert!(text_position(&closed, "branch/name/updated").is_none());
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
    let mut reopened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        reopened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    eprintln!(
        "SourceField reopened property originals: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        reopened.shapes
    );
    assert!(text_position(&reopened, "branch/name/updated").is_some());
    assert_eq!(source_field_state(&fx, &snarl), expected);
}

#[test]
fn compact_generated_source_item_popover_stays_read_only_and_retains_imported_metadata() {
    let (mut fx, mut snarl) = source_field_fixture(
        vec!["owned".into(), "but".into(), "invalid".into()],
        Some(Vec::new()),
    );
    fx.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::Int(1),
        },
    );
    fx.graph.nodes.insert(
        12,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    fx.graph.nodes.insert(
        10,
        Node::SequenceExists {
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 11,
                item: 0,
            },
            predicate: 12,
        },
    );
    let before = source_field_state(&fx, &snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let original_title = fx.viewer().title(&CanvasNode::Graph(0));
    let header = header_frame(&mut fx, &mut snarl, &context, None, "unused");
    eprintln!(
        "Owned SourceField header originals: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        header.shapes
    );
    assert_eq!(original_title, "Generated item #0");
    assert!(text_position(&header, "item #0").is_some());
    let item_icon = char::from(lucide_icons::Icon::ListOrdered).to_string();
    assert!(text_position(&header, &item_icon).is_some());
    let closed = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    click_properties(
        &mut fx,
        &mut snarl,
        &context,
        text_position(&closed, &pencil).expect("owned item exposes its read-only details"),
    );
    let mut opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    eprintln!(
        "Owned SourceField property originals: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        opened.shapes
    );
    let read_only = text_position(&opened, "Generated item (read-only)")
        .expect("ownership guard remains visible");
    assert!(text_position(&opened, "Invalid item shape (unchanged)").is_some());
    assert!(text_position(&opened, "owned/but/invalid").is_none());
    click_properties(&mut fx, &mut snarl, &context, read_only);
    let attempted = property_frame(
        &mut fx,
        &mut snarl,
        &context,
        vec![egui::Event::Text("changed".into())],
    )
    .0;
    eprintln!(
        "Owned SourceField attempted-edit originals: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        attempted.shapes
    );
    assert_eq!(source_field_state(&fx, &snarl), before);
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
    assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
}

#[test]
fn compact_source_fields_settle_below_the_inline_editor_width_without_changing_ports_or_wires() {
    for frame_owner in [
        None,
        Some(Vec::new()),
        Some(vec!["full_frame_path".repeat(12), "owner".into()]),
    ] {
        let (mut fx, mut snarl) = source_field_fixture(
            vec!["complete_source_path".repeat(12), "name".into()],
            frame_owner,
        );
        let before = source_field_state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let mut sizes = std::collections::BTreeMap::new();
        let mut recent = Vec::new();
        for frame in 0..8 {
            sizes.clear();
            let mut drag_responses = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    time: Some(frame as f64 * 0.1),
                    ..Default::default()
                },
                |ui| {
                    let mut viewer = fx.viewer();
                    viewer.node_sizes = Some(&mut sizes);
                    SnarlWidget::new()
                        .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                        .show(&mut snarl, &mut viewer, ui);
                    drag_responses = viewer
                        .pin_interaction_ids
                        .iter()
                        .map(|id| {
                            (
                                format!("{id:?}"),
                                ui.ctx()
                                    .read_response(*id)
                                    .map(|response| response.sense.senses_drag()),
                            )
                        })
                        .collect::<Vec<_>>();
                },
            );
            eprintln!(
                "SourceField geometry original frame {frame}: state={}; sizes={sizes:?}; pins={drag_responses:?}; shapes={:?}",
                source_field_state(&fx, &snarl),
                output.shapes
            );
            assert!(!drag_responses.is_empty());
            assert!(drag_responses.iter().all(|(_, drag)| *drag == Some(true)));
            recent.push(
                *sizes
                    .get(&CanvasNode::Graph(0))
                    .expect("this frame rendered the source node"),
            );
            assert_eq!(source_field_state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
        for size in &recent[4..] {
            assert!(
                size.x <= 160.0 && size.y <= 160.0,
                "closed source node still needs its old 170-pixel inline path editor: {size:?}"
            );
        }
        for pair in recent[4..].windows(2) {
            assert!(
                (pair[0] - pair[1]).length() < 0.5,
                "source node grows across repaints"
            );
        }
    }
}

fn join_field_fixture(collection: Vec<String>, path: Vec<String>) -> (Fixture, Snarl<CanvasNode>) {
    let (mut fx, snarl) = source_field_fixture(Vec::new(), None);
    fx.graph.nodes.insert(
        0,
        Node::JoinField {
            join: mapping::JoinId::new(7),
            collection,
            path,
        },
    );
    (fx, snarl)
}

fn join_field_header_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    protected: Option<NodeId>,
    frame: &mut usize,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let node = fx.call;
    let before = source_field_state(fx, snarl);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(*frame as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.protected_output = protected;
            viewer.show_header(node, &[], &[], ui, snarl);
        },
    );
    eprintln!(
        "JoinField header original frame {frame}: before={before}; after={}; shapes={:?}; accessibility={:?}",
        source_field_state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update
    );
    *frame += 1;
    output
}

#[test]
fn compact_join_field_headers_retain_separate_join_collection_path_and_output_identity() {
    let cases = [
        (
            vec!["full_collection_注文".repeat(4), "rows".into()],
            vec!["full_relative_資料".repeat(4), "keep (output)".into()],
            "#7 keep (output)",
        ),
        (Vec::new(), Vec::new(), "#7 current"),
    ];
    for (collection, path, summary) in cases {
        let collection_description = if collection.is_empty() {
            "<empty> (empty collection)".to_owned()
        } else {
            collection.join("/")
        };
        let path_description = if path.is_empty() {
            "<current> (empty path)".to_owned()
        } else {
            path.join("/")
        };
        let identity = format!(
            "\nJoin: #7\nCollection: {collection_description}\nPath: {path_description}\nRead-only joined field"
        );
        for protected in [None, Some(42), Some(0)] {
            let (mut fx, mut snarl) = join_field_fixture(collection.clone(), path.clone());
            let before = source_field_state(&fx, &snarl);
            let base_title = fx.viewer().title(&CanvasNode::Graph(0));
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let mut frame = 0;
            let mut titles = Vec::new();
            let mut output = join_field_header_frame(
                &mut fx,
                &mut snarl,
                &context,
                protected,
                &mut frame,
                Vec::new(),
            );
            for _ in 0..4 {
                if let Some(update) = &output.platform_output.accesskit_update {
                    titles.extend(update.nodes.iter().filter_map(|(_, node)| {
                        node.value()
                            .filter(|label| label.contains(&identity))
                            .map(str::to_owned)
                    }));
                }
                output = join_field_header_frame(
                    &mut fx,
                    &mut snarl,
                    &context,
                    protected,
                    &mut frame,
                    Vec::new(),
                );
            }
            let title = titles.first().expect("joined context is fully accessible");
            assert_eq!(
                title,
                &format!(
                    "{base_title}{}{identity}",
                    if protected == Some(0) {
                        " (output)"
                    } else {
                        ""
                    }
                )
            );
            let marked = format!("{summary} · output");
            assert!(
                text_position(
                    &output,
                    if protected == Some(0) {
                        &marked
                    } else {
                        summary
                    }
                )
                .is_some()
            );
            assert!(
                text_position(
                    &output,
                    if protected == Some(0) {
                        summary
                    } else {
                        &marked
                    }
                )
                .is_none(),
                "output identity alone adds the marker"
            );
            let icon = char::from(lucide_icons::Icon::ArrowRightFromLine).to_string();
            let hover = text_position(&output, &icon).expect("joined field icon");
            for hover_frame in 0..8 {
                output = join_field_header_frame(
                    &mut fx,
                    &mut snarl,
                    &context,
                    protected,
                    &mut frame,
                    if hover_frame == 0 {
                        vec![egui::Event::PointerMoved(hover)]
                    } else {
                        Vec::new()
                    },
                );
            }
            assert!(
                text_position(&output, title).is_some(),
                "full collection and relative path stay in the tooltip"
            );
            assert_eq!(source_field_state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
    }
}

#[test]
fn compact_join_field_properties_keep_the_existing_read_only_label_and_reopen() {
    let collection = vec!["orders".into(), "lines".into()];
    let path = vec!["details".into(), "資料📦".into()];
    let (mut fx, mut snarl) = join_field_fixture(collection, path);
    let before = source_field_state(&fx, &snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let label = "#7 orders/lines/details/資料📦";
    let closed = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    eprintln!(
        "JoinField closed properties original: state={before}; shapes={:?}",
        closed.shapes
    );
    assert!(
        text_position(&closed, label).is_none(),
        "full joined path is no longer an inline width floor"
    );
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    click_properties(
        &mut fx,
        &mut snarl,
        &context,
        text_position(&closed, &pencil).expect("existing property control"),
    );
    let mut opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        opened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    eprintln!(
        "JoinField opened properties original: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        opened.shapes
    );
    let read_only =
        text_position(&opened, label).expect("unchanged joined field label is in the popover");
    click_properties(&mut fx, &mut snarl, &context, read_only);
    let attempted = property_frame(
        &mut fx,
        &mut snarl,
        &context,
        vec![egui::Event::Text("changed".into())],
    )
    .0;
    eprintln!(
        "JoinField attempted edit original: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        attempted.shapes
    );
    assert_eq!(source_field_state(&fx, &snarl), before);
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
    eprintln!(
        "JoinField Escape original: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        closed.shapes
    );
    assert!(text_position(&closed, label).is_none());
    click_properties(
        &mut fx,
        &mut snarl,
        &context,
        text_position(&closed, &pencil).expect("property control after close"),
    );
    let mut reopened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    for _ in 0..3 {
        reopened = property_frame(&mut fx, &mut snarl, &context, Vec::new()).0;
    }
    eprintln!(
        "JoinField reopened original: state={}; shapes={:?}",
        source_field_state(&fx, &snarl),
        reopened.shapes
    );
    assert!(text_position(&reopened, label).is_some());
    assert_eq!(source_field_state(&fx, &snarl), before);
    assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
    assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
}

#[test]
fn compact_join_field_settled_width_does_not_depend_on_hidden_collection_or_path_lengths() {
    let mut settled = Vec::new();
    for long in [false, true] {
        let (collection, path) = if long {
            (
                vec!["complete_collection_資料".repeat(12), "lines".into()],
                vec!["complete_relative_注文".repeat(12), "name".into()],
            )
        } else {
            (vec!["rows".into()], vec!["name".into()])
        };
        let (mut fx, mut snarl) = join_field_fixture(collection, path);
        let before = source_field_state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let mut sizes = std::collections::BTreeMap::new();
        let mut recent = Vec::new();
        for frame in 0..8 {
            sizes.clear();
            let mut pins = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    time: Some(frame as f64 * 0.1),
                    ..Default::default()
                },
                |ui| {
                    let mut viewer = fx.viewer();
                    viewer.node_sizes = Some(&mut sizes);
                    SnarlWidget::new()
                        .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                        .show(&mut snarl, &mut viewer, ui);
                    pins = viewer
                        .pin_interaction_ids
                        .iter()
                        .map(|id| {
                            (
                                format!("{id:?}"),
                                ui.ctx()
                                    .read_response(*id)
                                    .map(|response| response.sense.senses_drag()),
                            )
                        })
                        .collect::<Vec<_>>();
                },
            );
            eprintln!(
                "JoinField settled geometry original long={long} frame={frame}: state={}; sizes={sizes:?}; pins={pins:?}; shapes={:?}",
                source_field_state(&fx, &snarl),
                output.shapes
            );
            assert!(!pins.is_empty() && pins.iter().all(|(_, drag)| *drag == Some(true)));
            recent.push(
                *sizes
                    .get(&CanvasNode::Graph(0))
                    .expect("rendered joined node"),
            );
            assert_eq!(source_field_state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
        for size in &recent[4..] {
            assert!(
                size.x <= 160.0 && size.y <= 160.0,
                "joined path still determines closed-node size: {size:?}"
            );
        }
        for pair in recent[4..].windows(2) {
            assert!(
                (pair[0] - pair[1]).length() < 0.5,
                "joined node grows after settling"
            );
        }
        settled.push(recent[7]);
    }
    assert!(
        (settled[0] - settled[1]).length() < 0.5,
        "hidden full identity still changes canvas geometry"
    );
}

fn lookup_popup_fixture(
    collection: Vec<String>,
    key: Vec<String>,
    value: Vec<String>,
) -> (Fixture, Snarl<CanvasNode>) {
    let fields = || {
        vec![
            SchemaNode::scalar("KeyA", ScalarType::Int),
            SchemaNode::scalar("KeyB", ScalarType::Int),
            SchemaNode::scalar("ValueA", ScalarType::String),
            SchemaNode::scalar("ValueB", ScalarType::String),
            SchemaNode::scalar("complete_lookup_key_資料".repeat(4), ScalarType::Int),
            SchemaNode::scalar("complete_lookup_value_注文".repeat(4), ScalarType::String),
        ]
    };
    let mut children = fields();
    for name in ["rowsA", "rows資料"] {
        children.push(SchemaNode::group(name, fields()).repeating());
    }
    if !collection.is_empty()
        && collection != ["rowsA".to_owned()]
        && collection != ["rows資料".to_owned()]
    {
        let mut group = SchemaNode::group(collection.last().unwrap(), fields()).repeating();
        for name in collection[..collection.len() - 1].iter().rev() {
            group = SchemaNode::group(name, vec![group]);
        }
        children.push(group);
    }
    let schema = SchemaNode::group("Root", children);
    let mut fx = fixture();
    fx.source_paths = SourcePathCatalog::new(&schema, &[]);
    fx.source_blocks = source_blocks(&schema);
    fx.graph.nodes.insert(
        0,
        Node::Lookup {
            collection,
            key,
            matches: 1,
            value,
        },
    );
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(7),
        },
    );
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let mut snarl = std::mem::take(&mut fx.snarl);
    let constant = snarl.insert_node(egui::pos2(0.0, 180.0), CanvasNode::Graph(1));
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
    snarl.connect(
        OutPinId {
            node: fx.call,
            output: 0,
        },
        InPinId {
            node: fx.target,
            input: 0,
        },
    );
    (fx, snarl)
}

fn lookup_header_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    protected: Option<NodeId>,
    frame: &mut usize,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let node = fx.call;
    let before = source_field_state(fx, snarl);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(*frame as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.protected_output = protected;
            viewer.show_header(node, &[], &[], ui, snarl);
        },
    );
    eprintln!(
        "Lookup header original frame {frame}: before={before}; after={}; shapes={:?}; accessibility={:?}",
        source_field_state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update
    );
    *frame += 1;
    output
}

fn lookup_property_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    controls: (Option<NodeId>, bool),
    frame: &mut usize,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let pin = snarl.out_pin(OutPinId {
        node: fx.call,
        output: 0,
    });
    let before = source_field_state(fx, snarl);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(*frame as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            ui.add_enabled_ui(controls.1, |ui| {
                let mut viewer = fx.viewer();
                viewer.protected_output = controls.0;
                viewer.show_output(&pin, ui, snarl);
            });
        },
    );
    eprintln!(
        "Lookup property original frame {frame}: controls={controls:?}; before={before}; after={}; shapes={:?}; accessibility={:?}",
        source_field_state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update
    );
    *frame += 1;
    output
}

fn click_lookup_property(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    controls: (Option<NodeId>, bool),
    frame: &mut usize,
    pos: egui::Pos2,
) {
    for pressed in [true, false] {
        lookup_property_frame(
            fx,
            snarl,
            context,
            controls,
            frame,
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

fn settle_lookup_properties(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    controls: (Option<NodeId>, bool),
    frame: &mut usize,
) -> egui::FullOutput {
    let mut output = lookup_property_frame(fx, snarl, context, controls, frame, Vec::new());
    for _ in 0..3 {
        output = lookup_property_frame(fx, snarl, context, controls, frame, Vec::new());
    }
    output
}

#[test]
fn compact_lookup_popup_edits_each_real_selector_and_preserves_other_fields_and_wires() {
    for protected in [None, Some(0)] {
        let (mut fx, mut snarl) = lookup_popup_fixture(
            vec!["rowsA".into()],
            vec!["KeyA".into()],
            vec!["ValueA".into()],
        );
        let context = egui::Context::default();
        crate::icons::install(&context);
        let controls = (protected, true);
        let mut frame = 0;
        let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
        let before = source_field_state(&fx, &snarl);
        let closed = settle_lookup_properties(&mut fx, &mut snarl, &context, controls, &mut frame);
        assert!(text_position(&closed, "collection").is_none());
        assert!(text_position(&closed, "KeyA").is_none());
        let edit = text_position(&closed, &pencil).expect("Lookup retains the property button");
        click_lookup_property(&mut fx, &mut snarl, &context, controls, &mut frame, edit);
        let mut opened =
            settle_lookup_properties(&mut fx, &mut snarl, &context, controls, &mut frame);
        assert_eq!(source_field_state(&fx, &snarl), before);
        for (field, selected, choice) in [
            ("collection", "rowsA", "rows資料"),
            ("key", "KeyA", "KeyB"),
            ("value", "ValueA", "ValueB"),
        ] {
            let before = source_field_state(&fx, &snarl);
            let mut expected_graph = fx.graph.clone();
            let selector =
                text_position(&opened, selected).expect("existing selected path is painted");
            click_lookup_property(
                &mut fx, &mut snarl, &context, controls, &mut frame, selector,
            );
            let choices =
                settle_lookup_properties(&mut fx, &mut snarl, &context, controls, &mut frame);
            let selected_choice =
                text_position(&choices, choice).expect("schema-backed choice is painted");
            click_lookup_property(
                &mut fx,
                &mut snarl,
                &context,
                controls,
                &mut frame,
                selected_choice,
            );
            opened = settle_lookup_properties(&mut fx, &mut snarl, &context, controls, &mut frame);
            let Node::Lookup {
                collection,
                key,
                value,
                ..
            } = expected_graph.nodes.get_mut(&0).unwrap()
            else {
                panic!("fixture is a Lookup");
            };
            match field {
                "collection" => *collection = vec![choice.into()],
                "key" => *key = vec![choice.into()],
                "value" => *value = vec![choice.into()],
                _ => unreachable!(),
            }
            let mut expected = before;
            expected["graph"] = serde_json::to_value(expected_graph).unwrap();
            assert_eq!(
                source_field_state(&fx, &snarl),
                expected,
                "only {field} changes"
            );
            assert!(
                text_position(&opened, "collection").is_some(),
                "nested selection keeps the editor open"
            );
            assert!(text_position(&opened, choice).is_some());
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
        let after = source_field_state(&fx, &snarl);
        lookup_property_frame(
            &mut fx,
            &mut snarl,
            &context,
            controls,
            &mut frame,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let closed = settle_lookup_properties(&mut fx, &mut snarl, &context, controls, &mut frame);
        assert!(text_position(&closed, "collection").is_none());
        let edit =
            text_position(&closed, &pencil).expect("properties remain available after Escape");
        click_lookup_property(&mut fx, &mut snarl, &context, controls, &mut frame, edit);
        let reopened =
            settle_lookup_properties(&mut fx, &mut snarl, &context, controls, &mut frame);
        for label in ["rows資料", "KeyB", "ValueB"] {
            assert!(text_position(&reopened, label).is_some());
        }
        assert_eq!(source_field_state(&fx, &snarl), after);
    }
}

#[test]
fn compact_lookup_popup_preserves_empty_unicode_output_identity_and_disabled_controls() {
    for (collection, protected) in [
        (Vec::new(), None),
        (vec!["complete_資料📦".repeat(3), "rows".into()], Some(0)),
        (vec!["keep (output)".into()], None),
        (vec!["keep (output)".into()], Some(0)),
    ] {
        let (key, value) = if collection.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            (vec!["KeyA".into()], vec!["ValueA".into()])
        };
        let (mut fx, mut snarl) = lookup_popup_fixture(collection.clone(), key, value);
        let before = source_field_state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        context.enable_accesskit();
        let mut frame = 0;
        let full_title = {
            let mut viewer = fx.viewer();
            viewer.protected_output = protected;
            viewer.title(&CanvasNode::Graph(0))
        };
        let mut accessible = Vec::new();
        let mut header = lookup_header_frame(
            &mut fx,
            &mut snarl,
            &context,
            protected,
            &mut frame,
            Vec::new(),
        );
        for _ in 0..4 {
            if let Some(update) = &header.platform_output.accesskit_update {
                accessible.extend(
                    update
                        .nodes
                        .iter()
                        .filter_map(|(_, node)| node.value().map(str::to_owned)),
                );
            }
            header = lookup_header_frame(
                &mut fx,
                &mut snarl,
                &context,
                protected,
                &mut frame,
                Vec::new(),
            );
        }
        eprintln!(
            "Lookup header originals: title={full_title:?}; accessible={accessible:?}; state={}; shapes={:?}",
            source_field_state(&fx, &snarl),
            header.shapes
        );
        assert!(
            accessible.contains(&full_title),
            "complete collection remains accessible"
        );
        let search = char::from(lucide_icons::Icon::Search).to_string();
        let hover = text_position(&header, &search).expect("Search header remains visible");
        for hover_frame in 0..8 {
            header = lookup_header_frame(
                &mut fx,
                &mut snarl,
                &context,
                protected,
                &mut frame,
                if hover_frame == 0 {
                    vec![egui::Event::PointerMoved(hover)]
                } else {
                    Vec::new()
                },
            );
        }
        assert!(
            text_position(&header, &full_title).is_some(),
            "hover retains the full untruncated collection"
        );
        if collection == ["keep (output)".to_owned()] {
            assert!(
                text_position(
                    &header,
                    if protected == Some(0) {
                        "keep (output) · output"
                    } else {
                        "keep (output)"
                    }
                )
                .is_some()
            );
            assert!(
                text_position(
                    &header,
                    if protected == Some(0) {
                        "keep (output)"
                    } else {
                        "keep (output) · output"
                    }
                )
                .is_none()
            );
        }
        let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
        let disabled = settle_lookup_properties(
            &mut fx,
            &mut snarl,
            &context,
            (protected, false),
            &mut frame,
        );
        let edit =
            text_position(&disabled, &pencil).expect("disabled property button remains painted");
        click_lookup_property(
            &mut fx,
            &mut snarl,
            &context,
            (protected, false),
            &mut frame,
            edit,
        );
        let disabled = settle_lookup_properties(
            &mut fx,
            &mut snarl,
            &context,
            (protected, false),
            &mut frame,
        );
        assert!(text_position(&disabled, "collection").is_none());
        assert_eq!(source_field_state(&fx, &snarl), before);
        let closed =
            settle_lookup_properties(&mut fx, &mut snarl, &context, (protected, true), &mut frame);
        let edit = text_position(&closed, &pencil).expect("enabled property button");
        click_lookup_property(
            &mut fx,
            &mut snarl,
            &context,
            (protected, true),
            &mut frame,
            edit,
        );
        let opened =
            settle_lookup_properties(&mut fx, &mut snarl, &context, (protected, true), &mut frame);
        assert!(text_position(&opened, "collection").is_some());
        assert!(text_position(&opened, "value").is_some());
        if collection.is_empty() {
            assert!(text_position(&opened, "<current rows>").is_some());
            assert!(text_position(&opened, "<item value>").is_some());
        } else {
            assert!(text_position(&opened, "KeyA").is_some());
            assert!(text_position(&opened, "ValueA").is_some());
        }
        click_lookup_property(
            &mut fx,
            &mut snarl,
            &context,
            (protected, true),
            &mut frame,
            egui::pos2(1150.0, 850.0),
        );
        let closed =
            settle_lookup_properties(&mut fx, &mut snarl, &context, (protected, true), &mut frame);
        assert!(
            text_position(&closed, "collection").is_none(),
            "outside click closes properties"
        );
        let edit = text_position(&closed, &pencil).expect("property button after outside click");
        click_lookup_property(
            &mut fx,
            &mut snarl,
            &context,
            (protected, true),
            &mut frame,
            edit,
        );
        let reopened =
            settle_lookup_properties(&mut fx, &mut snarl, &context, (protected, true), &mut frame);
        assert!(text_position(&reopened, "collection").is_some());
        assert_eq!(source_field_state(&fx, &snarl), before);
    }
}

#[test]
fn compact_lookup_settled_geometry_is_independent_of_hidden_key_and_value_lengths() {
    let mut settled = Vec::new();
    for long in [false, true] {
        let (key, value) = if long {
            (
                "complete_lookup_key_資料".repeat(4),
                "complete_lookup_value_注文".repeat(4),
            )
        } else {
            ("KeyA".into(), "ValueA".into())
        };
        let (mut fx, mut snarl) =
            lookup_popup_fixture(vec!["rowsA".into()], vec![key], vec![value]);
        let before = source_field_state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let mut recent = Vec::new();
        for frame in 0..8 {
            let mut sizes = std::collections::BTreeMap::new();
            let mut pins = Vec::new();
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    time: Some(frame as f64 * 0.1),
                    ..Default::default()
                },
                |ui| {
                    let mut viewer = fx.viewer();
                    viewer.node_sizes = Some(&mut sizes);
                    SnarlWidget::new()
                        .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                        .show(&mut snarl, &mut viewer, ui);
                    pins = viewer
                        .pin_interaction_ids
                        .iter()
                        .map(|id| {
                            (
                                format!("{id:?}"),
                                ui.ctx()
                                    .read_response(*id)
                                    .map(|response| response.sense.senses_drag()),
                            )
                        })
                        .collect::<Vec<_>>();
                },
            );
            eprintln!(
                "Lookup settled geometry original long={long} frame={frame}: state={}; sizes={sizes:?}; pins={pins:?}; shapes={:?}",
                source_field_state(&fx, &snarl),
                output.shapes
            );
            assert!(!pins.is_empty() && pins.iter().all(|(_, drag)| *drag == Some(true)));
            assert!(!fx.viewer().has_body(&CanvasNode::Graph(0)));
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
            assert_eq!(source_field_state(&fx, &snarl), before);
            recent.push(
                *sizes
                    .get(&CanvasNode::Graph(0))
                    .expect("Lookup is rendered"),
            );
        }
        for size in &recent[4..] {
            assert!(size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0);
        }
        for pair in recent[4..].windows(2) {
            assert!(
                (pair[0] - pair[1]).length() < 0.5,
                "Lookup changes size after settling"
            );
        }
        settled.push(recent[7]);
    }
    assert!(
        (settled[0] - settled[1]).length() < 0.5,
        "hidden picker text still changes closed-node geometry"
    );
}
