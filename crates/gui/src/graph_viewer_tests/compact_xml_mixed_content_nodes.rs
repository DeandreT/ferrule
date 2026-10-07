use super::*;

fn replacement(
    element: &str,
    collection: &[&str],
    expression: NodeId,
) -> mapping::XmlMixedContentReplacement {
    mapping::XmlMixedContentReplacement {
        element: element.into(),
        collection: collection.iter().map(|part| (*part).into()).collect(),
        expression,
    }
}

fn mixed_schema() -> SchemaNode {
    SchemaNode::group(
        "Description",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Bold", ScalarType::String).repeating(),
            SchemaNode::scalar("Italic", ScalarType::String).repeating(),
            SchemaNode::scalar("Plain", ScalarType::String).repeating(),
        ],
    )
}

fn mixed_fixture(
    path: Vec<String>,
    frame: Option<Vec<String>>,
    replacements: Vec<mapping::XmlMixedContentReplacement>,
) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    let source_schema = mixed_schema();
    fx.source_blocks = source_blocks(&source_schema);
    fx.source_paths = SourcePathCatalog::new(&source_schema, &[]);
    let expressions = replacements
        .iter()
        .map(|replacement| replacement.expression)
        .collect::<Vec<_>>();
    fx.graph.nodes.insert(
        0,
        Node::XmlMixedContent {
            path,
            frame,
            replacements,
        },
    );
    fx.graph
        .nodes
        .insert(42, Node::Const { value: Value::Null });
    fx.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![99],
        },
    );
    fx.graph.nodes.insert(
        99,
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
    let mut nodes = std::collections::BTreeMap::new();
    for expression in &expressions {
        if !nodes.contains_key(expression) {
            fx.graph
                .nodes
                .entry(*expression)
                .or_insert_with(|| Node::Const {
                    value: Value::String(format!("expression {expression}")),
                });
            let node = snarl.insert_node(
                egui::pos2(0.0, 180.0 + nodes.len() as f32 * 80.0),
                CanvasNode::Graph(*expression),
            );
            nodes.insert(*expression, node);
        }
    }
    for (input, expression) in expressions.iter().enumerate() {
        snarl.connect(
            OutPinId {
                node: nodes[expression],
                output: 0,
            },
            InPinId {
                node: fx.call,
                input,
            },
        );
    }
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
    let fanout = snarl.insert_node(egui::pos2(600.0, 220.0), CanvasNode::Graph(10));
    snarl.connect(
        OutPinId {
            node: fx.call,
            output: 0,
        },
        InPinId {
            node: fanout,
            input: 0,
        },
    );
    let unrelated_from = snarl.insert_node(egui::pos2(0.0, 720.0), CanvasNode::Graph(99));
    let unrelated_to = snarl.insert_node(egui::pos2(500.0, 720.0), CanvasNode::Graph(1));
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
    // Stored XML atomizer metadata uses ordinary serde. Passive path/slot fixtures
    // do not claim that arbitrary retained metadata passes schema validation.
    serde_json::json!({
        "graph": &fx.graph,
        "root_scope": &fx.root_scope,
        "positions": format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
        "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    })
}

fn text_shapes(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn collect(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        output: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => output.push((
                text.galley.text().to_owned(),
                text.visual_bounding_rect(),
                clip,
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, output);
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut texts);
    }
    texts
}

fn visible_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    text_shapes(output)
        .into_iter()
        .find_map(|(text, rect, clip)| {
            (text == label && clip.intersects(rect) && clip.contains(rect.center()))
                .then_some(rect.center())
        })
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
        "Mixed header original: state={}; texts={:?}; access={:?}",
        state(fx, snarl),
        text_shapes(&output),
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
        "Mixed details original: enabled={enabled}; state={}; texts={:?}",
        state(fx, snarl),
        text_shapes(&output)
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
    let glyph = char::from(lucide_icons::Icon::Pencil).to_string();
    let pos =
        visible_position(&output, &glyph).expect("mixed content has an actual details button");
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
fn compact_mixed_headers_retain_full_path_frame_count_tooltip_accessibility_and_output_owner() {
    let cases = [
        (
            Vec::new(),
            None,
            Vec::new(),
            "mixed current",
            "automatic".to_owned(),
            0,
        ),
        (
            vec!["Description".into()],
            Some(Vec::new()),
            vec![replacement("Italic", &[], 7)],
            "mixed Description",
            "explicit (empty path)".to_owned(),
            1,
        ),
        (
            vec!["complete_parent_資料".repeat(4), "資料📦".into()],
            None,
            vec![
                replacement("Italic", &["Italic"], 7),
                replacement("Bold", &["Bold"], 8),
            ],
            "mixed 資料📦",
            "automatic".to_owned(),
            2,
        ),
        (
            vec!["Description".into()],
            Some(vec!["complete_frame_注文".repeat(4), "owner".into()]),
            vec![replacement("Italic", &[], 7)],
            "mixed Description",
            format!("explicit {}/owner", "complete_frame_注文".repeat(4)),
            1,
        ),
        (
            Vec::new(),
            Some(Vec::new()),
            Vec::new(),
            "mixed current",
            "explicit (empty path)".to_owned(),
            0,
        ),
        (
            vec!["abcdefghijklmno資料📦long".into()],
            None,
            vec![
                replacement("Italic", &[], 7),
                replacement("Bold", &["Bold"], 8),
                replacement("Plain", &[], 9),
            ],
            "mixed abcdefghijklmno…",
            "automatic".to_owned(),
            3,
        ),
    ];
    for (path, frame, replacements, summary, frame_identity, count) in cases {
        let path_identity = if path.is_empty() {
            "<current> (empty path)".into()
        } else {
            path.join("/")
        };
        let identity = format!(
            "\nPath: {path_identity}\nFrame: {frame_identity}\nReplacements: {count}\nRead-only ordered XML content"
        );
        for protected in [None, Some(42), Some(0)] {
            let (mut fx, mut snarl) =
                mixed_fixture(path.clone(), frame.clone(), replacements.clone());
            let before = state(&fx, &snarl);
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let marked = format!("{summary} · output");
            let complete = format!(
                "XML mixed content: {}{}{identity}",
                path.join("/"),
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
                accessible.contains(&complete),
                "full mixed identity lost: {accessible:?}"
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
            let glyph = char::from(lucide_icons::Icon::FileCode).to_string();
            let hover =
                visible_position(&output, &glyph).expect("mixed header uses actual FileCode glyph");
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
                visible_position(&output, &complete).is_some(),
                "real stationary hover lost complete mixed identity"
            );
            assert_eq!(state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), count);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
    }
}

#[test]
fn compact_mixed_read_only_details_keep_order_context_and_real_keyboard_inspection_passive() {
    for frame in [None, Some(Vec::new()), Some(vec!["rows".into()])] {
        let (mut fx, mut snarl) = mixed_fixture(
            vec!["Description".into()],
            frame.clone(),
            vec![
                replacement("Italic", &[], 7),
                replacement("Bold", &["Bold"], 8),
            ],
        );
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let closed = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(visible_position(&closed, "Input 0: Italic").is_none());
        click_pencil(&mut fx, &mut snarl, &context, true);
        let opened = settle_properties(&mut fx, &mut snarl, &context, true);
        for label in [
            "Description (2 replacements)",
            "Input 0: Italic",
            "Collection: parent context (empty path)",
            "Expression: #7",
            "Input 1: Bold",
            "Collection: Bold",
            "Expression: #8",
        ] {
            assert!(
                visible_position(&opened, label).is_some(),
                "read-only mixed detail missing {label:?}"
            );
        }
        assert!(
            visible_position(&opened, "Input 0: Italic").unwrap().y
                < visible_position(&opened, "Input 1: Bold").unwrap().y
        );
        let frame_label = match frame {
            None => "Frame: automatic".to_owned(),
            Some(frame) if frame.is_empty() => "Frame: explicit (empty path)".to_owned(),
            Some(frame) => format!("Frame: explicit {}", frame.join("/")),
        };
        assert!(visible_position(&opened, &frame_label).is_some());
        properties(
            &mut fx,
            &mut snarl,
            &context,
            true,
            vec![key(egui::Key::Escape)],
        );
        let closed = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(visible_position(&closed, "Input 0: Italic").is_none());
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
        assert!(visible_position(&reopened, "Input 0: Italic").is_some());
        let saved = serde_json::to_vec(&fx.graph).unwrap();
        fx.graph = serde_json::from_slice(&saved).unwrap();
        assert_eq!(serde_json::to_vec(&fx.graph).unwrap(), saved);
        assert_eq!(state(&fx, &snarl), before);
        assert_eq!(fx.viewer().input_at(0, 0), Some(7));
        assert_eq!(fx.viewer().input_at(0, 1), Some(8));
        assert_eq!(fx.viewer().input_at(0, 2), None);
    }
}

#[test]
fn compact_mixed_disabled_unknown_metadata_and_actual_scrolling_retain_every_ordered_slot() {
    let collection_parent = "complete_collection_資料📦".repeat(3);
    let complete_collection = format!("{collection_parent}/leaf");
    let replacements = (0..24)
        .map(|slot| {
            if slot == 23 {
                replacement("element-23", &[collection_parent.as_str(), "leaf"], 43)
            } else {
                replacement(&format!("element-{slot}"), &[], 20 + slot)
            }
        })
        .collect();
    // Unknown paths/elements are passive serde metadata, not schema-admission claims.
    let (mut fx, mut snarl) = mixed_fixture(
        vec!["unavailable資料".into()],
        Some(Vec::new()),
        replacements,
    );
    let before = state(&fx, &snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    click_pencil(&mut fx, &mut snarl, &context, false);
    let closed = settle_properties(&mut fx, &mut snarl, &context, false);
    assert!(visible_position(&closed, "Input 0: element-0").is_none());
    click_pencil(&mut fx, &mut snarl, &context, true);
    let mut output = settle_properties(&mut fx, &mut snarl, &context, true);
    assert!(visible_position(&output, "unavailable資料 (24 replacements)").is_some());
    let anchor = visible_position(&output, "Input 0: element-0")
        .expect("first actual replacement is visible");
    assert!(
        visible_position(&output, "Input 23: element-23").is_none(),
        "unbounded replacement details escaped the scroll viewport"
    );
    fn retain_visible_slots(
        output: &egui::FullOutput,
        seen: &mut std::collections::BTreeSet<usize>,
    ) {
        let mut ordered = Vec::new();
        for (text, rect, clip) in text_shapes(output) {
            if let Some(slot) = text
                .strip_prefix("Input ")
                .and_then(|text| text.split(':').next())
                .and_then(|slot| slot.parse::<usize>().ok())
            {
                assert_eq!(text, format!("Input {slot}: element-{slot}"));
                assert!(
                    clip.height() <= 200.0,
                    "replacement viewport was not bounded: {clip:?}"
                );
                if clip.contains(rect.center()) {
                    seen.insert(slot);
                    ordered.push(slot);
                }
            }
        }
        assert!(
            ordered.windows(2).all(|pair| pair[0] < pair[1]),
            "visible replacement order changed: {ordered:?}"
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    retain_visible_slots(&output, &mut seen);
    // Real pointer/wheel input, with no ScrollArea state injection or scroll_to shortcut.
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![egui::Event::PointerMoved(anchor)],
    );
    for _ in 0..20 {
        output = properties(
            &mut fx,
            &mut snarl,
            &context,
            true,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        )
        .0;
        retain_visible_slots(&output, &mut seen);
    }
    for _ in 0..4 {
        output = properties(&mut fx, &mut snarl, &context, true, Vec::new()).0;
        retain_visible_slots(&output, &mut seen);
    }
    assert_eq!(seen, (0..24).collect());
    assert!(visible_position(&output, "Input 0: element-0").is_none());
    assert!(
        visible_position(&output, "Input 23: element-23").is_some(),
        "actual wheel scrolling did not expose the final slot"
    );
    assert!(visible_position(&output, "Expression: #43").is_some());
    assert!(
        visible_position(&output, &format!("Collection: {complete_collection}")).is_some(),
        "full Unicode collection identity was lost"
    );
    assert_eq!(state(&fx, &snarl), before);
    let saved = serde_json::to_vec(&fx.graph).unwrap();
    let reopened: Graph = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), saved);
    let Node::XmlMixedContent { replacements, .. } = &reopened.nodes[&0] else {
        panic!("inspection changed node kind")
    };
    assert_eq!(
        replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect::<Vec<_>>(),
        (20..44).collect::<Vec<_>>()
    );
}

#[test]
fn compact_mixed_closed_width_keeps_hidden_unicode_identity_out_of_nodes_and_preserves_blank_pins_fanout_hover()
 {
    let mut settled = Vec::new();
    for long in [false, true] {
        let path = if long {
            vec!["complete_parent_資料📦".repeat(12), "Description".into()]
        } else {
            vec!["Description".into()]
        };
        let frame = Some(if long {
            vec!["complete_frame_注文".repeat(12)]
        } else {
            vec!["rows".into()]
        });
        let collection = if long {
            "complete_collection_資料📦".repeat(12)
        } else {
            "Bold".into()
        };
        let (mut fx, mut snarl) = mixed_fixture(
            path,
            frame,
            vec![
                replacement("Italic", &[], 7),
                replacement("Bold", &[collection.as_str()], 8),
                replacement("Plain", &[collection.as_str()], 8),
            ],
        );
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        // Use the existing actual show_input route to witness unchanged blank labels.
        for slot in 0..3 {
            let pin = snarl.in_pin(InPinId {
                node: fx.call,
                input: slot,
            });
            let painted = context.run_ui(input(&context, Vec::new()), |ui| {
                fx.viewer().show_input(&pin, ui, &mut snarl);
            });
            eprintln!(
                "Mixed blank pin original: slot={slot}; texts={:?}",
                text_shapes(&painted)
            );
            assert!(
                text_shapes(&painted)
                    .iter()
                    .all(|(text, _, _)| text.is_empty()),
                "mixed pin acquired a label"
            );
        }
        let mut recent = Vec::new();
        let mut output = None;
        let mut stable_ids = None;
        for pass in 0..8 {
            let mut sizes = std::collections::BTreeMap::new();
            let mut pin_ids = Vec::new();
            let mut drag = Vec::new();
            let painted = context.run_ui(input(&context, Vec::new()), |ui| {
                let mut viewer = fx.viewer();
                viewer.node_sizes = Some(&mut sizes);
                SnarlWidget::new()
                    .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                    .show(&mut snarl, &mut viewer, ui);
                pin_ids = viewer.pin_interaction_ids.clone();
                drag = pin_ids
                    .iter()
                    .map(|id| {
                        ui.ctx()
                            .read_response(*id)
                            .map(|response| response.sense.senses_drag())
                    })
                    .collect();
            });
            eprintln!(
                "Mixed geometry original: long={long}; pass={pass}; state={}; sizes={sizes:?}; pin_ids={pin_ids:?}; drag={drag:?}; texts={:?}",
                state(&fx, &snarl),
                text_shapes(&painted)
            );
            assert!(!pin_ids.is_empty() && drag.iter().all(|sense| *sense == Some(true)));
            if pass >= 4 {
                if let Some(ids) = &stable_ids {
                    assert_eq!(&pin_ids, ids);
                } else {
                    stable_ids = Some(pin_ids);
                }
            }
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 3);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
            assert!(!fx.viewer().has_body(&CanvasNode::Graph(0)));
            assert_eq!(
                (0..4)
                    .map(|slot| fx.viewer().input_at(0, slot))
                    .collect::<Vec<_>>(),
                vec![Some(7), Some(8), Some(8), None]
            );
            assert_eq!(state(&fx, &snarl), before);
            recent.push(
                *sizes
                    .get(&CanvasNode::Graph(0))
                    .expect("actual mixed content node is rendered"),
            );
            output = Some(painted);
        }
        for size in &recent[4..] {
            assert!(
                size.x.is_finite()
                    && size.y.is_finite()
                    && size.x > 0.0
                    && size.y > 0.0
                    && size.x <= 220.0
            );
        }
        for pair in recent[4..].windows(2) {
            assert!((pair[0] - pair[1]).length() < 0.5);
        }
        settled.push(recent[7]);
        let glyph = char::from(lucide_icons::Icon::FileCode).to_string();
        let hover =
            visible_position(&output.unwrap(), &glyph).expect("real mixed header is visible");
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
            "Mixed hover original: wanted={:?}; actual={detected:?}; state={}",
            fx.call,
            state(&fx, &snarl)
        );
        assert_eq!(detected, Some(fx.call));
        for slot in 0..3 {
            let pin = snarl.in_pin(InPinId {
                node: fx.call,
                input: slot,
            });
            assert_eq!(pin.remotes.len(), 1);
            assert_eq!(
                snarl[pin.remotes[0].node],
                CanvasNode::Graph(if slot == 0 { 7 } else { 8 })
            );
            assert_eq!(input_wire_emphasis(detected, &pin), WireEmphasis::Incident);
        }
        let output_pin = snarl.out_pin(OutPinId {
            node: fx.call,
            output: 0,
        });
        assert_eq!(output_pin.remotes.len(), 2);
        assert!(output_pin.remotes.contains(&InPinId {
            node: fx.target,
            input: 0
        }));
        assert!(
            output_pin
                .remotes
                .iter()
                .any(|remote| snarl[remote.node] == CanvasNode::Graph(10) && remote.input == 0)
        );
        assert_eq!(
            output_wire_emphasis(detected, &output_pin),
            WireEmphasis::Incident
        );
        let unrelated_node = snarl
            .nodes_pos_ids()
            .find_map(|(id, _, node)| (*node == CanvasNode::Graph(99)).then_some(id))
            .unwrap();
        let unrelated = snarl.out_pin(OutPinId {
            node: unrelated_node,
            output: 0,
        });
        assert_eq!(
            output_wire_emphasis(detected, &unrelated),
            WireEmphasis::Unrelated
        );
        let mut viewer = fx.viewer();
        viewer.begin_node_hover_frame(None);
        let ordinary = viewer.output_wire_color(&output_pin);
        let ordinary_unrelated = viewer.output_wire_color(&unrelated);
        viewer.begin_node_hover_frame(detected);
        assert_eq!(
            viewer.output_wire_color(&output_pin),
            crate::wire_colors::with_emphasis(
                ordinary,
                viewer.colors.canvas.to_egui(),
                WireEmphasis::Incident
            )
        );
        let dimmed = viewer.output_wire_color(&unrelated);
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
    }
    // Equal three-pin vectors: no constant-height promise for arbitrary slot counts.
    assert!(
        (settled[0] - settled[1]).length() < 0.5,
        "hidden path/frame/collection identity expanded closed geometry: {settled:?}"
    );
}

#[derive(Default)]
struct TraceCollector(std::cell::RefCell<Vec<engine::TraceEvent>>);

impl engine::TraceSink for TraceCollector {
    fn record(&self, event: engine::TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn payload(
    fx: &Fixture,
    xml: &[u8],
) -> (
    anyhow::Result<cli::PayloadRunOutcome>,
    Vec<engine::TraceEvent>,
) {
    let project = mapping::Project {
        source: mixed_schema(),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("out", ScalarType::String)]),
        graph: fx.graph.clone(),
        root: fx.root_scope.clone(),
        ..crate::new_mapping::blank_project()
    };
    let trace = TraceCollector::default();
    let document = cli::PayloadDocument::new(std::path::Path::new("input.xml"), xml).unwrap();
    let result = cli::run_project_value_payloads(
        &project,
        std::path::Path::new("/tmp/ferrule-compact-mixed/project.json"),
        &cli::PayloadRunOptions::new(document)
            .with_output_path(std::path::Path::new("result.json"))
            .with_trace_sink(&trace),
    );
    let events = trace.0.into_inner();
    eprintln!("Mixed public payload original: result={result:?}; events={events:?}");
    (result, events)
}

fn deliveries(events: &[engine::TraceEvent]) -> Vec<(NodeId, usize, Vec<String>, usize, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            engine::TraceEvent::NodeInputValue {
                consumer: 0,
                input,
                input_index,
                positions,
                value,
            } => {
                let position = positions
                    .last()
                    .expect("nonempty replacement collection has an occurrence frame");
                Some((
                    *input,
                    *input_index,
                    position.collection.clone(),
                    position.index,
                    value.preview.clone(),
                ))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn compact_mixed_real_inspection_and_graph_roundtrip_preserve_public_payload_bytes_document_order_and_typed_error()
 {
    let (mut fx, mut snarl) = mixed_fixture(
        Vec::new(),
        None,
        vec![
            replacement("Italic", &["Italic"], 7),
            replacement("Bold", &["Bold"], 8),
            replacement("Plain", &[], 9),
        ],
    );
    fx.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::String("I".into()),
        },
    );
    fx.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::String("B".into()),
        },
    );
    fx.graph.nodes.insert(
        9,
        Node::RuntimeParameter {
            name: "missing".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    let before = state(&fx, &snarl);
    let xml = b"<Description>pre<Bold>first</Bold>|<Italic>second</Italic>-<Bold>third</Bold>post</Description>";
    let selected = b"<Description>pre<Bold>first</Bold>|<Italic>second</Italic>-<Bold>third</Bold><Plain>selected</Plain>post</Description>";
    let expected = vec![
        (8, 1, vec!["Bold".to_owned()], 1, "B".to_owned()),
        (7, 0, vec!["Italic".to_owned()], 1, "I".to_owned()),
        (8, 1, vec!["Bold".to_owned()], 2, "B".to_owned()),
    ];
    let (initial, initial_trace) = payload(&fx, xml);
    let initial = initial.expect("unselected throwing replacement remains lazy");
    assert_eq!(initial.artifacts.len(), 1);
    assert_eq!(
        initial.artifacts[0].bytes.as_slice(),
        b"{\n  \"out\": \"preB|I-Bpost\"\n}\n"
    );
    assert_eq!(deliveries(&initial_trace), expected);
    let (initial_error, initial_error_trace) = payload(&fx, selected);
    let initial_error = initial_error.unwrap_err();
    assert!(
        matches!(initial_error.downcast_ref::<engine::EngineError>(), Some(engine::EngineError::MissingRuntimeParameter { node: 9, name }) if name == "missing")
    );
    assert_eq!(deliveries(&initial_error_trace), expected);
    assert!(
        !initial_error_trace
            .iter()
            .any(|event| matches!(event, engine::TraceEvent::TargetFieldWritten { .. }))
    );
    let context = egui::Context::default();
    crate::icons::install(&context);
    click_pencil(&mut fx, &mut snarl, &context, true);
    let opened = settle_properties(&mut fx, &mut snarl, &context, true);
    assert!(visible_position(&opened, "Input 0: Italic").is_some());
    assert!(visible_position(&opened, "Input 1: Bold").is_some());
    properties(
        &mut fx,
        &mut snarl,
        &context,
        true,
        vec![key(egui::Key::Escape)],
    );
    let saved = serde_json::to_vec(&fx.graph).unwrap();
    fx.graph = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec(&fx.graph).unwrap(), saved);
    let (after, after_trace) = payload(&fx, xml);
    let after = after.unwrap();
    assert_eq!(
        after.artifacts[0].bytes.as_slice(),
        b"{\n  \"out\": \"preB|I-Bpost\"\n}\n"
    );
    assert_eq!(after, initial);
    assert_eq!(deliveries(&after_trace), expected);
    let (after_error, after_error_trace) = payload(&fx, selected);
    let after_error = after_error.unwrap_err();
    assert!(
        matches!(after_error.downcast_ref::<engine::EngineError>(), Some(engine::EngineError::MissingRuntimeParameter { node: 9, name }) if name == "missing")
    );
    assert_eq!(deliveries(&after_error_trace), expected);
    assert_eq!(after_error.to_string(), initial_error.to_string());
    assert!(
        !after_error_trace
            .iter()
            .any(|event| matches!(event, engine::TraceEvent::TargetFieldWritten { .. }))
    );
    assert_eq!(state(&fx, &snarl), before);
}
