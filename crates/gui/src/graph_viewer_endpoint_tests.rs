use super::*;
use crate::canvas::{source_blocks, target_blocks};
use egui_snarl::ui::SnarlViewer;
use ir::{ScalarType, SchemaNode};

#[test]
fn wheel_scrolls_before_paint_and_routes_hidden_wires_to_edge_proxies() {
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
    let mut transform = egui::emath::TSTransform::from_translation(egui::vec2(0.0, -90.0));
    {
        let mut viewer = GraphViewer {
            graph: &mut graph,
            root_scope: &mut root_scope,
            extra_targets: &[],
            source_blocks: &source_blocks,
            target_blocks: &target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &source_paths,
            colors: crate::appearance::SemanticThemeColors::default(),
            wire_color_mode: crate::appearance::WireColorMode::Theme,
            endpoint_scroll: &mut endpoint_scroll,
            endpoint_search_match: None,
            node_sizes: Some(&mut node_sizes),
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            pending_endpoint_wheel: Some((source_pointer, -90.0)),
            endpoint_wheel_consumed: false,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        viewer.current_transform(&mut transform, &mut snarl);
        assert!(viewer.endpoint_wheel_consumed);
    }

    assert_eq!(transform.translation.y, 0.0);
    assert_eq!(endpoint_scroll.offset(CanvasNode::SourceBlock(0), 20), 5);
    assert_eq!(wire_pins(&snarl), [(0, 0), (11, 12)]);

    let target_pointer = snarl
        .get_node_info(target)
        .map(|info| info.pos + egui::vec2(100.0, 100.0))
        .expect("target endpoint exists");
    transform.translation.y = -90.0;
    {
        let mut viewer = GraphViewer {
            graph: &mut graph,
            root_scope: &mut root_scope,
            extra_targets: &[],
            source_blocks: &source_blocks,
            target_blocks: &target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &source_paths,
            colors: crate::appearance::SemanticThemeColors::default(),
            wire_color_mode: crate::appearance::WireColorMode::Theme,
            endpoint_scroll: &mut endpoint_scroll,
            endpoint_search_match: None,
            node_sizes: Some(&mut node_sizes),
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            pending_endpoint_wheel: Some((target_pointer, -90.0)),
            endpoint_wheel_consumed: false,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        viewer.current_transform(&mut transform, &mut snarl);
        assert!(viewer.endpoint_wheel_consumed);
    }

    assert_eq!(endpoint_scroll.offset(CanvasNode::TargetBlock(0), 20), 5);
    assert_eq!(wire_pins(&snarl), [(0, 0), (11, 11)]);
}
