use super::*;

fn dynamic_fixture(
    object: Vec<String>,
    frame: Option<Vec<String>>,
) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        0,
        Node::DynamicSourceField {
            object,
            frame,
            key: 7,
        },
    );
    fx.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::String("property".into()),
        },
    );
    fx.graph
        .nodes
        .insert(42, Node::Const { value: Value::Null });
    fx.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![8],
        },
    );
    fx.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::String("unrelated".into()),
        },
    );
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let mut snarl = std::mem::take(&mut fx.snarl);
    snarl.get_node_info_mut(fx.source).unwrap().pos = egui::pos2(0.0, 400.0);
    snarl.get_node_info_mut(fx.target).unwrap().pos = egui::pos2(600.0, 0.0);
    let key = snarl.insert_node(egui::pos2(0.0, 180.0), CanvasNode::Graph(7));
    let unrelated_from = snarl.insert_node(egui::pos2(0.0, 600.0), CanvasNode::Graph(8));
    let unrelated_to = snarl.insert_node(egui::pos2(400.0, 600.0), CanvasNode::Graph(1));
    snarl.connect(
        OutPinId {
            node: key,
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
    snarl.connect(
        OutPinId {
            node: unrelated_from,
            output: 0,
        },
        InPinId {
            node: unrelated_to,
            input: 0,
        },
    );
    (fx, snarl)
}

fn state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> serde_json::Value {
    // DynamicSourceField has ordinary Vec/Option serde fields, including empty paths.
    // These canvas fixtures do not claim schema validation of arbitrary stored paths.
    serde_json::json!({
        "graph": &fx.graph,
        "root_scope": &fx.root_scope,
        "positions": format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
        "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    })
}

fn visible_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, clip: egui::Rect, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.visual_bounding_rect();
                (clip.intersects(rect) && clip.contains(rect.center())).then_some(rect.center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| find(shape, clip, label))
            }
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, shape.clip_rect, label))
}

fn input(context: &egui::Context, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 900.0),
        )),
        time: Some(context.cumulative_frame_nr() as f64 * 0.1),
        events,
        ..Default::default()
    }
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn header_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    protected: Option<NodeId>,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let node = fx.call;
    let output = context.run_ui(input(context, events), |ui| {
        let mut viewer = fx.viewer();
        viewer.protected_output = protected;
        viewer.show_header(node, &[], &[], ui, snarl);
    });
    eprintln!(
        "Dynamic header original: state={}; shapes={:?}; access={:?}",
        state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update
    );
    output
}

fn properties(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, egui::Id) {
    let pin = snarl.out_pin(OutPinId {
        node: fx.call,
        output: 0,
    });
    let mut button = egui::Id::NULL;
    let output = context.run_ui(input(context, events), |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            button = ui.next_auto_id();
            fx.viewer().show_output(&pin, ui, snarl);
        });
    });
    eprintln!(
        "Dynamic properties original: enabled={enabled}; state={}; shapes={:?}",
        state(fx, snarl),
        output.shapes
    );
    (output, button)
}

fn settle_properties(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
) -> egui::FullOutput {
    let mut output = properties(fx, snarl, context, enabled, Vec::new()).0;
    for _ in 0..3 {
        output = properties(fx, snarl, context, enabled, Vec::new()).0;
    }
    output
}

fn click_pencil(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
) {
    let output = settle_properties(fx, snarl, context, enabled);
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let pos =
        visible_position(&output, &pencil).expect("computed property has an actual pencil button");
    for pressed in [true, false] {
        properties(
            fx,
            snarl,
            context,
            enabled,
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
fn compact_dynamic_headers_retain_complete_object_frame_hover_accessibility_and_output_owner() {
    let cases = [
        (
            vec!["complete_parent_資料".repeat(4), "資料📦".into()],
            None,
            "dynamic 資料📦",
            "automatic".to_owned(),
        ),
        (
            vec!["root".into(), "資料📦".into()],
            Some(Vec::new()),
            "dynamic 資料📦",
            "explicit (empty path)".to_owned(),
        ),
        (
            vec!["object".into()],
            Some(vec!["complete_frame_注文".repeat(4), "owner".into()]),
            "dynamic object",
            format!("explicit {}/owner", "complete_frame_注文".repeat(4)),
        ),
        (Vec::new(), None, "dynamic current", "automatic".to_owned()),
        (
            Vec::new(),
            Some(Vec::new()),
            "dynamic current",
            "explicit (empty path)".to_owned(),
        ),
        (
            vec!["abcdefghijklmno資料📦long".into()],
            None,
            "dynamic abcdefghijklmno…",
            "automatic".to_owned(),
        ),
    ];
    for (object, frame, summary, frame_identity) in cases {
        let object_identity = if object.is_empty() {
            "<current> (empty path)".into()
        } else {
            object.join("/")
        };
        let identity = format!(
            "\nOpen source object: {object_identity}\nFrame: {frame_identity}\nThe property name is supplied by the input.\nRead-only computed property"
        );
        for protected in [None, Some(42), Some(0)] {
            let (mut fx, mut snarl) = dynamic_fixture(object.clone(), frame.clone());
            let before = state(&fx, &snarl);
            let saved = serde_json::to_vec(&fx.graph)
                .expect("actual empty object/frame serde is supported");
            let reopened: Graph = serde_json::from_slice(&saved).unwrap();
            assert_eq!(serde_json::to_vec(&reopened).unwrap(), saved);
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let marked = format!("{summary} · output");
            let expected_title = format!(
                "Dynamic field: {}{}{identity}",
                object.join("/"),
                if protected == Some(0) {
                    " (output)"
                } else {
                    ""
                }
            );
            let mut accessible = Vec::new();
            let mut output = header_frame(&mut fx, &mut snarl, &context, protected, Vec::new());
            for _ in 0..8 {
                if let Some(update) = &output.platform_output.accesskit_update {
                    accessible.extend(update.nodes.iter().filter_map(|(_, node)| {
                        node.value()
                            .filter(|value| value.contains(&identity))
                            .map(str::to_owned)
                    }));
                }
                output = header_frame(&mut fx, &mut snarl, &context, protected, Vec::new());
            }
            assert!(
                accessible.contains(&expected_title),
                "full accessible identity lost: {accessible:?}"
            );
            assert!(
                visible_position(
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
                visible_position(
                    &output,
                    if protected == Some(0) {
                        summary
                    } else {
                        &marked
                    }
                )
                .is_none()
            );
            let glyph = char::from(lucide_icons::Icon::Braces).to_string();
            let hover =
                visible_position(&output, &glyph).expect("computed property uses braces icon");
            for frame in 0..8 {
                output = header_frame(
                    &mut fx,
                    &mut snarl,
                    &context,
                    protected,
                    if frame == 0 {
                        vec![egui::Event::PointerMoved(hover)]
                    } else {
                        Vec::new()
                    },
                );
            }
            assert!(
                visible_position(&output, &expected_title).is_some(),
                "full hovered identity missing"
            );
            assert_eq!(state(&fx, &snarl), before);
            assert_eq!(fx.viewer().input_at(0, 0), Some(7));
            assert_eq!(fx.viewer().input_at(0, 1), None);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
    }
}

#[test]
fn compact_dynamic_read_only_details_open_close_and_reopen_without_changing_key_paths_or_wires() {
    for frame in [
        None,
        Some(Vec::new()),
        Some(vec!["orders".into(), "lines".into()]),
    ] {
        let object = vec!["details".into(), "資料📦".into()];
        let property_label = format!(
            "open source object: {}{}",
            frame
                .as_ref()
                .map(|path| format!("{}/", path.join("/")))
                .unwrap_or_default(),
            object.join("/")
        );
        let (mut fx, mut snarl) = dynamic_fixture(object, frame);
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let closed = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(
            visible_position(&closed, &property_label).is_none(),
            "read-only metadata remains in the closed node"
        );
        click_pencil(&mut fx, &mut snarl, &context, true);
        let opened = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(
            visible_position(&opened, &property_label).is_some(),
            "actual existing read-only details were lost"
        );
        assert_eq!(state(&fx, &snarl), before);
        properties(
            &mut fx,
            &mut snarl,
            &context,
            true,
            vec![key(egui::Key::Escape)],
        );
        let closed = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(visible_position(&closed, &property_label).is_none());
        let (_, button) = properties(&mut fx, &mut snarl, &context, true, Vec::new());
        context.memory_mut(|memory| memory.request_focus(button));
        properties(
            &mut fx,
            &mut snarl,
            &context,
            true,
            vec![key(egui::Key::Enter)],
        );
        let reopened = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(
            visible_position(&reopened, &property_label).is_some(),
            "keyboard did not reopen the real details button"
        );
        let saved = serde_json::to_vec(&fx.graph).unwrap();
        fx.graph = serde_json::from_slice(&saved).unwrap();
        assert_eq!(serde_json::to_vec(&fx.graph).unwrap(), saved);
        assert_eq!(state(&fx, &snarl), before);
        assert_eq!(fx.viewer().input_at(0, 0), Some(7));
        assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
        assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
    }
}

#[test]
fn compact_dynamic_disabled_inspection_and_unsupported_stored_paths_remain_passive_and_serializable()
 {
    // These unknown paths are serde-valid retained metadata, not engine-valid schema claims.
    let (mut fx, mut snarl) = dynamic_fixture(
        vec!["unavailable資料".into()],
        Some(vec!["missing-frame".into()]),
    );
    let before = state(&fx, &snarl);
    let bytes =
        serde_json::to_vec(&fx.graph).expect("unknown dynamic object paths use ordinary serde");
    let context = egui::Context::default();
    crate::icons::install(&context);
    let label = "open source object: missing-frame/unavailable資料";
    click_pencil(&mut fx, &mut snarl, &context, false);
    let closed = settle_properties(&mut fx, &mut snarl, &context, false);
    assert!(
        visible_position(&closed, label).is_none(),
        "disabled pencil opened the details popup"
    );
    click_pencil(&mut fx, &mut snarl, &context, true);
    let opened = settle_properties(&mut fx, &mut snarl, &context, true);
    assert!(visible_position(&opened, label).is_some());
    assert_eq!(serde_json::to_vec(&fx.graph).unwrap(), bytes);
    assert_eq!(state(&fx, &snarl), before);
    let reopened: Graph = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
    let Node::DynamicSourceField { key, .. } = &reopened.nodes[&0] else {
        panic!("read-only inspection changed node kind")
    };
    assert_eq!(*key, 7);
}

#[test]
fn compact_dynamic_settled_geometry_keeps_unicode_paths_out_of_closed_nodes_and_retains_pin_hover_wires()
 {
    let mut settled = Vec::new();
    for long in [false, true] {
        let (object, frame) = if long {
            (
                vec!["complete_object_資料📦".repeat(12), "資料📦".into()],
                Some(vec!["complete_frame_注文".repeat(12)]),
            )
        } else {
            (vec!["資料📦".into()], Some(vec!["rows".into()]))
        };
        let (mut fx, mut snarl) = dynamic_fixture(object, frame);
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let mut recent = Vec::new();
        let mut output = None;
        for _ in 0..8 {
            let mut sizes = std::collections::BTreeMap::new();
            let mut senses = Vec::new();
            let painted = context.run_ui(input(&context, Vec::new()), |ui| {
                let mut viewer = fx.viewer();
                viewer.node_sizes = Some(&mut sizes);
                SnarlWidget::new()
                    .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                    .show(&mut snarl, &mut viewer, ui);
                senses = viewer
                    .pin_interaction_ids
                    .iter()
                    .map(|id| {
                        ui.ctx()
                            .read_response(*id)
                            .map(|response| response.sense.senses_drag())
                    })
                    .collect();
            });
            eprintln!(
                "Dynamic geometry original: long={long}; state={}; sizes={sizes:?}; drag={senses:?}; shapes={:?}",
                state(&fx, &snarl),
                painted.shapes
            );
            assert!(!senses.is_empty() && senses.iter().all(|sense| *sense == Some(true)));
            assert!(
                visible_position(&painted, "property name").is_some(),
                "computed-key pin lost its label"
            );
            assert_eq!(fx.viewer().input_at(0, 0), Some(7));
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
            assert!(!fx.viewer().has_body(&CanvasNode::Graph(0)));
            assert_eq!(state(&fx, &snarl), before);
            recent.push(
                *sizes
                    .get(&CanvasNode::Graph(0))
                    .expect("computed property is rendered"),
            );
            output = Some(painted);
        }
        for size in &recent[4..] {
            assert!(size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0);
            assert!(
                size.x <= 220.0 && size.y <= 150.0,
                "closed computed property uses too much canvas space: {size:?}"
            );
        }
        for pair in recent[4..].windows(2) {
            assert!(
                (pair[0] - pair[1]).length() < 0.5,
                "node grows across settled repaints"
            );
        }
        settled.push(recent[7]);
        let glyph = char::from(lucide_icons::Icon::Braces).to_string();
        let hover = visible_position(&output.unwrap(), &glyph)
            .expect("computed property header icon visible");
        let mut detected = None;
        for _ in 0..2 {
            let _ = context.run_ui(
                input(&context, vec![egui::Event::PointerMoved(hover)]),
                |ui| {
                    let mut viewer = fx.viewer();
                    viewer.begin_node_hover_frame(detected);
                    SnarlWidget::new()
                        .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                        .show(&mut snarl, &mut viewer, ui);
                    detected = viewer.end_node_hover_frame();
                },
            );
        }
        eprintln!(
            "Dynamic hover original: wanted={:?}; actual={detected:?}; state={}",
            fx.call,
            state(&fx, &snarl)
        );
        assert_eq!(detected, Some(fx.call));
        let input_pin = snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        });
        let output_pin = snarl.out_pin(OutPinId {
            node: fx.call,
            output: 0,
        });
        let unrelated_node = snarl
            .nodes_pos_ids()
            .find_map(|(id, _, node)| (*node == CanvasNode::Graph(8)).then_some(id))
            .unwrap();
        let unrelated_pin = snarl.out_pin(OutPinId {
            node: unrelated_node,
            output: 0,
        });
        assert_eq!(input_pin.remotes.len(), 1);
        assert_eq!(snarl[input_pin.remotes[0].node], CanvasNode::Graph(7));
        assert_eq!(
            output_pin.remotes,
            vec![InPinId {
                node: fx.target,
                input: 0
            }]
        );
        assert_eq!(
            input_wire_emphasis(detected, &input_pin),
            WireEmphasis::Incident
        );
        assert_eq!(
            output_wire_emphasis(detected, &output_pin),
            WireEmphasis::Incident
        );
        assert_eq!(
            output_wire_emphasis(detected, &unrelated_pin),
            WireEmphasis::Unrelated
        );
        let mut viewer = fx.viewer();
        viewer.begin_node_hover_frame(None);
        let ordinary = viewer.output_wire_color(&output_pin);
        let unrelated = viewer.output_wire_color(&unrelated_pin);
        viewer.begin_node_hover_frame(detected);
        assert_eq!(
            viewer.output_wire_color(&output_pin),
            crate::wire_colors::with_emphasis(
                ordinary,
                viewer.colors.canvas.to_egui(),
                WireEmphasis::Incident
            )
        );
        let dimmed = viewer.output_wire_color(&unrelated_pin);
        assert_eq!(
            dimmed,
            crate::wire_colors::with_emphasis(
                unrelated,
                viewer.colors.canvas.to_egui(),
                WireEmphasis::Unrelated
            )
        );
        assert!(dimmed.a() < unrelated.a());
        assert_eq!(state(&fx, &snarl), before);
    }
    assert!(
        (settled[0] - settled[1]).length() < 0.5,
        "hidden Unicode object/frame identity changes closed geometry: {settled:?}"
    );
}
