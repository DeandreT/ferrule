//! Authored compact-header fixtures; no external mapping or customer data.

use crate::test_ui::ContextExt as _;

use super::*;
use crate::canvas::{SourceBlock, TargetBlock, source_blocks, target_blocks};
use egui::epaint::{Shape, TextShape};
use ir::{ScalarType, SchemaNode};

/// An ordinary header (If / else), an edit-enabled header (Uppercase) and an
/// edit-enabled inline header (Not), each with hidden unconnected inputs.
const IF_ELSE: NodeId = 10;
const UPPERCASE: NodeId = 11;
const NOT: NodeId = 12;
const SNARL_ID: &str = "compact-header-canvas";

struct CompactHeaderFixture {
    graph: Graph,
    root: Scope,
    source_blocks: Vec<SourceBlock>,
    target_blocks: Vec<TargetBlock>,
    source_paths: SourcePathCatalog,
    scroll: crate::canvas_endpoints::EndpointScrollState,
    sizes: std::collections::BTreeMap<CanvasNode, egui::Vec2>,
    snarl: Snarl<CanvasNode>,
    nodes: [(NodeId, SnarlNodeId); 3],
}

impl CompactHeaderFixture {
    fn new() -> Self {
        let schema = SchemaNode::group("row", vec![SchemaNode::scalar("a", ScalarType::String)]);
        let mut graph = Graph::default();
        for input in 1..=5 {
            graph.nodes.insert(input, Node::Unconnected);
        }
        graph.nodes.insert(
            IF_ELSE,
            Node::If {
                condition: 1,
                then: 2,
                else_: 3,
            },
        );
        graph.nodes.insert(
            UPPERCASE,
            Node::Call {
                function: "upper".into(),
                args: vec![4],
            },
        );
        graph.nodes.insert(
            NOT,
            Node::Call {
                function: "not".into(),
                args: vec![5],
            },
        );
        let mut snarl = Snarl::new();
        let mut top = 60.0;
        let nodes = [IF_ELSE, UPPERCASE, NOT].map(|id| {
            let node = snarl.insert_node(egui::pos2(60.0, top), CanvasNode::Graph(id));
            top += 160.0;
            (id, node)
        });
        snarl.connect(
            egui_snarl::OutPinId {
                node: nodes[1].1,
                output: 0,
            },
            egui_snarl::InPinId {
                node: nodes[0].1,
                input: 0,
            },
        );
        Self {
            graph,
            root: Scope::default(),
            source_blocks: source_blocks(&schema),
            target_blocks: target_blocks(&schema),
            source_paths: SourcePathCatalog::new(&schema, &[]),
            scroll: crate::canvas_endpoints::EndpointScrollState::default(),
            sizes: std::collections::BTreeMap::new(),
            snarl,
            nodes,
        }
    }

    fn snarl_node(&self, id: NodeId) -> SnarlNodeId {
        self.nodes
            .iter()
            .find_map(|&(node, snarl_node)| (node == id).then_some(snarl_node))
            .expect("fixture node")
    }

    fn viewer(&mut self) -> GraphViewer<'_> {
        GraphViewer {
            graph: &mut self.graph,
            root_scope: &mut self.root,
            primary_root_authoring: false,
            extra_targets: &[],
            inactive_target_scopes: &[],
            project_references: Default::default(),
            source_blocks: &self.source_blocks,
            target_blocks: &self.target_blocks,
            source_x12: false,
            target_x12: false,
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
            planned_edits: Vec::new(),
            error: None,
        }
    }

    fn full_title(&mut self, id: NodeId) -> String {
        let title = self.viewer().title(&CanvasNode::Graph(id));
        graph_node_presentation::complete_title(self.graph.nodes.get(&id), title)
    }

    /// The complete hover hint and accessible label of a fixture node.
    fn hint(&mut self, id: NodeId) -> String {
        let full_title = self.full_title(id);
        graph_node_presentation::hint(&self.graph.nodes[&id], &full_title)
    }

    /// Renders the real canvas, including egui-snarl's collapse button, at
    /// unit scale so painted coordinates match layout coordinates.
    fn frame(&mut self, context: &egui::Context, pointer: Option<egui::Pos2>) -> egui::FullOutput {
        let mut snarl = std::mem::take(&mut self.snarl);
        let output = context.run_test_ui(
            egui::RawInput {
                time: Some(context.cumulative_frame_nr() as f64 / 10.0),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            },
            |ui| {
                let mut style = crate::appearance::EditorAppearance::default().to_snarl_style();
                style.min_scale = Some(1.0);
                style.max_scale = Some(1.0);
                egui_snarl::ui::SnarlWidget::new()
                    .id(egui::Id::unique(SNARL_ID))
                    .style(style)
                    .show(&mut snarl, &mut self.viewer(), ui);
            },
        );
        self.snarl = snarl;
        output
    }

    fn frames(&mut self, context: &egui::Context, pointer: Option<egui::Pos2>) -> egui::FullOutput {
        let mut output = self.frame(context, pointer);
        for _ in 0..8 {
            output = self.frame(context, None);
        }
        output
    }

    /// Screen rectangles of a node frame and its stored edit-trigger anchor.
    fn rects(
        &self,
        context: &egui::Context,
        id: NodeId,
    ) -> (Option<egui::Rect>, Option<egui::Rect>) {
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::unique(SNARL_ID));
        let transform = context.layer_transform_to_global(layer).unwrap_or_default();
        let frame = context
            .read_response(
                layer
                    .id
                    .with(("snarl-node", self.snarl_node(id)))
                    .with("frame"),
            )
            .map(|frame| transform * frame.rect);
        let anchor = context
            .data(|data| data.get_temp::<egui::Response>(layer.id.with(("node_edit_anchor", id))))
            .map(|edit| transform * edit.rect);
        (frame, anchor)
    }

    /// Node sizes, positions, pin centres and wires: the canvas geometry the
    /// header width constraint must leave unchanged.
    fn geometry(&self, output: &egui::FullOutput) -> String {
        let mut pins = flat_shapes(output)
            .into_iter()
            .filter_map(|shape| match shape {
                Shape::Circle(circle) => Some(circle.center),
                _ => None,
            })
            .collect::<Vec<_>>();
        pins.sort_by(|left, right| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)));
        format!(
            "sizes={:?} positions={:?} pins={pins:?} wires={:?}",
            self.sizes,
            self.snarl.nodes_pos_ids().collect::<Vec<_>>(),
            self.snarl.wires().collect::<Vec<_>>(),
        )
    }
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::fonts::install(&context);
    crate::icons::install(&context);
    context
}

fn flat_shapes(output: &egui::FullOutput) -> Vec<&Shape> {
    fn flatten<'a>(shape: &'a Shape, shapes: &mut Vec<&'a Shape>) {
        match shape {
            Shape::Vec(nested) => nested.iter().for_each(|shape| flatten(shape, shapes)),
            shape => shapes.push(shape),
        }
    }
    let mut shapes = Vec::new();
    output
        .shapes
        .iter()
        .for_each(|shape| flatten(&shape.shape, &mut shapes));
    shapes
}

fn texts<'a>(shapes: &[&'a Shape]) -> impl Iterator<Item = &'a TextShape> {
    shapes.iter().filter_map(|shape| match shape {
        Shape::Text(text) => Some(text),
        _ => None,
    })
}

/// Painted pieces of one compact header, in screen coordinates.
#[derive(Debug)]
struct PaintedHeader {
    collapse: egui::Rect,
    icon: egui::Rect,
    title: egui::Rect,
    title_rows: usize,
    title_elided: bool,
}

fn painted_header(output: &egui::FullOutput, title: &str) -> PaintedHeader {
    let shapes = flat_shapes(output);
    let text = texts(&shapes)
        .find(|text| text.galley.text() == title)
        .unwrap_or_else(|| panic!("{title:?} header is painted"));
    let title_rect = text.visual_bounding_rect();
    let before_title = |rect: &egui::Rect| {
        rect.y_range().contains(title_rect.center().y) && rect.center().x < title_rect.left()
    };
    // The icon is the single glyph nearest to the title's left.
    let icon = texts(&shapes)
        .filter(|text| text.galley.text().chars().count() == 1)
        .map(TextShape::visual_bounding_rect)
        .filter(before_title)
        .max_by(|left, right| left.center().x.total_cmp(&right.center().x))
        .unwrap_or_else(|| panic!("{title:?} header icon is painted"));
    let collapse = shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Path(path) if path.closed && path.points.len() == 3 => {
                Some(egui::Rect::from_points(&path.points))
            }
            _ => None,
        })
        .filter(before_title)
        .max_by(|left, right| left.center().x.total_cmp(&right.center().x))
        .unwrap_or_else(|| panic!("{title:?} collapse button is painted"));
    PaintedHeader {
        collapse,
        icon,
        title: title_rect,
        title_rows: text.galley.rows.len(),
        title_elided: text.galley.elided,
    }
}

fn display_name(function: &str) -> &'static str {
    functions::builtin(function)
        .map(|builtin| builtin.display_name)
        .expect("builtin function")
}

fn headers() -> [(NodeId, &'static str, bool); 3] {
    [
        (IF_ELSE, "If / else", false),
        (UPPERCASE, display_name("upper"), true),
        (NOT, display_name("not"), true),
    ]
}

#[test]
fn compact_headers_clear_the_collapse_button_and_keep_canvas_geometry() {
    let mut fixture = CompactHeaderFixture::new();
    let context = context();
    let output = fixture.frames(&context, None);
    let geometry = fixture.geometry(&output);
    for (id, title, editable) in headers() {
        let header = painted_header(&output, title);
        let (frame, anchor) = fixture.rects(&context, id);
        let frame = frame.expect("node frame response");
        eprintln!("{title}: {header:?} frame={frame:?} anchor={anchor:?}");
        assert!(
            header.collapse.right() <= header.icon.left(),
            "{title:?} icon {:?} overlaps the collapse button {:?}",
            header.icon,
            header.collapse,
        );
        assert!(header.icon.right() <= header.title.left());
        assert_eq!(header.title_rows, 1);
        assert!(
            !header.title_elided,
            "{title:?} auto width elides its title"
        );
        assert!(header.title.right() <= frame.right());
        // The edit trigger and its popup anchor follow the title in the row.
        assert_eq!(anchor.is_some(), editable, "{title:?} edit anchor");
        if let Some(anchor) = anchor {
            assert!(header.title.right() <= anchor.left());
            assert!(anchor.right() <= frame.right());
        }
    }

    // Revealing each edit trigger paints it at its anchor without moving
    // nodes, pins or wires.
    for (id, title, editable) in headers() {
        let (frame, anchor) = fixture.rects(&context, id);
        let frame = frame.expect("node frame response");
        let output = fixture.frames(&context, Some(frame.center()));
        assert_eq!(fixture.geometry(&output), geometry, "hovering {title:?}");
        let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
        let pencil = texts(&flat_shapes(&output))
            .find(|text| text.galley.text() == pencil)
            .map(TextShape::visual_bounding_rect);
        match (anchor, pencil) {
            (Some(anchor), Some(pencil)) => {
                assert!(editable);
                assert!(
                    anchor.contains(pencil.center()),
                    "{title:?} pencil {pencil:?}"
                );
                assert!(painted_header(&output, title).collapse.right() <= anchor.left());
            }
            (None, None) => assert!(!editable),
            other => panic!("{title:?} edit trigger mismatch: {other:?}"),
        }
    }
}

#[test]
fn compact_headers_keep_hover_and_accessibility_labels() {
    let mut fixture = CompactHeaderFixture::new();
    let context = context();
    context.enable_accesskit();
    let output = fixture.frames(&context, None);
    let accessible = output
        .platform_output
        .accesskit_update
        .iter()
        .flat_map(|update| &update.nodes)
        .flat_map(|(_, node)| [node.label(), node.value()])
        .flatten()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for (id, title, editable) in headers() {
        let hint = fixture.hint(id);
        assert!(
            accessible.contains(&hint),
            "{title:?} accessible label {hint:?} missing from {accessible:?}"
        );
        let edit = format!("Edit {} · F2", fixture.full_title(id));
        assert_eq!(accessible.contains(&edit), editable, "{edit:?}");

        let icon = painted_header(&fixture.frames(&context, None), title).icon;
        let output = fixture.frames(&context, Some(icon.center()));
        assert!(
            texts(&flat_shapes(&output)).any(|text| text.galley.text() == hint),
            "{title:?} hover hint {hint:?} not shown"
        );
    }
}
