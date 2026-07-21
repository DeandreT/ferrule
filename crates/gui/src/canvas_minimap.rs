//! Compact navigation overlay for the mapping canvas.

use std::collections::BTreeMap;

use egui_snarl::Snarl;

use crate::appearance::SemanticThemeColors;
use crate::canvas::CanvasNode;

const MAX_SIZE: egui::Vec2 = egui::vec2(200.0, 132.0);
const MIN_SIZE: egui::Vec2 = egui::vec2(132.0, 88.0);
const MARGIN: f32 = 12.0;
const INNER_MARGIN: f32 = 8.0;

pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    viewport: egui::Rect,
    snarl: &Snarl<CanvasNode>,
    node_sizes: &BTreeMap<CanvasNode, egui::Vec2>,
    canvas_transform: egui::emath::TSTransform,
    colors: SemanticThemeColors,
) -> Option<egui::Pos2> {
    let graph_bounds = graph_bounds(snarl, node_sizes)?;
    if viewport.width() < MIN_SIZE.x + MARGIN * 2.0 || viewport.height() < MIN_SIZE.y + MARGIN * 2.0
    {
        return None;
    }
    let size = egui::vec2(
        MAX_SIZE.x.min(viewport.width() * 0.24).max(MIN_SIZE.x),
        MAX_SIZE.y.min(viewport.height() * 0.22).max(MIN_SIZE.y),
    );
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            viewport.right() - size.x - MARGIN,
            viewport.bottom() - size.y - MARGIN,
        ),
        size,
    );
    let inner = rect.shrink(INNER_MARGIN);
    let map = egui::emath::RectTransform::from_to(graph_bounds, fitted_rect(graph_bounds, inner));
    let painter = ui.painter_at(rect);
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

    let response = ui
        .interact(rect, id.with("minimap"), egui::Sense::click_and_drag())
        .on_hover_text("Navigate mapping canvas");
    (response.clicked() || response.dragged())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|pointer| map.inverse().transform_pos_clamped(pointer))
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
}
