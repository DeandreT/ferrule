use super::*;

fn primary_fixture(node: Node) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    let schema = crate::primary_root_authoring::test_schema();
    fx.source_paths = SourcePathCatalog::new(&schema, &[]);
    fx.source_blocks = source_blocks(&schema);
    fx.graph.nodes.insert(0, node);
    fx.graph
        .nodes
        .insert(42, Node::Const { value: Value::Null });
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let mut snarl = std::mem::take(&mut fx.snarl);
    snarl.get_node_info_mut(fx.source).unwrap().pos = egui::pos2(0.0, 260.0);
    snarl.get_node_info_mut(fx.target).unwrap().pos = egui::pos2(600.0, 0.0);
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

fn expected_fixture_serialization_refusal(graph: &Graph) -> Option<&'static str> {
    match graph.nodes.get(&0) {
        Some(Node::SourceRootField { path, .. }) if path.is_empty() => {
            Some("primary root scalar requires a bounded ordinary data path")
        }
        Some(Node::SourceRootXmlTypeEquals {
            canonical_expanded_type,
        }) if canonical_expanded_type.is_empty() => {
            Some("primary root XML type requires a bounded resolved identity")
        }
        _ => None,
    }
}

fn checked_fixture_serialization<T: std::fmt::Debug>(
    graph: &Graph,
    result: Result<T, serde_json::Error>,
) -> Result<T, String> {
    if let Some(expected) = expected_fixture_serialization_refusal(graph) {
        eprintln!(
            "Malformed in-memory Graph/IR original before rendering: {graph:#?}; serialization original: {result:?}"
        );
        let error =
            result.expect_err("malformed in-memory root metadata must refuse serialization");
        assert!(error.is_data());
        assert_eq!(error.to_string(), expected);
        Err(error.to_string())
    } else {
        Ok(
            result
                .expect("serde-valid fixture must retain complete successful graph serialization"),
        )
    }
}

fn state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> serde_json::Value {
    let graph = match checked_fixture_serialization(&fx.graph, serde_json::to_value(&fx.graph)) {
        Ok(graph) => graph,
        Err(refusal) => serde_json::json!({
            "malformed_in_memory_graph_ir_debug": format!("{:#?}", fx.graph),
            "actual_serialization_refusal": refusal,
        }),
    };
    serde_json::json!({
        "graph": graph,
        "root_scope": &fx.root_scope,
        "positions": format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
        "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    })
}

fn visible_positions(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn collect(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        label: &str,
        positions: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.visual_bounding_rect();
                if clip.intersects(rect) && clip.contains(rect.center()) {
                    positions.push(rect.center());
                }
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, label, positions);
                }
            }
            _ => {}
        }
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, label, &mut positions);
    }
    positions
}

fn pointer_events(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
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
    let root = fx.call;
    let before = state(fx, snarl);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(context.cumulative_frame_nr() as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.protected_output = protected;
            viewer.show_header(root, &[], &[], ui, snarl);
        },
    );
    eprintln!(
        "Primary compact header original: before={before}; after={}; shapes={:?}; access={:?}",
        state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update,
    );
    output
}

fn property_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
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
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(context.cumulative_frame_nr() as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            edit_id = ui.next_auto_id();
            let mut viewer = fx.viewer();
            viewer.primary_root_authoring = allowed;
            viewer.show_output(&pin, ui, snarl);
        },
    );
    eprintln!(
        "Primary compact property original: allowed={allowed}; state={}; shapes={:?}; access={:?}",
        state(fx, snarl),
        output.shapes,
        output.platform_output.accesskit_update,
    );
    (output, edit_id)
}

fn settle_properties(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
) -> egui::FullOutput {
    let mut output = property_frame(fx, snarl, context, allowed, Vec::new()).0;
    for _ in 0..3 {
        output = property_frame(fx, snarl, context, allowed, Vec::new()).0;
    }
    output
}

fn click_position(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
    pos: egui::Pos2,
) {
    for pressed in [true, false] {
        property_frame(fx, snarl, context, allowed, pointer_events(pos, pressed));
    }
}

fn click_label(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
    label: &str,
    last: bool,
) {
    let output = settle_properties(fx, snarl, context, allowed);
    let positions = visible_positions(&output, label);
    let pos = if last {
        positions.last()
    } else {
        positions.first()
    }
    .copied()
    .unwrap_or_else(|| {
        panic!(
            "missing visible primary control {label:?}; shapes={:?}",
            output.shapes
        )
    });
    click_position(fx, snarl, context, allowed, pos);
}

fn open_properties(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
) -> egui::FullOutput {
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    click_label(fx, snarl, context, allowed, &pencil, false);
    settle_properties(fx, snarl, context, allowed)
}

#[test]
fn compact_primary_headers_keep_full_identity_required_marker_hover_and_output_accessibility() {
    let parent = "complete_untruncated_primary_資料📦".repeat(5);
    let namespace = format!("urn:complete:{}", "資料📦".repeat(20));
    let cases = [
        (
            Node::SourceRootField { path: vec![parent.clone(), "資料📦".into()], required: false },
            "primary 資料📦",
            lucide_icons::Icon::ArrowRightFromLine,
            format!("\nPrimary source root\nPath: {parent}/資料📦\nRequired read: no\nMissing values remain null."),
        ),
        (
            Node::SourceRootField { path: vec![parent, "資料📦".into()], required: true },
            "required 資料📦",
            lucide_icons::Icon::ArrowRightFromLine,
            format!("\nPrimary source root\nPath: {}/資料📦\nRequired read: yes\nA missing value stops execution when this field is read.", "complete_untruncated_primary_資料📦".repeat(5)),
        ),
        (
            Node::SourceRootField {
                path: vec!["資料📦資料📦資料📦資料📦資料📦資料📦".into()],
                required: true,
            },
            "required 資料📦資料📦…",
            lucide_icons::Icon::ArrowRightFromLine,
            "\nPrimary source root\nPath: 資料📦資料📦資料📦資料📦資料📦資料📦\nRequired read: yes\nA missing value stops execution when this field is read.".into(),
        ),
        (
            Node::SourceRootField { path: Vec::new(), required: true },
            "required field?",
            lucide_icons::Icon::ArrowRightFromLine,
            "\nPrimary source root\nPath: <empty> (empty path)\nRequired read: yes\nA missing value stops execution when this field is read.".into(),
        ),
        (
            Node::SourceRootXmlTypeEquals { canonical_expanded_type: format!("{{{namespace}}}Derived") },
            "type = Derived",
            lucide_icons::Icon::Check,
            format!("\nPrimary source root\nXML type: {{{namespace}}}Derived\nCompares the actual input XML type annotation."),
        ),
        (
            Node::SourceRootXmlTypeEquals { canonical_expanded_type: String::new() },
            "type = ?",
            lucide_icons::Icon::Check,
            "\nPrimary source root\nXML type: \nCompares the actual input XML type annotation.".into(),
        ),
    ];
    for (node, summary, icon, identity) in cases {
        for protected in [None, Some(42), Some(0)] {
            let (mut fx, mut snarl) = primary_fixture(node.clone());
            let before = state(&fx, &snarl);
            let title = fx.viewer().title(&CanvasNode::Graph(0));
            let complete = format!(
                "{title}{}{identity}",
                if protected == Some(0) {
                    " (output)"
                } else {
                    ""
                }
            );
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let mut observed = Vec::new();
            let mut output = header_frame(&mut fx, &mut snarl, &context, protected, Vec::new());
            for _ in 0..8 {
                if let Some(update) = &output.platform_output.accesskit_update {
                    observed.extend(update.nodes.iter().filter_map(|(_, node)| {
                        node.value()
                            .filter(|value| value.contains(&identity))
                            .map(str::to_owned)
                    }));
                }
                output = header_frame(&mut fx, &mut snarl, &context, protected, Vec::new());
            }
            assert!(
                observed.iter().any(|value| value == &complete),
                "full root identity was lost: {observed:?}"
            );
            let marked = format!("{summary} · output");
            let visible = if protected == Some(0) {
                marked.as_str()
            } else {
                summary
            };
            assert!(
                !visible_positions(&output, visible).is_empty(),
                "missing readable summary {visible:?}"
            );
            assert!(
                visible_positions(
                    &output,
                    if protected == Some(0) {
                        summary
                    } else {
                        marked.as_str()
                    }
                )
                .is_empty()
            );
            let glyph = char::from(icon).to_string();
            let hover = visible_positions(&output, &glyph)
                .first()
                .copied()
                .expect("primary icon is visible");
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
                !visible_positions(&output, &complete).is_empty(),
                "hover lost complete root identity"
            );
            assert_eq!(state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
    }
}

#[test]
fn compact_primary_popovers_edit_real_field_required_and_namespaced_type_and_preserve_wires() {
    for initial in [
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: false,
        },
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Base".into(),
        },
    ] {
        let field = matches!(initial, Node::SourceRootField { .. });
        let (mut fx, mut snarl) = primary_fixture(initial);
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let closed = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(visible_positions(&closed, if field { "Code" } else { "Base" }).is_empty());
        let opened = open_properties(&mut fx, &mut snarl, &context, true);
        assert!(
            !visible_positions(
                &opened,
                if field {
                    "Primary source field"
                } else {
                    "Primary source XML type"
                }
            )
            .is_empty()
        );
        assert_eq!(state(&fx, &snarl), before);
        let expected_node = if field {
            click_label(&mut fx, &mut snarl, &context, true, "Code", false);
            click_label(&mut fx, &mut snarl, &context, true, "Extra", true);
            click_label(
                &mut fx,
                &mut snarl,
                &context,
                true,
                "Require a value when read",
                false,
            );
            Node::SourceRootField {
                path: vec!["Extra".into()],
                required: true,
            }
        } else {
            click_label(&mut fx, &mut snarl, &context, true, "Base", false);
            click_label(
                &mut fx,
                &mut snarl,
                &context,
                true,
                "{urn:root}Derived",
                true,
            );
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: "{urn:root}Derived".into(),
            }
        };
        let mut expected_graph = before["graph"].clone();
        expected_graph["nodes"]["0"] = serde_json::to_value(&expected_node).unwrap();
        let mut expected = before.clone();
        expected["graph"] = expected_graph;
        let after = state(&fx, &snarl);
        eprintln!("Primary real picker original: expected={expected}; actual={after}");
        assert_eq!(after, expected);
        let opened = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(
            !visible_positions(
                &opened,
                if field {
                    "Require a value when read"
                } else {
                    "{urn:root}Derived"
                }
            )
            .is_empty(),
            "nested selector closed the property editor"
        );
        assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
        assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        let saved = serde_json::to_vec(&fx.graph).unwrap();
        let reopened: Graph = serde_json::from_slice(&saved).unwrap();
        assert_eq!(serde_json::to_vec(&reopened).unwrap(), saved);
        fx.graph = reopened;
        let next_context = egui::Context::default();
        crate::icons::install(&next_context);
        let header = header_frame(&mut fx, &mut snarl, &next_context, None, Vec::new());
        assert!(
            !visible_positions(
                &header,
                if field {
                    "required Extra"
                } else {
                    "type = Derived"
                }
            )
            .is_empty()
        );
        assert_eq!(state(&fx, &snarl), expected);
    }
}

#[test]
fn compact_primary_popovers_open_by_keyboard_close_and_reopen_without_repair_or_rewiring() {
    for node in [
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: true,
        },
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Base".into(),
        },
    ] {
        let field = matches!(node, Node::SourceRootField { .. });
        let (mut fx, mut snarl) = primary_fixture(node);
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let (_, edit_id) = property_frame(&mut fx, &mut snarl, &context, true, Vec::new());
        context.memory_mut(|memory| memory.request_focus(edit_id));
        property_frame(
            &mut fx,
            &mut snarl,
            &context,
            true,
            vec![key(egui::Key::Enter)],
        );
        let opened = settle_properties(&mut fx, &mut snarl, &context, true);
        let control = if field {
            "Require a value when read"
        } else {
            "Primary source XML type"
        };
        assert!(
            !visible_positions(&opened, control).is_empty(),
            "keyboard did not open primary properties"
        );
        property_frame(
            &mut fx,
            &mut snarl,
            &context,
            true,
            vec![key(egui::Key::Escape)],
        );
        let closed = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(visible_positions(&closed, control).is_empty());
        let reopened = open_properties(&mut fx, &mut snarl, &context, true);
        assert!(!visible_positions(&reopened, control).is_empty());
        assert_eq!(state(&fx, &snarl), before);
        assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
        assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
    }
}

#[test]
fn compact_primary_locked_and_invalid_imported_properties_preserve_exact_graph_and_policy() {
    for (node, control, warning) in [
        (
            Node::SourceRootField {
                path: vec!["LegacyMissing資料".into()],
                required: false,
            },
            "Require a value when read",
            "Choose a supported primary source field.",
        ),
        (
            Node::SourceRootField {
                path: Vec::new(),
                required: true,
            },
            "Require a value when read",
            "Choose a supported primary source field.",
        ),
        (
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: "{urn:legacy}Missing資料".into(),
            },
            "{urn:legacy}Missing資料",
            "Choose a declared primary XML type.",
        ),
    ] {
        let (mut fx, mut snarl) = primary_fixture(node);
        let before = state(&fx, &snarl);
        let bytes_before = checked_fixture_serialization(&fx.graph, serde_json::to_vec(&fx.graph));
        let context = egui::Context::default();
        crate::icons::install(&context);
        let opened = open_properties(&mut fx, &mut snarl, &context, false);
        assert!(
            !visible_positions(&opened, warning).is_empty(),
            "unsupported or malformed in-memory metadata is explained"
        );
        click_label(&mut fx, &mut snarl, &context, false, control, false);
        let locked = settle_properties(&mut fx, &mut snarl, &context, false);
        assert!(
            visible_positions(&locked, "Extra").is_empty(),
            "locked selector unexpectedly opened"
        );
        assert_eq!(state(&fx, &snarl), before);
        assert_eq!(
            checked_fixture_serialization(&fx.graph, serde_json::to_vec(&fx.graph)),
            bytes_before
        );
        let passive = settle_properties(&mut fx, &mut snarl, &context, true);
        assert!(!visible_positions(&passive, warning).is_empty());
        assert_eq!(
            state(&fx, &snarl),
            before,
            "enabling inspection silently repaired an unsupported or malformed node"
        );
    }
}

#[test]
fn compact_primary_nodes_settle_with_long_unicode_identities_and_keep_pin_drag_and_incident_wires()
{
    for type_node in [false, true] {
        let mut settled_widths = Vec::new();
        for long in [false, true] {
            let node = if type_node {
                Node::SourceRootXmlTypeEquals {
                    canonical_expanded_type: if long {
                        format!("{{urn:{}}}Derived", "complete_namespace_資料📦".repeat(8))
                    } else {
                        "Derived".into()
                    },
                }
            } else {
                Node::SourceRootField {
                    path: if long {
                        vec!["complete_parent_資料📦".repeat(8), "資料📦".into()]
                    } else {
                        vec!["資料📦".into()]
                    },
                    required: true,
                }
            };
            let (mut fx, mut snarl) = primary_fixture(node);
            fx.graph.nodes.insert(
                1,
                Node::Call {
                    function: "upper".into(),
                    args: vec![2],
                },
            );
            fx.graph.nodes.insert(
                2,
                Node::Const {
                    value: Value::String("unrelated".into()),
                },
            );
            let unrelated_from = snarl.insert_node(egui::pos2(100.0, 500.0), CanvasNode::Graph(2));
            let unrelated_to = snarl.insert_node(egui::pos2(400.0, 500.0), CanvasNode::Graph(1));
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
            let before = state(&fx, &snarl);
            let context = egui::Context::default();
            crate::icons::install(&context);
            let mut recent = Vec::new();
            let mut output = None;
            for _ in 0..8 {
                let mut sizes = std::collections::BTreeMap::new();
                let mut senses = Vec::new();
                let painted = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 900.0),
                        )),
                        time: Some(context.cumulative_frame_nr() as f64 * 0.1),
                        ..Default::default()
                    },
                    |ui| {
                        let mut viewer = fx.viewer();
                        viewer.primary_root_authoring = true;
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
                    },
                );
                eprintln!(
                    "Primary settled original: type={type_node}; long={long}; before={before}; after={}; sizes={sizes:?}; drag={senses:?}; shapes={:?}",
                    state(&fx, &snarl),
                    painted.shapes
                );
                assert!(!senses.is_empty() && senses.iter().all(|sense| *sense == Some(true)));
                assert_eq!(state(&fx, &snarl), before);
                assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
                assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
                assert!(!fx.viewer().has_body(&CanvasNode::Graph(0)));
                recent.push(
                    *sizes
                        .get(&CanvasNode::Graph(0))
                        .expect("primary node rendered"),
                );
                output = Some(painted);
            }
            for size in &recent[4..] {
                assert!(size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0);
                assert!(
                    size.x <= 220.0 && size.y <= 100.0,
                    "closed primary node uses too much canvas space: {size:?}"
                );
            }
            for pair in recent[4..].windows(2) {
                assert!(
                    (pair[0] - pair[1]).length() < 0.5,
                    "primary node grows across repaints"
                );
            }
            settled_widths.push(recent[7].x);
            let output = output.expect("settled frame");
            let glyph = char::from(if type_node {
                lucide_icons::Icon::Check
            } else {
                lucide_icons::Icon::ArrowRightFromLine
            })
            .to_string();
            let hover = visible_positions(&output, &glyph)
                .first()
                .copied()
                .expect("root header icon visible");
            let mut detected = None;
            let root = fx.call;
            for _ in 0..2 {
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 900.0),
                        )),
                        time: Some(context.cumulative_frame_nr() as f64 * 0.1),
                        events: vec![egui::Event::PointerMoved(hover)],
                        ..Default::default()
                    },
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
                "Primary hover original: wanted={root:?}; actual={detected:?}; state={}",
                state(&fx, &snarl)
            );
            assert_eq!(detected, Some(root));
            let incident = snarl.out_pin(OutPinId {
                node: root,
                output: 0,
            });
            let unrelated = snarl.out_pin(OutPinId {
                node: unrelated_from,
                output: 0,
            });
            let mut viewer = fx.viewer();
            for preset in [
                crate::appearance::AppearancePreset::Dark,
                crate::appearance::AppearancePreset::HighContrast,
            ] {
                viewer.colors = crate::appearance::SemanticThemeColors::preset(preset);
                for mode in [WireColorMode::Theme, WireColorMode::UniquePerWire] {
                    viewer.wire_color_mode = mode;
                    viewer.begin_node_hover_frame(None);
                    let incident_normal = viewer.output_wire_color(&incident);
                    let unrelated_normal = viewer.output_wire_color(&unrelated);
                    viewer.begin_node_hover_frame(detected);
                    assert_eq!(
                        viewer.output_wire_color(&incident),
                        crate::wire_colors::with_emphasis(
                            incident_normal,
                            viewer.colors.canvas.to_egui(),
                            WireEmphasis::Incident,
                        ),
                    );
                    let dimmed = viewer.output_wire_color(&unrelated);
                    assert_eq!(
                        dimmed,
                        crate::wire_colors::with_emphasis(
                            unrelated_normal,
                            viewer.colors.canvas.to_egui(),
                            WireEmphasis::Unrelated,
                        ),
                    );
                    assert!(dimmed.a() < unrelated_normal.a());
                    if mode == WireColorMode::UniquePerWire
                        || preset == crate::appearance::AppearancePreset::HighContrast
                    {
                        assert_eq!(viewer.output_wire_color(&incident), incident_normal);
                    }
                }
            }
            assert_eq!(
                output_wire_emphasis(detected, &incident),
                WireEmphasis::Incident
            );
            assert_eq!(
                output_wire_emphasis(detected, &unrelated),
                WireEmphasis::Unrelated
            );
            assert_eq!(state(&fx, &snarl), before);
        }
        assert!(
            (settled_widths[0] - settled_widths[1]).abs() < 0.5,
            "hidden root identity length affects compact width: {settled_widths:?}"
        );
    }
}

#[derive(Clone, Copy)]
struct HoverInput {
    zoom: f32,
    pointer: Option<egui::Pos2>,
    clipped: bool,
    popup: bool,
}

struct HoverObservation {
    detected: Option<SnarlNodeId>,
    frame: Option<egui::Response>,
    popup: Option<egui::Rect>,
}

fn transformed_hover_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    mode: HoverInput,
) -> HoverObservation {
    let root = fx.call;
    let graph_point = snarl.get_node_info(root).unwrap().pos;
    let canvas_id = egui::Id::new("compact-primary-existing-frame-hover");
    let frame_id = canvas_id.with(("snarl-node", root)).with("frame");
    let mut observation = HoverObservation {
        detected: None,
        frame: None,
        popup: None,
    };
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(context.cumulative_frame_nr() as f64 * 0.1),
            events: mode
                .pointer
                .map(egui::Event::PointerMoved)
                .into_iter()
                .collect(),
            ..Default::default()
        },
        |ui| {
            if mode.clipped {
                ui.set_clip_rect(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(220.0, 900.0),
                ));
            }
            let mut viewer = fx.viewer();
            viewer.primary_root_authoring = true;
            viewer.camera_focus = Some((graph_point, egui::pos2(450.0, 300.0), Some(mode.zoom)));
            viewer.begin_node_hover_frame(None);
            SnarlWidget::new()
                .id(canvas_id)
                .style(crate::appearance::EditorAppearance::default().to_snarl_style())
                .show(snarl, &mut viewer, ui);
            observation.detected = viewer.end_node_hover_frame();
            observation.frame = ui.ctx().read_response(frame_id);
            if mode.popup {
                let anchor = mode
                    .pointer
                    .expect("foreground popup control uses a real captured node point")
                    - egui::vec2(12.0, 12.0);
                observation.popup = egui::Popup::new(
                    canvas_id.with("foreground-occlusion"),
                    ui.ctx().clone(),
                    anchor,
                    ui.layer_id(),
                )
                .open(true)
                .width(160.0)
                .show(|ui| {
                    ui.set_min_size(egui::vec2(160.0, 80.0));
                    ui.label("Foreground inspection");
                })
                .map(|popup| popup.response.rect);
            }
        },
    );
    let frame = observation.frame.as_ref().map(|response| {
        (
            response.id,
            response.layer_id,
            response.rect,
            response.interact_rect,
            response.contains_pointer(),
            context.layer_transform_to_global(response.layer_id),
        )
    });
    eprintln!(
        "Primary transformed hover original before assertions: zoom={}; pointer={:?}; clipped={}; popup={:?}; wanted={root:?}; detected={:?}; existing_frame={frame:?}; raw_pointer={:?}; pointer_area_layer={:?}; state={}; shape_count={}",
        mode.zoom,
        mode.pointer,
        mode.clipped,
        observation.popup,
        observation.detected,
        context.input(|input| input.pointer.interact_pos()),
        mode.pointer.and_then(|point| context.layer_id_at(point)),
        state(fx, snarl),
        output.shapes.len(),
    );
    observation
}

#[test]
fn primary_node_hover_uses_transformed_clipped_widget_hits_and_yields_to_real_foreground_popups() {
    for zoom in [0.75, 1.5] {
        let (mut fx, mut snarl) = primary_fixture(Node::SourceRootField {
            path: vec!["Code".into()],
            required: true,
        });
        let before = state(&fx, &snarl);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let mut mode = HoverInput {
            zoom,
            pointer: None,
            clipped: false,
            popup: false,
        };
        let mut observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        for _ in 0..7 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        let frame = observation
            .frame
            .as_ref()
            .expect("actual egui-snarl node frame is registered");
        let transform = context
            .layer_transform_to_global(frame.layer_id)
            .expect("actual canvas layer has a transform");
        assert!((transform.scaling - zoom).abs() < 0.001);
        assert!(transform.translation.length() > 0.0);
        let point = (transform * frame.rect).center();
        assert!(point.x > 220.0 && point.x < 1100.0 && point.y < 900.0);
        mode.pointer = Some(point);
        for _ in 0..3 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        assert_eq!(observation.detected, Some(fx.call));
        assert!(observation.frame.as_ref().unwrap().contains_pointer());
        assert_eq!(state(&fx, &snarl), before);
        mode.popup = true;
        for _ in 0..4 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        assert!(
            observation
                .popup
                .expect("actual foreground popup was rendered")
                .contains(point)
        );
        assert_eq!(
            observation.detected, None,
            "node behind a real popup stole hover"
        );
        assert!(!observation.frame.as_ref().unwrap().contains_pointer());
        assert_eq!(state(&fx, &snarl), before);
        mode.popup = false;
        for _ in 0..4 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        assert_eq!(
            observation.detected,
            Some(fx.call),
            "hover did not return after popup closed"
        );
        mode.clipped = true;
        for _ in 0..3 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        assert_eq!(
            observation.detected, None,
            "node outside the canvas clip stole hover"
        );
        assert!(
            !observation
                .frame
                .as_ref()
                .is_some_and(|frame| frame.contains_pointer())
        );
        mode.clipped = false;
        mode.pointer = Some(egui::pos2(1100.0, 850.0));
        for _ in 0..3 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        assert_eq!(
            observation.detected, None,
            "pointer away from every node retained hover"
        );
        mode.pointer = Some(point);
        for _ in 0..3 {
            observation = transformed_hover_frame(&mut fx, &mut snarl, &context, mode);
        }
        assert_eq!(observation.detected, Some(fx.call));
        assert_eq!(state(&fx, &snarl), before);
    }
}
