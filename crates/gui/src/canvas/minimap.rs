//! Compact navigation overlay for the mapping canvas.

use std::collections::BTreeMap;

use egui_snarl::Snarl;

use crate::appearance::SemanticThemeColors;
use crate::canvas::CanvasNode;

const DEFAULT_MAX_SIZE: egui::Vec2 = egui::vec2(200.0, 132.0);
const MIN_SIZE: egui::Vec2 = egui::vec2(132.0, 88.0);
const VIEWPORT_FRACTION_LIMIT: egui::Vec2 = egui::vec2(0.55, 0.55);
const MARGIN: f32 = 12.0;
const INNER_MARGIN: f32 = 8.0;
const RESIZE_GRIP_SIZE: f32 = 18.0;
const FOCUS_ZOOM: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationRequest {
    pub graph_position: egui::Pos2,
    pub zoom: Option<f32>,
}

pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    viewport: egui::Rect,
    snarl: &Snarl<CanvasNode>,
    node_sizes: &BTreeMap<CanvasNode, egui::Vec2>,
    canvas_transform: egui::emath::TSTransform,
    colors: SemanticThemeColors,
) -> Option<NavigationRequest> {
    let graph_bounds = graph_bounds(snarl, node_sizes)?;
    if viewport.width() < MIN_SIZE.x + MARGIN * 2.0 || viewport.height() < MIN_SIZE.y + MARGIN * 2.0
    {
        return None;
    }
    let default_size = egui::vec2(
        DEFAULT_MAX_SIZE
            .x
            .min(viewport.width() * 0.24)
            .max(MIN_SIZE.x),
        DEFAULT_MAX_SIZE
            .y
            .min(viewport.height() * 0.22)
            .max(MIN_SIZE.y),
    );
    let size_id = egui::Id::new("mapping_minimap_size");
    let requested_size = ui
        .ctx()
        .data(|data| data.get_temp::<egui::Vec2>(size_id))
        .unwrap_or(default_size);
    let size = clamp_size(requested_size, viewport.size());
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            viewport.right() - size.x - MARGIN,
            viewport.bottom() - size.y - MARGIN,
        ),
        size,
    );
    let layer_id = egui::LayerId::new(egui::Order::Foreground, id.with("minimap_layer"));
    let mut overlay_ui = ui.new_child(
        egui::UiBuilder::new()
            .layer_id(layer_id)
            .max_rect(rect)
            .sense(egui::Sense::hover()),
    );
    overlay_ui.set_clip_rect(viewport);
    let inner = rect.shrink(INNER_MARGIN);
    let map = egui::emath::RectTransform::from_to(graph_bounds, fitted_rect(graph_bounds, inner));
    let painter = overlay_ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, colors.canvas.to_egui().gamma_multiply(0.94));
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, colors.node_border.to_egui()),
        egui::StrokeKind::Inside,
    );

    let positions = snarl
        .nodes_pos_ids()
        .map(|(id, position, node)| {
            let size = node_sizes
                .get(node)
                .copied()
                .unwrap_or_else(|| fallback_size(*node));
            (
                id,
                (
                    map.transform_rect(egui::Rect::from_min_size(position, size)),
                    *node,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let wire_color = colors.wire.to_egui().gamma_multiply(0.55);
    for (from, to) in snarl.wires() {
        if let (Some(from), Some(to)) = (
            positions.get(&from.node).map(|(rect, _)| rect.center()),
            positions.get(&to.node).map(|(rect, _)| rect.center()),
        ) {
            painter.line_segment([from, to], egui::Stroke::new(0.75, wire_color));
        }
    }
    for (rect, node) in positions.values() {
        let color = match node {
            CanvasNode::SourceBlock(_) => colors.source.to_egui(),
            CanvasNode::TargetBlock(_) => colors.target.to_egui(),
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => colors.transform.to_egui(),
        };
        painter.rect_filled(*rect, 1.0, color.gamma_multiply(0.72));
    }

    let visible_graph = canvas_transform.inverse() * viewport;
    let visible = map.transform_rect(visible_graph).intersect(inner);
    if visible.is_positive() {
        painter.rect_stroke(
            visible,
            1.0,
            egui::Stroke::new(1.5, colors.selection.to_egui()),
            egui::StrokeKind::Inside,
        );
    }

    let resize_corner = rect.left_top();
    let resize_rect = egui::Rect::from_min_size(
        resize_corner,
        egui::vec2(RESIZE_GRIP_SIZE, RESIZE_GRIP_SIZE),
    );
    let response = overlay_ui
        .interact(rect, id.with("minimap"), egui::Sense::click_and_drag())
        .on_hover_text("Click to focus; drag to pan");
    let resize_response = overlay_ui
        .interact(resize_rect, id.with("minimap_resize"), egui::Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeNwSe)
        .on_hover_text("Resize minimap");
    let resize_stroke = egui::Stroke::new(1.0, colors.node_border.to_egui());
    for inset in [4.0, 8.0, 12.0] {
        painter.line_segment(
            [
                resize_corner + egui::vec2(inset, 3.0),
                resize_corner + egui::vec2(3.0, inset),
            ],
            resize_stroke,
        );
    }
    if resize_response.dragged() {
        let delta = resize_response.drag_delta();
        let next = resized_size(size, delta, viewport.size());
        overlay_ui
            .ctx()
            .data_mut(|data| data.insert_temp(size_id, next));
        overlay_ui.ctx().request_repaint();
        return None;
    }

    (response.clicked() || response.dragged())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .filter(|pointer| !resize_rect.contains(*pointer))
        .map(|pointer| NavigationRequest {
            graph_position: map.inverse().transform_pos_clamped(pointer),
            zoom: response.clicked().then_some(FOCUS_ZOOM),
        })
}

fn clamp_size(size: egui::Vec2, viewport: egui::Vec2) -> egui::Vec2 {
    let maximum = viewport * VIEWPORT_FRACTION_LIMIT;
    egui::vec2(
        size.x.clamp(MIN_SIZE.x, maximum.x.max(MIN_SIZE.x)),
        size.y.clamp(MIN_SIZE.y, maximum.y.max(MIN_SIZE.y)),
    )
}

fn resized_size(size: egui::Vec2, top_left_drag: egui::Vec2, viewport: egui::Vec2) -> egui::Vec2 {
    clamp_size(size - top_left_drag, viewport)
}

fn graph_bounds(
    snarl: &Snarl<CanvasNode>,
    node_sizes: &BTreeMap<CanvasNode, egui::Vec2>,
) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    for (_, position, node) in snarl.nodes_pos_ids() {
        let size = node_sizes
            .get(node)
            .copied()
            .unwrap_or_else(|| fallback_size(*node));
        bounds = bounds.union(egui::Rect::from_min_size(position, size));
    }
    bounds.is_finite().then(|| bounds.expand(24.0))
}

fn fitted_rect(graph: egui::Rect, available: egui::Rect) -> egui::Rect {
    let scale = (available.width() / graph.width())
        .min(available.height() / graph.height())
        .max(0.000_1);
    egui::Rect::from_center_size(available.center(), graph.size() * scale)
}

const fn fallback_size(node: CanvasNode) -> egui::Vec2 {
    match node {
        CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_) => egui::vec2(190.0, 160.0),
        CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => egui::vec2(150.0, 72.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_rect_preserves_graph_aspect_ratio() {
        let graph = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_000.0, 200.0));
        let available = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 100.0));

        let fitted = fitted_rect(graph, available);

        assert!((fitted.width() / fitted.height() - 5.0).abs() < 0.001);
        assert!(available.contains_rect(fitted));
    }

    #[test]
    fn graph_bounds_include_measured_node_extents() {
        let mut snarl = Snarl::new();
        snarl.insert_node(egui::pos2(10.0, 20.0), CanvasNode::Graph(1));
        snarl.insert_node(egui::pos2(300.0, 120.0), CanvasNode::TargetBlock(0));
        let sizes = BTreeMap::from([
            (CanvasNode::Graph(1), egui::vec2(80.0, 40.0)),
            (CanvasNode::TargetBlock(0), egui::vec2(160.0, 100.0)),
        ]);

        let bounds = graph_bounds(&snarl, &sizes);

        assert!(bounds.is_some_and(|bounds| {
            bounds.contains(egui::pos2(10.0, 20.0)) && bounds.contains(egui::pos2(460.0, 220.0))
        }));
    }

    #[test]
    fn minimap_size_is_bounded_by_controls_and_viewport() {
        assert_eq!(
            clamp_size(egui::vec2(40.0, 40.0), egui::vec2(1_000.0, 800.0)),
            MIN_SIZE
        );
        assert_eq!(
            clamp_size(egui::vec2(900.0, 700.0), egui::vec2(1_000.0, 800.0)),
            egui::vec2(550.0, 440.0)
        );
    }

    #[test]
    fn dragging_the_anchored_minimap_corner_resizes_in_both_axes() {
        let viewport = egui::vec2(1_000.0, 800.0);
        let size = egui::vec2(200.0, 132.0);

        assert_eq!(
            resized_size(size, egui::vec2(-40.0, -28.0), viewport),
            egui::vec2(240.0, 160.0)
        );
        assert_eq!(
            resized_size(size, egui::vec2(30.0, 20.0), viewport),
            egui::vec2(170.0, 112.0)
        );
    }
}
