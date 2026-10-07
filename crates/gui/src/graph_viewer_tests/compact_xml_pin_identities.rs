use super::*;

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn walk(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        found: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => found.push((
                text.galley.text().to_owned(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
                clip,
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, clip, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        walk(&shape.shape, shape.clip_rect, &mut found);
    }
    found
}

fn visible(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
    texts(output).into_iter().find_map(|(text, rect, clip)| {
        (text == wanted && clip.contains(rect.center())).then_some(rect.center())
    })
}

fn state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> serde_json::Value {
    serde_json::json!({
        "graph": &fx.graph,
        "root": &fx.root_scope,
        "nodes": snarl.nodes_pos_ids().map(|(id, pos, node)| {
            format!("{id:?}:{node:?}:{pos:?}")
        }).collect::<Vec<_>>(),
        "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
    })
}

struct Frame {
    output: egui::FullOutput,
    size: egui::Vec2,
    pins: Vec<(egui::Id, egui::Pos2, bool)>,
}

fn draw(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    search: &mut crate::canvas_search::CanvasSearchState,
    protected: Option<NodeId>,
    events: Vec<egui::Event>,
) -> Frame {
    let mut sizes = std::collections::BTreeMap::new();
    let mut pins = Vec::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 950.0),
            )),
            time: Some(context.input(|input| input.time) + 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut viewer = fx.viewer();
                viewer.protected_output = protected;
                viewer.node_sizes = Some(&mut sizes);
                crate::canvas_keyboard::show(
                    snarl,
                    &mut viewer,
                    search,
                    crate::canvas_keyboard::CanvasOptions {
                        id_salt: egui::Id::new("compact-xml-real-pin-identities"),
                        show_minimap: false,
                        view_generation: 0,
                        style: crate::appearance::EditorAppearance::default().to_snarl_style(),
                        focus: None,
                    },
                    ui,
                );
                pins = crate::canvas_keyboard::current_pin_interaction_ids(
                    context,
                    egui::Id::new("compact-xml-real-pin-identities"),
                    0,
                )
                .into_iter()
                .map(|id| {
                    let response = context
                        .read_response(id)
                        .expect("actual native pin response");
                    let point = context
                        .layer_transform_to_global(response.layer_id)
                        .unwrap_or_default()
                        * response.rect.center();
                    (id, point, response.sense.senses_drag())
                })
                .collect();
            });
        },
    );
    Frame {
        output,
        size: sizes[&CanvasNode::Graph(0)],
        pins,
    }
}

fn sole_node(node: Node) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    fx.graph.nodes.clear();
    fx.graph.nodes.insert(0, node);
    let mut snarl = Snarl::new();
    fx.call = snarl.insert_node(egui::pos2(460.0, 100.0), CanvasNode::Graph(0));
    (fx, snarl)
}

#[test]
fn compact_mixed_real_native_pin_hover_identifies_ordered_replacements_without_painted_labels_or_size_changes()
 {
    let (mut fx, mut snarl) = sole_node(Node::XmlMixedContent {
        path: vec!["Description".into()],
        frame: Some(vec!["Orders".into(), "Item".into()]),
        replacements: vec![
            mapping::XmlMixedContentReplacement {
                element: "Bold".into(),
                collection: Vec::new(),
                expression: 7,
            },
            mapping::XmlMixedContentReplacement {
                element: "Italic_資料📦".into(),
                collection: vec!["Orders".into(), "Italic".into()],
                expression: 8,
            },
            mapping::XmlMixedContentReplacement {
                element: "Bold".into(),
                collection: vec!["Bold".into()],
                expression: 7,
            },
        ],
    });
    for (id, value, y) in [(7, "bold", 240.0), (8, "italic", 390.0)] {
        fx.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::String(value.into()),
            },
        );
        let from = snarl.insert_node(egui::pos2(80.0, y), CanvasNode::Graph(id));
        for slot in if id == 7 { vec![0, 2] } else { vec![1] } {
            snarl.connect(
                OutPinId {
                    node: from,
                    output: 0,
                },
                InPinId {
                    node: fx.call,
                    input: slot,
                },
            );
        }
    }
    let before = state(&fx, &snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut search = crate::canvas_search::CanvasSearchState::default();
    let mut frame = draw(&mut fx, &mut snarl, &context, &mut search, None, Vec::new());
    for _ in 0..8 {
        frame = draw(&mut fx, &mut snarl, &context, &mut search, None, Vec::new());
    }
    eprintln!(
        "Mixed native pin closed original: state={before}; size={:?}; pins={:?}; texts={:?}",
        frame.size,
        frame.pins,
        texts(&frame.output)
    );
    assert_eq!(frame.pins.len(), 6);
    assert!(frame.pins.iter().all(|(_, _, drag)| *drag));
    let original_ids = frame.pins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
    let closed_size = frame.size;
    assert!(closed_size.x <= 220.0 && closed_size.y <= 180.0);
    assert!(visible(&frame.output, "Bold").is_none());
    assert!(visible(&frame.output, "Italic_資料📦").is_none());
    // These are the only three input pins: the two source literals have only
    // outputs. Read their actual transformed native hit rectangles, not guessed
    // coordinates or synthetic hover flags.
    let mut columns = Vec::<Vec<(egui::Id, egui::Pos2, bool)>>::new();
    for pin in &frame.pins {
        if let Some(column) = columns
            .iter_mut()
            .find(|column| (column[0].1.x - pin.1.x).abs() < 0.5)
        {
            column.push(*pin);
        } else {
            columns.push(vec![*pin]);
        }
    }
    let mut inputs = columns
        .into_iter()
        .find(|column| column.len() == 3)
        .expect("three real input pins share their native column");
    inputs.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    for ((id, point, _), expected) in inputs.into_iter().zip([
        "Input 0: Bold\nCollection: parent context (empty path)\nExpression: #7",
        "Input 1: Italic_資料📦\nCollection: Orders/Italic\nExpression: #8",
        "Input 2: Bold\nCollection: Bold\nExpression: #7",
    ]) {
        frame = draw(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            None,
            vec![egui::Event::PointerMoved(point)],
        );
        for _ in 0..10 {
            frame = draw(&mut fx, &mut snarl, &context, &mut search, None, Vec::new());
        }
        eprintln!(
            "Mixed native pin hovered original: wanted={expected:?}; pin={id:?}; point={point:?}; state={}; size={:?}; pins={:?}; texts={:?}",
            state(&fx, &snarl),
            frame.size,
            frame.pins,
            texts(&frame.output)
        );
        assert!(context.read_response(id).unwrap().contains_pointer());
        assert!(
            visible(&frame.output, expected).is_some(),
            "actual pin tooltip lost {expected:?}"
        );
        assert!((frame.size - closed_size).length() < 0.5);
        assert_eq!(
            frame.pins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            original_ids
        );
        assert!(frame.pins.iter().all(|(_, _, drag)| *drag));
        assert_eq!(state(&fx, &snarl), before);
    }
    let bytes = serde_json::to_vec(&fx.graph).unwrap();
    let reopened: Graph = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
}

#[test]
fn compact_xml_serializer_real_stationary_hover_and_accessibility_keep_frame_and_element_identity()
{
    let mut sizes_by_output = std::collections::BTreeMap::new();
    for (frame_selection, frame_label) in [
        (None, "automatic"),
        (Some(Vec::new()), "explicit (empty path)"),
        (
            Some(vec!["Orders_資料📦".into(), "Item".into()]),
            "explicit Orders_資料📦/Item",
        ),
    ] {
        for protected in [None, Some(0)] {
            let (mut fx, mut snarl) = sole_node(Node::XmlSerialize {
                path: vec!["Document".into()],
                frame: frame_selection.clone(),
                schema: Box::new(
                    SchemaNode::group(
                        "Record_注文",
                        vec![SchemaNode::scalar("Text", ScalarType::String)],
                    )
                    .xml_qualified("urn:record:資料")
                    .unwrap(),
                ),
                declaration: true,
                indent: false,
                namespace: Some("urn:writer".into()),
            });
            let before = state(&fx, &snarl);
            let bytes = serde_json::to_vec(&fx.graph).unwrap();
            let reopened: Graph = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
            let context = egui::Context::default();
            crate::icons::install(&context);
            context.enable_accesskit();
            let mut search = crate::canvas_search::CanvasSearchState::default();
            let expected = format!(
                "XML serialize: Document{}\nPath: Document\nFrame: {frame_label}\nElement: Record_注文\nElement namespace: qualified \"urn:record:資料\"\nXML declaration: true\nIndent output: false\nDefault namespace: set \"urn:writer\"",
                if protected == Some(0) {
                    " (output)"
                } else {
                    ""
                }
            );
            let mut accessible = Vec::new();
            let mut frame = draw(
                &mut fx,
                &mut snarl,
                &context,
                &mut search,
                protected,
                Vec::new(),
            );
            for _ in 0..8 {
                if let Some(update) = &frame.output.platform_output.accesskit_update {
                    accessible.extend(
                        update
                            .nodes
                            .iter()
                            .filter_map(|(_, node)| node.value().map(str::to_owned)),
                    );
                }
                frame = draw(
                    &mut fx,
                    &mut snarl,
                    &context,
                    &mut search,
                    protected,
                    Vec::new(),
                );
            }
            eprintln!(
                "XML serializer closed original: state={before}; size={:?}; pins={:?}; accessible={accessible:?}; texts={:?}",
                frame.size,
                frame.pins,
                texts(&frame.output)
            );
            assert!(
                accessible.contains(&expected),
                "complete accessible serializer identity lost"
            );
            assert!(
                visible(
                    &frame.output,
                    if protected.is_some() {
                        "serialize · output"
                    } else {
                        "serialize"
                    }
                )
                .is_some()
            );
            assert!(visible(&frame.output, "Record_注文").is_none());
            assert_eq!(frame.pins.len(), 1);
            assert!(frame.pins[0].2);
            let closed_size = frame.size;
            assert!(closed_size.x <= 220.0 && closed_size.y <= 120.0);
            if let Some(size) = sizes_by_output.get(&protected) {
                assert!(
                    (closed_size - *size).length() < 0.5,
                    "frame identity changed closed dimensions"
                );
            } else {
                sizes_by_output.insert(protected, closed_size);
            }
            let glyph = char::from(lucide_icons::Icon::FileCode).to_string();
            let point =
                visible(&frame.output, &glyph).expect("serializer's actual icon is visible");
            frame = draw(
                &mut fx,
                &mut snarl,
                &context,
                &mut search,
                protected,
                vec![egui::Event::PointerMoved(point)],
            );
            for _ in 0..10 {
                frame = draw(
                    &mut fx,
                    &mut snarl,
                    &context,
                    &mut search,
                    protected,
                    Vec::new(),
                );
            }
            eprintln!(
                "XML serializer hovered original: wanted={expected:?}; state={}; size={:?}; pins={:?}; texts={:?}",
                state(&fx, &snarl),
                frame.size,
                frame.pins,
                texts(&frame.output)
            );
            assert!(
                visible(&frame.output, &expected).is_some(),
                "real serializer tooltip lost complete identity"
            );
            assert!((frame.size - closed_size).length() < 0.5);
            assert_eq!(state(&fx, &snarl), before);
            assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
            assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
        }
    }
}
#[test]
fn compact_xml_serializer_real_hover_and_accessibility_distinguish_namespace_sentinels_and_escaped_values()
 {
    // These are stored, serializable IR identities. In particular, the escaped
    // string controls exercise presentation without claiming XML-writer URI
    // validity or changing the serializer's runtime boundary.
    let cases = [
        (None, None, "unspecified", "not set"),
        (
            ir::XmlNamespace::qualified("unspecified"),
            None,
            "qualified \"unspecified\"",
            "not set",
        ),
        (
            Some(ir::XmlNamespace::Unqualified),
            None,
            "explicitly unqualified",
            "not set",
        ),
        (
            ir::XmlNamespace::qualified("explicitly unqualified"),
            None,
            "qualified \"explicitly unqualified\"",
            "not set",
        ),
        (
            None,
            Some("not set".to_owned()),
            "unspecified",
            "set \"not set\"",
        ),
        (
            ir::XmlNamespace::qualified("urn:quote\"\\line\n資料"),
            Some("urn:quote\"\\line\n資料".to_owned()),
            "qualified \"urn:quote\\\"\\\\line\\n資料\"",
            "set \"urn:quote\\\"\\\\line\\n資料\"",
        ),
    ];
    let mut closed_size = None;
    let mut actual_accessible_titles = std::collections::BTreeSet::new();
    let mut actual_hovered_titles = std::collections::BTreeSet::new();
    for (element_namespace, namespace, element_label, default_label) in cases {
        let mut schema = SchemaNode::group(
            "Record",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        );
        schema.xml_namespace = element_namespace;
        let (mut fx, mut snarl) = sole_node(Node::XmlSerialize {
            path: vec!["Document".into()],
            frame: None,
            schema: Box::new(schema),
            declaration: true,
            indent: false,
            namespace,
        });
        let before = state(&fx, &snarl);
        let bytes =
            serde_json::to_vec(&fx.graph).expect("complete stored namespace graph serializes");
        let reopened: Graph =
            serde_json::from_slice(&bytes).expect("complete namespace graph reopens");
        assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
        let expected = format!(
            "XML serialize: Document\nPath: Document\nFrame: automatic\nElement: Record\nElement namespace: {element_label}\nXML declaration: true\nIndent output: false\nDefault namespace: {default_label}"
        );
        let context = egui::Context::default();
        crate::icons::install(&context);
        context.enable_accesskit();
        let mut search = crate::canvas_search::CanvasSearchState::default();
        let mut accessible = Vec::new();
        let mut frame = draw(&mut fx, &mut snarl, &context, &mut search, None, Vec::new());
        for _ in 0..8 {
            if let Some(update) = &frame.output.platform_output.accesskit_update {
                accessible.extend(
                    update
                        .nodes
                        .iter()
                        .filter_map(|(_, node)| node.value().map(str::to_owned)),
                );
            }
            frame = draw(&mut fx, &mut snarl, &context, &mut search, None, Vec::new());
        }
        eprintln!(
            "Namespace sentinel closed original: wanted={expected:?}; state={before}; size={:?}; pins={:?}; accessible={accessible:?}; texts={:?}",
            frame.size,
            frame.pins,
            texts(&frame.output)
        );
        assert!(
            accessible.contains(&expected),
            "real accessible namespace identity missing: {expected:?}"
        );
        actual_accessible_titles.insert(
            accessible
                .into_iter()
                .find(|title| title == &expected)
                .unwrap(),
        );
        assert!(visible(&frame.output, "serialize").is_some());
        assert!(visible(&frame.output, "Record").is_none());
        assert_eq!(frame.pins.len(), 1);
        assert!(frame.pins[0].2);
        let ids = frame.pins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
        let size = frame.size;
        assert!(size.x <= 220.0 && size.y <= 120.0);
        if let Some(original_size) = closed_size {
            assert!(
                (size - original_size).length() < 0.5,
                "namespace identity widened the closed node"
            );
        } else {
            closed_size = Some(size);
        }
        let glyph = char::from(lucide_icons::Icon::FileCode).to_string();
        let point = visible(&frame.output, &glyph).expect("actual compact serializer icon");
        frame = draw(
            &mut fx,
            &mut snarl,
            &context,
            &mut search,
            None,
            vec![egui::Event::PointerMoved(point)],
        );
        for _ in 0..10 {
            frame = draw(&mut fx, &mut snarl, &context, &mut search, None, Vec::new());
        }
        eprintln!(
            "Namespace sentinel hovered original: wanted={expected:?}; state={}; size={:?}; pins={:?}; texts={:?}",
            state(&fx, &snarl),
            frame.size,
            frame.pins,
            texts(&frame.output)
        );
        let hovered = texts(&frame.output)
            .into_iter()
            .find_map(|(text, rect, clip)| {
                (text == expected && clip.contains(rect.center())).then_some(text)
            })
            .expect("real painted stationary namespace tooltip retains its full literal identity");
        actual_hovered_titles.insert(hovered);
        assert!((frame.size - size).length() < 0.5);
        assert_eq!(
            frame.pins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            ids
        );
        assert!(frame.pins[0].2);
        assert_eq!(state(&fx, &snarl), before);
        assert_eq!(serde_json::to_vec(&fx.graph).unwrap(), bytes);
        assert_eq!(fx.viewer().inputs(&CanvasNode::Graph(0)), 0);
        assert_eq!(fx.viewer().outputs(&CanvasNode::Graph(0)), 1);
    }
    assert_eq!(
        actual_accessible_titles.len(),
        6,
        "distinct stored namespace states must have distinct full accessible titles"
    );
    assert_eq!(
        actual_hovered_titles, actual_accessible_titles,
        "real hover and accessible identities must agree"
    );
}
