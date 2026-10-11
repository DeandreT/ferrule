use super::*;
use crate::canvas::{source_blocks, target_blocks};
use ir::{ScalarType, SchemaNode};

#[test]
fn wheel_scrolls_endpoint_under_pointer_and_routes_hidden_wires_to_edge_proxies() {
    let source_schema = SchemaNode::group(
        "source",
        (0..20)
            .map(|index| SchemaNode::scalar(format!("s{index}"), ScalarType::String))
            .collect(),
    );
    let target_schema = SchemaNode::group(
        "target",
        (0..20)
            .map(|index| SchemaNode::scalar(format!("t{index}"), ScalarType::String))
            .collect(),
    );
    let source_blocks = source_blocks(&source_schema);
    let target_blocks = target_blocks(&target_schema);
    let source_paths = SourcePathCatalog::new(&source_schema, &[]);
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["s0".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["s15".into()],
            frame: None,
        },
    );
    let mut root_scope = Scope {
        bindings: vec![
            Binding {
                target_field: "t0".into(),
                node: 0,
            },
            Binding {
                target_field: "t15".into(),
                node: 1,
            },
        ],
        ..Scope::default()
    };
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::pos2(20.0, 40.0), CanvasNode::SourceBlock(0));
    let target = snarl.insert_node(egui::pos2(420.0, 40.0), CanvasNode::TargetBlock(0));
    let mut endpoint_scroll = crate::canvas_endpoints::EndpointScrollState::default();
    crate::app::sync_endpoint_wires(
        &graph,
        &root_scope,
        &source_blocks,
        &target_blocks,
        &endpoint_scroll,
        &mut snarl,
    );

    let wire_pins = |snarl: &Snarl<CanvasNode>| {
        let mut pins = snarl
            .wires()
            .map(|(output, input)| (output.output, input.input))
            .collect::<Vec<_>>();
        pins.sort_unstable();
        pins
    };
    assert_eq!(wire_pins(&snarl), [(0, 0), (12, 12)]);

    let mut node_sizes = std::collections::BTreeMap::from([
        (CanvasNode::SourceBlock(0), egui::vec2(230.0, 300.0)),
        (CanvasNode::TargetBlock(0), egui::vec2(230.0, 300.0)),
    ]);
    let source_pointer = snarl
        .get_node_info(source)
        .map(|info| info.pos + egui::vec2(100.0, 100.0))
        .expect("source endpoint exists");
    {
        let mut viewer = GraphViewer {
            graph: &mut graph,
            root_scope: &mut root_scope,
            primary_root_authoring: false,
            extra_targets: &[],
            inactive_target_scopes: &[],
            project_references: Default::default(),
            source_blocks: &source_blocks,
            target_blocks: &target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &source_paths,
            function_names: Default::default(),
            function_inputs: Default::default(),
            parameter_names: Default::default(),
            protected_output: None,
            function_output: None,
            requested_function_open: None,
            colors: crate::appearance::SemanticThemeColors::default(),
            wire_color_mode: crate::appearance::WireColorMode::Theme,
            endpoint_scroll: &mut endpoint_scroll,
            value_map_wheel: None,
            endpoint_search_match: None,
            node_sizes: Some(&mut node_sizes),
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        assert!(viewer.scroll_endpoint_at(source_pointer, -90.0, &mut snarl));
    }

    assert_eq!(endpoint_scroll.offset(CanvasNode::SourceBlock(0), 20), 5);
    assert_eq!(wire_pins(&snarl), [(0, 0), (11, 12)]);

    let target_pointer = snarl
        .get_node_info(target)
        .map(|info| info.pos + egui::vec2(100.0, 100.0))
        .expect("target endpoint exists");
    {
        let mut viewer = GraphViewer {
            graph: &mut graph,
            root_scope: &mut root_scope,
            primary_root_authoring: false,
            extra_targets: &[],
            inactive_target_scopes: &[],
            project_references: Default::default(),
            source_blocks: &source_blocks,
            target_blocks: &target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &source_paths,
            function_names: Default::default(),
            function_inputs: Default::default(),
            parameter_names: Default::default(),
            protected_output: None,
            function_output: None,
            requested_function_open: None,
            colors: crate::appearance::SemanticThemeColors::default(),
            wire_color_mode: crate::appearance::WireColorMode::Theme,
            endpoint_scroll: &mut endpoint_scroll,
            value_map_wheel: None,
            endpoint_search_match: None,
            node_sizes: Some(&mut node_sizes),
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        assert!(viewer.scroll_endpoint_at(target_pointer, -90.0, &mut snarl));
    }

    assert_eq!(endpoint_scroll.offset(CanvasNode::TargetBlock(0), 20), 5);
    assert_eq!(wire_pins(&snarl), [(0, 0), (11, 11)]);
}

fn endpoint_text<'a>(
    output: &'a egui::FullOutput,
    label: &str,
) -> Option<&'a egui::epaint::TextShape> {
    fn find<'a>(
        shape: &'a egui::epaint::Shape,
        label: &str,
    ) -> Option<&'a egui::epaint::TextShape> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text),
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
}

#[test]
fn endpoint_titles_use_role_icons_and_ellipsize_within_allocated_width() {
    let long = "注文資料📦_complete_endpoint_name_".repeat(20);
    let cases = [
        (
            true,
            "Source: Orders".to_owned(),
            "Orders".to_owned(),
            false,
        ),
        (
            false,
            "Target: Invoice".to_owned(),
            "Invoice".to_owned(),
            false,
        ),
        (true, format!("Source: {long}"), long.clone(), true),
        (false, format!("Target: {long}"), long, true),
        (
            true,
            "Target: literal".to_owned(),
            "Target: literal".to_owned(),
            false,
        ),
    ];
    for (source, full_title, visible_title, long) in cases {
        for width in [198.0, 448.0] {
            let context = egui::Context::default();
            crate::icons::install(&context);
            let mut header_rect = egui::Rect::NOTHING;
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(width);
                        header_rect =
                            graph_node_presentation::show_endpoint_header(ui, source, &full_title)
                                .rect;
                    },
                );
            }
            eprintln!(
                "endpoint header source={source}, width={width}, full={full_title:?}, visible={visible_title:?}, rect={header_rect:?}, shapes={:?}",
                output.shapes,
            );
            let text = endpoint_text(&output, &visible_title).expect("endpoint title is painted");
            assert_eq!(text.galley.rows.len(), 1);
            assert_eq!(text.galley.elided, long);
            assert!(text.visual_bounding_rect().right() <= header_rect.left() + width + 1.0);
            assert!(header_rect.width() <= width + 1.0);
            let icon = char::from(if source {
                lucide_icons::Icon::ArrowRightFromLine
            } else {
                lucide_icons::Icon::ArrowRightToLine
            })
            .to_string();
            assert!(endpoint_text(&output, &icon).is_some());
        }
    }
}

struct EndpointHeaderFixture {
    graph: Graph,
    root: Scope,
    source_blocks: Vec<SourceBlock>,
    target_blocks: Vec<TargetBlock>,
    source_paths: SourcePathCatalog,
    scroll: crate::canvas_endpoints::EndpointScrollState,
    sizes: std::collections::BTreeMap<CanvasNode, egui::Vec2>,
}

impl EndpointHeaderFixture {
    fn new() -> (Self, Snarl<CanvasNode>, [SnarlNodeId; 2]) {
        let source_schema = SchemaNode::group(
            "source",
            (0..20)
                .map(|index| SchemaNode::scalar(format!("s{index}"), ScalarType::String))
                .collect(),
        );
        let target_schema = SchemaNode::group(
            "target",
            (0..20)
                .map(|index| SchemaNode::scalar(format!("t{index}"), ScalarType::String))
                .collect(),
        );
        let mut source_blocks = source_blocks(&source_schema);
        let mut target_blocks = target_blocks(&target_schema);
        source_blocks[0].title = format!("Source: {}", "complete_注文📦_source_".repeat(20));
        source_blocks[0].frame = Some(vec!["BEG".into()]);
        target_blocks[0].title = format!("Target: {}", "complete_資料📦_target_".repeat(20));
        target_blocks[0].chain = vec!["NM1".into()];
        let source_paths = SourcePathCatalog::new(&source_schema, &[]);
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["s15".into()],
                frame: None,
            },
        );
        let root = Scope {
            bindings: vec![Binding {
                target_field: "t15".into(),
                node: 0,
            }],
            ..Scope::default()
        };
        let mut scroll = crate::canvas_endpoints::EndpointScrollState::default();
        scroll.scroll_rows(CanvasNode::SourceBlock(0), 20, 5);
        scroll.scroll_rows(CanvasNode::TargetBlock(0), 20, 5);
        let mut snarl = Snarl::new();
        let source = snarl.insert_node(egui::pos2(20.0, 40.0), CanvasNode::SourceBlock(0));
        let target = snarl.insert_node(egui::pos2(420.0, 40.0), CanvasNode::TargetBlock(0));
        crate::app::sync_endpoint_wires(
            &graph,
            &root,
            &source_blocks,
            &target_blocks,
            &scroll,
            &mut snarl,
        );
        let sizes = std::collections::BTreeMap::from([
            (CanvasNode::SourceBlock(0), egui::vec2(230.0, 300.0)),
            (CanvasNode::TargetBlock(0), egui::vec2(230.0, 300.0)),
        ]);
        (
            Self {
                graph,
                root,
                source_blocks,
                target_blocks,
                source_paths,
                scroll,
                sizes,
            },
            snarl,
            [source, target],
        )
    }

    fn state(&self, snarl: &Snarl<CanvasNode>) -> serde_json::Value {
        serde_json::json!({
            "graph": &self.graph,
            "root": &self.root,
            "blocks": format!("{:?}/{:?}", self.source_blocks, self.target_blocks),
            "view": [CanvasNode::SourceBlock(0), CanvasNode::TargetBlock(0)].map(|node| {
                serde_json::json!({
                    "node": format!("{node:?}"),
                    "width": self.scroll.width(node, 230.0),
                    "offset": self.scroll.offset(node, 20),
                    "visible": format!("{:?}", self.scroll.visible_range(node, 20)),
                    "limit": self.scroll.visible_limit(node, 20),
                    "pins": (0..20).map(|pin| self.scroll.visual_pin_for_semantic(node, pin, 20)).collect::<Vec<_>>(),
                })
            }),
            "sizes": format!("{:?}", self.sizes),
            "positions": format!("{:?}", snarl.nodes_pos_ids().collect::<Vec<_>>()),
            "wires": format!("{:?}", snarl.wires().collect::<Vec<_>>()),
        })
    }

    fn frame(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        node: SnarlNodeId,
        context: &egui::Context,
        x12: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run_ui(
            egui::RawInput {
                time: Some(context.cumulative_frame_nr() as f64 / 10.0),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let mut viewer = GraphViewer {
                    graph: &mut self.graph,
                    root_scope: &mut self.root,
                    primary_root_authoring: false,
                    extra_targets: &[],
                    inactive_target_scopes: &[],
                    project_references: Default::default(),
                    source_blocks: &self.source_blocks,
                    target_blocks: &self.target_blocks,
                    source_x12: x12,
                    target_x12: x12,
                    source_paths: &self.source_paths,
                    function_names: Default::default(),
                    function_inputs: Default::default(),
                    parameter_names: Default::default(),
                    protected_output: None,
                    function_output: None,
                    requested_function_open: None,
                    colors: crate::appearance::SemanticThemeColors::default(),
                    wire_color_mode: crate::appearance::WireColorMode::Theme,
                    endpoint_scroll: &mut self.scroll,
                    value_map_wheel: None,
                    endpoint_search_match: None,
                    node_sizes: Some(&mut self.sizes),
                    hovered_node: None,
                    hovered_node_this_frame: None,
                    camera_pan: egui::Vec2::ZERO,
                    camera_focus: None,
                    canvas_transform: None,
                    pin_interaction_ids: Vec::new(),
                    error: None,
                };
                viewer.show_header(node, &[], &[], ui, snarl);
            },
        )
    }
}

#[test]
fn endpoint_headers_keep_full_hover_accessibility_and_resized_scrolled_model_state() {
    for source in [true, false] {
        for x12 in [false, true] {
            for width in [110.0, 230.0, 480.0] {
                let (mut fixture, mut snarl, nodes) = EndpointHeaderFixture::new();
                let (node, endpoint, title, icon, special) = if source {
                    (
                        nodes[0],
                        CanvasNode::SourceBlock(0),
                        fixture.source_blocks[0].title.clone(),
                        lucide_icons::Icon::ArrowRightFromLine,
                        "X12 BEG: Beginning Segment for Purchase Order",
                    )
                } else {
                    (
                        nodes[1],
                        CanvasNode::TargetBlock(0),
                        fixture.target_blocks[0].title.clone(),
                        lucide_icons::Icon::ArrowRightToLine,
                        "X12 NM1: Individual or Organizational Name",
                    )
                };
                fixture
                    .scroll
                    .resize(endpoint, 20, width, egui::vec2(-1000.0, 0.0));
                fixture
                    .scroll
                    .resize(endpoint, 20, width, egui::vec2(width - 110.0, 0.0));
                let before = fixture.state(&snarl);
                let serialized_before =
                    serde_json::to_vec(&before).expect("serializable endpoint state");
                let expected_hover = if x12 {
                    format!("{title}\n{special}")
                } else {
                    title.clone()
                };
                let context = egui::Context::default();
                crate::icons::install(&context);
                context.enable_accesskit();
                let mut accessible = Vec::new();
                let mut output = fixture.frame(&mut snarl, node, &context, x12, Vec::new());
                for _ in 0..8 {
                    if let Some(update) = &output.platform_output.accesskit_update {
                        accessible.extend(
                            update
                                .nodes
                                .iter()
                                .filter_map(|(_, node)| node.value().map(str::to_owned)),
                        );
                    }
                    output = fixture.frame(&mut snarl, node, &context, x12, Vec::new());
                }
                eprintln!(
                    "endpoint unhovered source={source} x12={x12} width={width}: before={before}; after={}; accessible={accessible:?}; shapes={:?}",
                    fixture.state(&snarl),
                    output.shapes
                );
                assert!(accessible.iter().any(|value| value == &title));
                let visible = title
                    .strip_prefix(if source { "Source: " } else { "Target: " })
                    .expect("fixture role prefix");
                let text = endpoint_text(&output, visible).expect("endpoint title painted");
                assert!(text.galley.elided);
                assert_eq!(text.galley.rows.len(), 1);
                assert!(text.visual_bounding_rect().width() <= width - 32.0 + 1.0);
                let icon = char::from(icon).to_string();
                let hover = endpoint_text(&output, &icon)
                    .expect("role icon painted")
                    .visual_bounding_rect()
                    .center();
                for frame in 0..8 {
                    output = fixture.frame(
                        &mut snarl,
                        node,
                        &context,
                        x12,
                        if frame == 0 {
                            vec![egui::Event::PointerMoved(hover)]
                        } else {
                            Vec::new()
                        },
                    );
                }
                let after = fixture.state(&snarl);
                eprintln!(
                    "endpoint hovered source={source} x12={x12} width={width}: expected={expected_hover:?}; shapes={:?}; before={before}; after={after}",
                    output.shapes
                );
                assert!(endpoint_text(&output, &expected_hover).is_some());
                assert_eq!(after, before);
                assert_eq!(
                    serde_json::to_vec(&after).expect("serializable endpoint state"),
                    serialized_before
                );
                assert_eq!(fixture.scroll.width(endpoint, 999.0), width);
                assert_eq!(fixture.scroll.offset(endpoint, 20), 5);
                assert_eq!(
                    fixture.scroll.visual_pin_for_semantic(endpoint, 15, 20),
                    Some(11)
                );
            }
        }
    }
}
