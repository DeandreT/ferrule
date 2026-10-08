use super::*;

#[path = "compact_value_map_nodes/clipboard.rs"]
mod clipboard;

fn map_fixture(
    input_type: Option<ScalarType>,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        0,
        Node::ValueMap {
            input: 7,
            input_type,
            table,
            default,
        },
    );
    fx.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Int(1),
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
    fx.graph.nodes.insert(
        10,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let mut snarl = std::mem::take(&mut fx.snarl);
    snarl.get_node_info_mut(fx.source).unwrap().pos = egui::pos2(0.0, 400.0);
    snarl.get_node_info_mut(fx.target).unwrap().pos = egui::pos2(600.0, 0.0);
    let input = snarl.insert_node(egui::pos2(0.0, 180.0), CanvasNode::Graph(7));
    let unrelated_from = snarl.insert_node(egui::pos2(0.0, 650.0), CanvasNode::Graph(8));
    let unrelated_to = snarl.insert_node(egui::pos2(400.0, 650.0), CanvasNode::Graph(1));
    let fanout = snarl.insert_node(egui::pos2(600.0, 220.0), CanvasNode::Graph(10));
    for (from, to, input_index) in [
        (input, fx.call, 0),
        (fx.call, fx.target, 0),
        (fx.call, fanout, 0),
        (unrelated_from, unrelated_to, 0),
    ] {
        snarl.connect(
            OutPinId {
                node: from,
                output: 0,
            },
            InPinId {
                node: to,
                input: input_index,
            },
        );
    }
    (fx, snarl)
}

fn table(fx: &Fixture) -> &[(Value, Value)] {
    let Node::ValueMap { table, .. } = &fx.graph.nodes[&0] else {
        panic!("value map node changed")
    };
    table
}

fn default(fx: &Fixture) -> &Option<Value> {
    let Node::ValueMap { default, .. } = &fx.graph.nodes[&0] else {
        panic!("value map node changed")
    };
    default
}

fn raw_value(value: &Value) -> String {
    match value {
        Value::Float(value) => format!("Float({value:?}, bits={:016x})", value.to_bits()),
        value => format!("{value:?}"),
    }
}

fn state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> serde_json::Value {
    let Node::ValueMap {
        input_type,
        table,
        default,
        ..
    } = &fx.graph.nodes[&0]
    else {
        panic!("value map node changed")
    };
    // The explicit bits retain nonfinite and signed-zero values that ordinary JSON cannot describe.
    serde_json::json!({
        "graph": &fx.graph,
        "root": &fx.root_scope,
        "typed_table": table.iter().map(|(from, to)| (raw_value(from), raw_value(to))).collect::<Vec<_>>(),
        "typed_default": default.as_ref().map(raw_value),
        "input_type": format!("{input_type:?}"),
        "positions": format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
        "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    })
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn collect(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        out: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => out.push((
                text.galley.text().to_owned(),
                text.visual_bounding_rect(),
                clip,
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut out);
    }
    out
}

fn positions(output: &egui::FullOutput, text: &str) -> Vec<egui::Pos2> {
    texts(output)
        .into_iter()
        .filter_map(|(value, rect, clip)| {
            (value == text && clip.intersects(rect) && clip.contains(rect.center()))
                .then_some(rect.center())
        })
        .collect()
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

fn key_with(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn key(key: egui::Key) -> egui::Event {
    key_with(key, egui::Modifiers::NONE)
}

fn command() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    }
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
        "ValueMap properties original: enabled={enabled}; state={}; text={:?}; commands={:?}; focus={:?}",
        state(fx, snarl),
        texts(&output),
        output.platform_output.commands,
        context.memory(|memory| memory.focused())
    );
    (output, button)
}

fn settle(
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

fn click(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
    pos: egui::Pos2,
) {
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

fn open(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
) -> egui::FullOutput {
    let closed = settle(fx, snarl, context, enabled);
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let pos = positions(&closed, &pencil)[0];
    click(fx, snarl, context, enabled, pos);
    settle(fx, snarl, context, enabled)
}

fn replace_cell(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    shown: &str,
    value: &str,
) -> egui::FullOutput {
    let output = settle(fx, snarl, context, true);
    let pos = positions(&output, shown)[0];
    click(fx, snarl, context, true, pos);
    properties(
        fx,
        snarl,
        context,
        true,
        vec![
            key_with(egui::Key::A, command()),
            egui::Event::Paste(value.into()),
        ],
    )
    .0
}

#[test]
fn compact_value_map_headers_retain_complete_policy_accessibility_stationary_tooltip_and_typed_inspection()
 {
    let cases = [
        (
            None,
            Vec::new(),
            None,
            "0 entries",
            "automatic (runtime type)",
            "null (no default)",
        ),
        (
            Some(ScalarType::String),
            vec![(Value::Int(123), Value::Bool(true))],
            Some(Value::Null),
            "1 entry",
            "string",
            "null (stored default)",
        ),
        (
            Some(ScalarType::Int),
            (0..41)
                .map(|index| {
                    (
                        Value::String(format!("key-{index:02}")),
                        Value::String(format!("value-{index:02}")),
                    )
                })
                .collect(),
            Some(Value::String(String::new())),
            "41 entries",
            "int",
            "empty string (stored default)",
        ),
        (
            Some(ScalarType::Float),
            vec![
                (
                    Value::Float(f64::from_bits(0x7ff8000000000001)),
                    Value::Float(f64::INFINITY),
                ),
                (Value::Float(f64::NEG_INFINITY), Value::Float(-0.0)),
            ],
            Some(Value::Float(-0.0)),
            "2 entries",
            "float",
            "float: -0 (stored default)",
        ),
        (
            Some(ScalarType::Bool),
            vec![(Value::Bool(false), Value::String("資料📦".repeat(20)))],
            Some(Value::Bool(true)),
            "1 entry",
            "bool",
            "bool: true (stored default)",
        ),
        (
            None,
            vec![
                (Value::Null, Value::json_null()),
                (Value::xml_nil(), Value::String(String::new())),
            ],
            Some(Value::json_null()),
            "2 entries",
            "automatic (runtime type)",
            "json null: json:null (stored default)",
        ),
    ];
    for (input_type, table, default, summary, type_name, unmatched) in cases {
        for protected in [None, Some(42), Some(0)] {
            let (mut fx, mut snarl) = map_fixture(input_type, table.clone(), default.clone());
            let before = state(&fx, &snarl);
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let output_title = if protected == Some(0) {
                "Value Map (output)"
            } else {
                "Value Map"
            };
            let expected_title = format!(
                "{output_title}\nEntries: {}\nInput type: {type_name}\nUnmatched value: {unmatched}\nFirst matching entry wins. Edit the table with the pencil.",
                table.len()
            );
            let expected_summary = if protected == Some(0) {
                format!("{summary} · output")
            } else {
                summary.into()
            };
            let mut accessible = false;
            let mut output = None;
            for _ in 0..2 {
                let node = fx.call;
                let painted = context.run_ui(input(&context, Vec::new()), |ui| {
                    let mut viewer = fx.viewer();
                    viewer.protected_output = protected;
                    viewer.show_header(node, &[], &[], ui, &mut snarl);
                });
                eprintln!(
                    "ValueMap header original: state={}; text={:?}; access={:?}",
                    state(&fx, &snarl),
                    texts(&painted),
                    painted.platform_output.accesskit_update
                );
                if let Some(update) = &painted.platform_output.accesskit_update {
                    accessible |= update
                        .nodes
                        .iter()
                        .any(|(_, node)| node.value() == Some(expected_title.as_str()));
                }
                output = Some(painted);
            }
            let mut output = output.unwrap();
            assert!(
                accessible,
                "complete policy identity missing from AccessKit"
            );
            assert!(!positions(&output, &expected_summary).is_empty());
            let icon = char::from(lucide_icons::Icon::ArrowRightLeft).to_string();
            let hover = positions(&output, &icon)[0];
            for frame in 0..8 {
                let node = fx.call;
                output = context.run_ui(
                    input(
                        &context,
                        if frame == 0 {
                            vec![egui::Event::PointerMoved(hover)]
                        } else {
                            Vec::new()
                        },
                    ),
                    |ui| {
                        let mut viewer = fx.viewer();
                        viewer.protected_output = protected;
                        viewer.show_header(node, &[], &[], ui, &mut snarl);
                    },
                );
                eprintln!(
                    "ValueMap tooltip original: text={:?}; state={}",
                    texts(&output),
                    state(&fx, &snarl)
                );
            }
            assert!(
                !positions(&output, &expected_title).is_empty(),
                "full real stationary tooltip missing"
            );
            let opened = open(&mut fx, &mut snarl, &context, true);
            assert!(!positions(&opened, "Default").is_empty());
            assert_eq!(
                state(&fx, &snarl),
                before,
                "inspection converted a typed entry/default"
            );
            assert_eq!(fx.viewer().input_at(0, 0), Some(7));
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 1);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
            assert!(!fx.viewer().has_body(&CanvasNode::Graph(0)));
        }
    }
}

#[test]
fn compact_value_map_real_add_remove_and_default_controls_keep_exact_order_and_empty_policy() {
    let (mut fx, mut snarl) = map_fixture(None, Vec::new(), None);
    let context = egui::Context::default();
    crate::icons::install(&context);
    open(&mut fx, &mut snarl, &context, true);
    let plus = char::from(lucide_icons::Icon::Plus).to_string();
    let trash = char::from(lucide_icons::Icon::Trash2).to_string();
    for expected in [1, 2] {
        let output = settle(&mut fx, &mut snarl, &context, true);
        click(
            &mut fx,
            &mut snarl,
            &context,
            true,
            positions(&output, &plus)[0],
        );
        let output = settle(&mut fx, &mut snarl, &context, true);
        assert_eq!(
            table(&fx),
            vec![(Value::String(String::new()), Value::String(String::new())); expected]
        );
        assert!(
            !positions(
                &output,
                if expected == 1 {
                    "1 entry"
                } else {
                    "2 entries"
                }
            )
            .is_empty()
        );
    }
    for expected in [1, 0] {
        let output = settle(&mut fx, &mut snarl, &context, true);
        click(
            &mut fx,
            &mut snarl,
            &context,
            true,
            positions(&output, &trash)[0],
        );
        settle(&mut fx, &mut snarl, &context, true);
        assert_eq!(table(&fx).len(), expected);
    }
    let output = settle(&mut fx, &mut snarl, &context, true);
    click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        positions(&output, "Default")[0],
    );
    settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(default(&fx), &Some(Value::String(String::new())));
    let output = settle(&mut fx, &mut snarl, &context, true);
    click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        positions(&output, "Default")[0],
    );
    settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(default(&fx), &None);

    let entries = vec![
        (Value::String("duplicate".into()), Value::Int(11)),
        (Value::String("duplicate".into()), Value::Int(22)),
        (Value::String("last".into()), Value::Int(33)),
    ];
    let (mut fx, mut snarl) = map_fixture(
        Some(ScalarType::String),
        entries.clone(),
        Some(Value::Bool(true)),
    );
    let context = egui::Context::default();
    crate::icons::install(&context);
    let wires = snarl.wires().collect::<Vec<_>>();
    let opened = open(&mut fx, &mut snarl, &context, true);
    let mut buttons = positions(&opened, &trash);
    buttons.sort_by(|left, right| left.y.total_cmp(&right.y).then(left.x.total_cmp(&right.x)));
    assert_eq!(buttons.len(), 3);
    click(&mut fx, &mut snarl, &context, true, buttons[1]);
    let output = settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(table(&fx), vec![entries[0].clone(), entries[2].clone()]);
    click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        positions(&output, "Default")[0],
    );
    let output = settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(default(&fx), &None);
    click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        positions(&output, "Default")[0],
    );
    settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(default(&fx), &Some(Value::String(String::new())));
    assert!(matches!(
        fx.graph.nodes[&0],
        Node::ValueMap {
            input_type: Some(ScalarType::String),
            input: 7,
            ..
        }
    ));
    assert_eq!(snarl.wires().collect::<Vec<_>>(), wires);
}

struct CanvasFrame {
    output: egui::FullOutput,
    transform: egui::emath::TSTransform,
    sizes: std::collections::BTreeMap<CanvasNode, egui::Vec2>,
    pins: Vec<(egui::Id, bool, egui::Pos2)>,
    hovered: Option<SnarlNodeId>,
}

fn canvas(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    search: &mut crate::canvas_search::CanvasSearchState,
    events: Vec<egui::Event>,
) -> CanvasFrame {
    let mut sizes = std::collections::BTreeMap::new();
    let mut transform = None;
    let mut pins = Vec::new();
    let mut hovered = None;
    let output = context.run_ui(input(context, events), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let mut viewer = fx.viewer();
            viewer.node_sizes = Some(&mut sizes);
            crate::canvas_keyboard::show(
                snarl,
                &mut viewer,
                search,
                crate::canvas_keyboard::CanvasOptions {
                    id_salt: egui::Id::new("compact-value-map-real-canvas"),
                    show_minimap: false,
                    view_generation: 0,
                    style: crate::appearance::EditorAppearance::default().to_snarl_style(),
                    focus: None,
                },
                ui,
            );
            transform = viewer.canvas_transform;
            hovered = viewer.hovered_node_this_frame;
            pins = crate::canvas_keyboard::current_pin_interaction_ids(
                context,
                egui::Id::new("compact-value-map-real-canvas"),
                0,
            )
            .iter()
            .map(|id| {
                let response = context
                    .read_response(*id)
                    .expect("registered actual pin response");
                let center = context
                    .layer_transform_to_global(response.layer_id)
                    .unwrap_or_default()
                    * response.rect.center();
                (*id, response.sense.senses_drag(), center)
            })
            .collect();
        });
    });
    eprintln!(
        "ValueMap canvas original: state={}; transform={transform:?}; sizes={sizes:?}; pins={pins:?}; hover={hovered:?}; text={:?}",
        state(fx, snarl),
        texts(&output)
    );
    CanvasFrame {
        output,
        transform: transform.expect("actual Snarl transform"),
        sizes,
        pins,
        hovered,
    }
}

fn wheel(delta: egui::Vec2) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    }
}

fn canvas_click(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    search: &mut crate::canvas_search::CanvasSearchState,
    pos: egui::Pos2,
) {
    for pressed in [true, false] {
        canvas(
            fx,
            snarl,
            context,
            search,
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
fn compact_value_map_real_canvas_geometry_pins_hover_and_popup_wheel_isolate_the_background() {
    let mut settled = Vec::new();
    for long in [false, true] {
        let entries = (0..41)
            .map(|index| {
                (
                    Value::String(format!("key-{index:02}")),
                    Value::String(if long {
                        format!("value-{index:02}-{}", "資料📦".repeat(40))
                    } else {
                        format!("value-{index:02}")
                    }),
                )
            })
            .collect();
        let (mut fx, mut snarl) = map_fixture(None, entries, None);
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let mut search = crate::canvas_search::CanvasSearchState::default();
        let mut recent = Vec::new();
        let mut frame = None;
        for _ in 0..8 {
            let next = canvas(&mut fx, &mut snarl, &context, &mut search, Vec::new());
            assert!(!next.pins.is_empty() && next.pins.iter().all(|(_, drag, _)| *drag));
            recent.push(next.sizes[&CanvasNode::Graph(0)]);
            frame = Some(next);
        }
        let mut frame = frame.unwrap();
        let size = recent[7];
        assert!(
            size.x.is_finite()
                && size.y.is_finite()
                && size.x > 0.0
                && size.y > 0.0
                && size.x <= 220.0
                && size.y <= 150.0,
            "closed table uses too much space: {size:?}"
        );
        for pair in recent[4..].windows(2) {
            assert!((pair[0] - pair[1]).length() < 0.5);
        }
        settled.push(size);
        assert!(positions(&frame.output, "key-00").is_empty());
        let icon = char::from(lucide_icons::Icon::ArrowRightLeft).to_string();
        let hover = positions(&frame.output, &icon)[0];
        for _ in 0..2 {
            frame = canvas(
                &mut fx,
                &mut snarl,
                &context,
                &mut search,
                vec![egui::Event::PointerMoved(hover)],
            );
        }
        assert_eq!(frame.hovered, Some(fx.call));
        let input_pin = snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        });
        let output_pin = snarl.out_pin(OutPinId {
            node: fx.call,
            output: 0,
        });
        let unrelated = snarl
            .nodes_pos_ids()
            .find_map(|(id, _, node)| (*node == CanvasNode::Graph(8)).then_some(id))
            .unwrap();
        let unrelated_pin = snarl.out_pin(OutPinId {
            node: unrelated,
            output: 0,
        });
        assert_eq!(input_pin.remotes.len(), 1);
        assert_eq!(output_pin.remotes.len(), 2);
        assert_eq!(
            input_wire_emphasis(frame.hovered, &input_pin),
            WireEmphasis::Incident
        );
        assert_eq!(
            output_wire_emphasis(frame.hovered, &output_pin),
            WireEmphasis::Incident
        );
        assert_eq!(
            output_wire_emphasis(frame.hovered, &unrelated_pin),
            WireEmphasis::Unrelated
        );
        let mut viewer = fx.viewer();
        viewer.begin_node_hover_frame(None);
        let ordinary_input = viewer.input_wire_color(&input_pin, CanvasNode::Graph(0), 0);
        let ordinary_unrelated = viewer.output_wire_color(&unrelated_pin);
        viewer.begin_node_hover_frame(frame.hovered);
        assert_eq!(
            viewer.input_wire_color(&input_pin, CanvasNode::Graph(0), 0),
            crate::wire_colors::with_emphasis(
                ordinary_input,
                viewer.colors.canvas.to_egui(),
                WireEmphasis::Incident
            )
        );
        let dimmed = viewer.output_wire_color(&unrelated_pin);
        assert_eq!(
            dimmed,
            crate::wire_colors::with_emphasis(
                ordinary_unrelated,
                viewer.colors.canvas.to_egui(),
                WireEmphasis::Unrelated
            )
        );
        assert!(dimmed.a() < ordinary_unrelated.a());
        assert_eq!(state(&fx, &snarl), before);
        if long {
            continue;
        }

        let pin_ids = frame.pins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
        let original_wires = snarl.wires().collect::<Vec<_>>();
        let before_pan = frame.transform;
        frame = canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![wheel(egui::vec2(0.0, -40.0))],
        );
        assert!(
            (frame.transform.translation - before_pan.translation).length() > 0.1,
            "closed node swallowed vertical canvas wheel"
        );
        let before_horizontal = frame.transform;
        frame = canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![wheel(egui::vec2(36.0, 0.0))],
        );
        assert!(
            (frame.transform.translation.x - before_horizontal.translation.x).abs() > 0.1,
            "horizontal trackpad motion disappeared"
        );
        for _ in 0..4 {
            frame = canvas(&mut fx, &mut snarl, &context, &mut search, Vec::new());
        }
        let header = positions(&frame.output, &icon)[0];
        let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
        let edit = positions(&frame.output, &pencil)
            .into_iter()
            .min_by(|left, right| {
                left.distance_sq(header)
                    .total_cmp(&right.distance_sq(header))
            })
            .unwrap();
        canvas_click(&mut fx, &mut snarl, &context, &mut search, edit);
        for _ in 0..4 {
            frame = canvas(&mut fx, &mut snarl, &context, &mut search, Vec::new());
        }
        let scroll_at = positions(&frame.output, "key-00")[0];
        assert!(positions(&frame.output, "key-40").is_empty());
        let popup_transform = frame.transform;
        let mut visited = std::collections::BTreeSet::new();
        for _ in 0..24 {
            for index in 0..41 {
                if !positions(&frame.output, &format!("key-{index:02}")).is_empty() {
                    visited.insert(index);
                }
            }
            frame = canvas(
                &mut fx,
                &mut snarl,
                &context,
                &mut search,
                vec![
                    egui::Event::PointerMoved(scroll_at),
                    wheel(egui::vec2(0.0, -70.0)),
                ],
            );
            assert!(
                (frame.transform.translation - popup_transform.translation).length() < 0.001
                    && (frame.transform.scaling - popup_transform.scaling).abs() < 0.001,
                "popup wheel panned/zoomed the background"
            );
        }
        for _ in 0..4 {
            frame = canvas(&mut fx, &mut snarl, &context, &mut search, Vec::new());
        }
        for index in 0..41 {
            if !positions(&frame.output, &format!("key-{index:02}")).is_empty() {
                visited.insert(index);
            }
        }
        assert_eq!(visited, (0..41).collect());
        assert!(!positions(&frame.output, "key-40").is_empty());
        assert!(positions(&frame.output, "key-00").is_empty());
        let last = positions(&frame.output, "key-40")[0];
        canvas_click(&mut fx, &mut snarl, &context, &mut search, last);
        assert!(context.egui_wants_keyboard_input());
        canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![key(egui::Key::Backspace)],
        );
        assert!(
            matches!(fx.graph.nodes.get(&0), Some(Node::ValueMap { .. })),
            "text Backspace removed its graph node"
        );
        assert_eq!(snarl.wires().collect::<Vec<_>>(), original_wires);
        canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![key(egui::Key::Escape)],
        );
        for _ in 0..4 {
            frame = canvas(&mut fx, &mut snarl, &context, &mut search, Vec::new());
        }
        assert!(positions(&frame.output, "Default").is_empty());
        let closed_at = positions(&frame.output, &icon)[0];
        frame = canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![egui::Event::PointerMoved(closed_at)],
        );
        let closed_transform = frame.transform;
        frame = canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![wheel(egui::vec2(0.0, -30.0))],
        );
        assert!(
            (frame.transform.translation - closed_transform.translation).length() > 0.1,
            "closed popup kept intercepting wheel"
        );
        assert_eq!(
            frame.pins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            pin_ids
        );
        let literal = positions(&frame.output, "1")[0];
        let (drag_id, _, drag_at) = frame
            .pins
            .iter()
            .copied()
            .min_by(|left, right| {
                left.2
                    .distance_sq(literal)
                    .total_cmp(&right.2.distance_sq(literal))
            })
            .unwrap();
        canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![
                egui::Event::PointerMoved(drag_at),
                egui::Event::PointerButton {
                    pos: drag_at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![egui::Event::PointerMoved(drag_at + egui::vec2(12.0, 0.0))],
        );
        eprintln!(
            "ValueMap actual pin drag original: expected={drag_id:?}; actual={:?}; wires={:?}",
            context.dragged_id(),
            snarl.wires().collect::<Vec<_>>()
        );
        assert_eq!(context.dragged_id(), Some(drag_id));
        canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![wheel(egui::vec2(10.0, 0.0))],
        );
        canvas(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            vec![egui::Event::PointerButton {
                pos: drag_at + egui::vec2(12.0, 0.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            snarl.wires().collect::<Vec<_>>(),
            original_wires,
            "cancelled real pin drag changed existing fanout"
        );
    }
    assert!(
        (settled[0] - settled[1]).length() < 0.5,
        "hidden cell text enlarged the closed node"
    );
}

fn project(fx: &Fixture) -> mapping::Project {
    let mut project = crate::new_mapping::blank_project();
    project.source = SchemaNode::group("source", Vec::new());
    project.target = SchemaNode::group(
        "target",
        vec![SchemaNode::scalar("out", ScalarType::String)],
    );
    project.graph = fx.graph.clone();
    project.root = fx.root_scope.clone();
    project
}

fn run(fx: &Fixture) -> Value {
    let result = engine::run(&project(fx), &ir::Instance::Group(Vec::new().into()));
    eprintln!(
        "ValueMap native original: project={:?}; result={result:?}",
        project(fx)
    );
    result
        .unwrap()
        .field("out")
        .and_then(ir::Instance::as_scalar)
        .unwrap()
        .clone()
}

#[test]
fn compact_value_map_real_edits_roundtrip_with_public_mapping_output_and_saved_snapshot_history() {
    let entries = vec![
        (
            Value::String("1".into()),
            Value::String("before-first".into()),
        ),
        (
            Value::String("1".into()),
            Value::String("before-second".into()),
        ),
    ];
    let (mut fx, mut snarl) = map_fixture(Some(ScalarType::String), entries, None);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let wires = snarl.wires().collect::<Vec<_>>();
    let saved = serde_json::to_vec(&fx.graph).unwrap();
    let mut history =
        editor_ui::SnapshotHistory::new(saved.clone(), editor_ui::DocumentOrigin::Saved);
    assert_eq!(run(&fx), Value::String("before-first".into()));
    open(&mut fx, &mut snarl, &context, true);
    replace_cell(&mut fx, &mut snarl, &context, "before-first", "after資料📦");
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![key(egui::Key::Escape)],
    );
    settle(&mut fx, &mut snarl, &context, true);
    let edited = serde_json::to_vec(&fx.graph).unwrap();
    eprintln!(
        "ValueMap real edit snapshot originals: saved={}; edited={}",
        String::from_utf8_lossy(&saved),
        String::from_utf8_lossy(&edited)
    );
    assert_ne!(edited, saved);
    assert_eq!(table(&fx)[1].1, Value::String("before-second".into()));
    assert_eq!(run(&fx), Value::String("after資料📦".into()));
    history.record(saved.clone(), "Edit Value map");
    assert!(history.is_dirty(&edited));
    // This uses the app's public snapshot adapter, not an App coalescing/shortcut claim.
    let restored = history.undo(edited.clone()).unwrap().into_snapshot();
    fx.graph = serde_json::from_slice(&restored).unwrap();
    assert!(!history.is_dirty(&restored));
    assert_eq!(run(&fx), Value::String("before-first".into()));
    let restored = history.redo(restored).unwrap().into_snapshot();
    fx.graph = serde_json::from_slice(&restored).unwrap();
    assert_eq!(serde_json::to_vec(&fx.graph).unwrap(), edited);
    assert!(history.is_dirty(&restored));
    assert_eq!(run(&fx), Value::String("after資料📦".into()));
    assert_eq!(snarl.wires().collect::<Vec<_>>(), wires);

    let (mut fx, mut snarl) = map_fixture(
        None,
        vec![(Value::String("known".into()), Value::String("hit".into()))],
        None,
    );
    let context = egui::Context::default();
    crate::icons::install(&context);
    assert_eq!(run(&fx), Value::Null);
    let opened = open(&mut fx, &mut snarl, &context, true);
    click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        positions(&opened, "Default")[0],
    );
    settle(&mut fx, &mut snarl, &context, true);
    assert_eq!(default(&fx), &Some(Value::String(String::new())));
    assert_eq!(run(&fx), Value::String(String::new()));
}
