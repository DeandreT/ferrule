use egui_snarl::ui::{SnarlStyle, SnarlWidget};
use egui_snarl::{NodeId as SnarlNodeId, Snarl};

use crate::canvas::CanvasNode;
use crate::graph_viewer::GraphViewer;

const CANVAS_ID: &str = "mapping_canvas";
const TARGET_FIT_EXTENT: f32 = 150.0;
const EDGE_PAN_ZONE: f32 = 72.0;
const EDGE_PAN_SPEED: f32 = 900.0;

#[derive(Clone, Default)]
struct PinInteractionIds(Vec<egui::Id>);

pub fn show(
    snarl: &mut Snarl<CanvasNode>,
    viewer: &mut GraphViewer<'_>,
    view_generation: u64,
    style: SnarlStyle,
    ui: &mut egui::Ui,
) {
    let canvas_id = egui::Id::new((CANVAS_ID, view_generation));
    let viewport = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    let dragged_id = ui.ctx().dragged_id();
    let (pointer, primary_down, frame_seconds) = ui.ctx().input(|input| {
        (
            input.pointer.latest_pos(),
            input.pointer.primary_down(),
            input.stable_dt.min(0.05),
        )
    });
    let previous_pin_ids = ui.ctx().data(|data| {
        data.get_temp::<PinInteractionIds>(canvas_id.with("pin_interaction_ids"))
            .unwrap_or_default()
    });
    let wire_dragging = pin_drag_active(primary_down, dragged_id, &previous_pin_ids.0);
    viewer.camera_pan = edge_pan_delta(viewport, pointer, wire_dragging, frame_seconds);
    if viewer.camera_pan != egui::Vec2::ZERO {
        ui.ctx().request_repaint();
    }
    viewer.pin_interaction_ids.clear();
    let fit_marker = canvas_id.with("initial_fit_complete");
    let hover_marker = canvas_id.with("hovered_node");
    let initialize_fit = ui
        .ctx()
        .data(|data| !data.get_temp::<bool>(fit_marker).unwrap_or(false));
    let hovered_node = ui
        .ctx()
        .data(|data| data.get_temp::<Option<SnarlNodeId>>(hover_marker).flatten());
    viewer.begin_node_hover_frame(hovered_node);
    let selected = SnarlWidget::new()
        .id(canvas_id)
        .style(style)
        .get_selected_nodes(ui);
    let delete = !ui.ctx().egui_wants_keyboard_input()
        && ui.ctx().input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        });
    if delete {
        viewer.remove_snarl_nodes(&selected, snarl);
    }
    let shifted_target = initialize_fit
        .then(|| extend_target_fit_bounds(snarl, TARGET_FIT_EXTENT))
        .flatten();
    SnarlWidget::new()
        .id(canvas_id)
        .style(style)
        .show(snarl, viewer, ui);
    let pin_interaction_ids = std::mem::take(&mut viewer.pin_interaction_ids);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            canvas_id.with("pin_interaction_ids"),
            PinInteractionIds(pin_interaction_ids),
        );
    });
    let hovered_node_this_frame = viewer.end_node_hover_frame();
    ui.ctx()
        .data_mut(|data| data.insert_temp(hover_marker, hovered_node_this_frame));
    if hovered_node_this_frame != hovered_node {
        // egui-snarl resolves wire colors from pins before it reports final
        // node rectangles, so a changed hover needs one immediate repaint.
        ui.ctx().request_repaint();
    }
    if let Some((node, position)) = shifted_target
        && let Some(info) = snarl.get_node_info_mut(node)
    {
        info.pos = position;
    }
    if initialize_fit {
        ui.ctx().data_mut(|data| data.insert_temp(fit_marker, true));
    }
}

fn pin_drag_active(
    primary_down: bool,
    dragged_id: Option<egui::Id>,
    pin_interaction_ids: &[egui::Id],
) -> bool {
    primary_down && dragged_id.is_some_and(|dragged| pin_interaction_ids.contains(&dragged))
}

fn edge_pan_delta(
    viewport: egui::Rect,
    pointer: Option<egui::Pos2>,
    dragging: bool,
    frame_seconds: f32,
) -> egui::Vec2 {
    if !dragging || !viewport.is_positive() || frame_seconds <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let Some(pointer) = pointer else {
        return egui::Vec2::ZERO;
    };
    let pressure = |position: f32, minimum: f32, maximum: f32| {
        let toward_minimum = ((minimum + EDGE_PAN_ZONE - position) / EDGE_PAN_ZONE).clamp(0.0, 1.0);
        let toward_maximum =
            ((position - (maximum - EDGE_PAN_ZONE)) / EDGE_PAN_ZONE).clamp(0.0, 1.0);
        toward_minimum - toward_maximum
    };
    egui::vec2(
        pressure(pointer.x, viewport.left(), viewport.right()),
        pressure(pointer.y, viewport.top(), viewport.bottom()),
    ) * (EDGE_PAN_SPEED * frame_seconds)
}

fn extend_target_fit_bounds(
    snarl: &mut Snarl<CanvasNode>,
    extent: f32,
) -> Option<(SnarlNodeId, egui::Pos2)> {
    let (target, position) = snarl
        .nodes_pos_ids()
        .filter_map(|(node, position, value)| {
            matches!(value, CanvasNode::TargetBlock(_)).then_some((node, position))
        })
        .max_by(|(left_node, left), (right_node, right)| {
            left.x
                .total_cmp(&right.x)
                .then_with(|| left_node.cmp(right_node))
        })?;
    let info = snarl.get_node_info_mut(target)?;
    info.pos.x += extent;
    Some((target, position))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_fit_extent_is_temporary_and_deterministic() {
        let mut snarl = Snarl::new();
        snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
        let first = snarl.insert_node(egui::pos2(400.0, 0.0), CanvasNode::TargetBlock(0));
        let rightmost = snarl.insert_node(egui::pos2(400.0, 120.0), CanvasNode::TargetBlock(1));

        let shifted = extend_target_fit_bounds(&mut snarl, 180.0);

        assert_eq!(shifted, Some((rightmost, egui::pos2(400.0, 120.0))));
        assert_eq!(
            snarl.get_node_info(rightmost).map(|info| info.pos),
            Some(egui::pos2(580.0, 120.0))
        );
        assert_eq!(
            snarl.get_node_info(first).map(|info| info.pos),
            Some(egui::pos2(400.0, 0.0))
        );
        if let Some((node, position)) = shifted
            && let Some(info) = snarl.get_node_info_mut(node)
        {
            info.pos = position;
        }
        assert_eq!(
            snarl.get_node_info(rightmost).map(|info| info.pos),
            Some(egui::pos2(400.0, 120.0))
        );
    }

    #[test]
    fn edge_pan_uses_distance_pressure_only_during_active_drags() {
        let viewport = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(900.0, 650.0));

        assert_eq!(
            edge_pan_delta(viewport, Some(viewport.center()), true, 1.0 / 60.0),
            egui::Vec2::ZERO
        );
        assert_eq!(
            edge_pan_delta(viewport, Some(egui::pos2(100.0, 350.0)), false, 1.0 / 60.0),
            egui::Vec2::ZERO
        );
        let lower_left = edge_pan_delta(viewport, Some(egui::pos2(100.0, 650.0)), true, 1.0 / 60.0);
        assert!((lower_left - egui::vec2(15.0, -15.0)).length() < 0.001);
        let upper_right = edge_pan_delta(viewport, Some(egui::pos2(950.0, 20.0)), true, 1.0 / 60.0);
        assert!((upper_right - egui::vec2(-15.0, 15.0)).length() < 0.001);
    }

    #[test]
    fn edge_pan_activation_accepts_only_recorded_pin_drags() {
        let pin = egui::Id::new("pin");
        let node = egui::Id::new("node");

        assert!(pin_drag_active(true, Some(pin), &[pin]));
        assert!(!pin_drag_active(true, Some(node), &[pin]));
        assert!(!pin_drag_active(false, Some(pin), &[pin]));
        assert!(!pin_drag_active(true, None, &[pin]));
    }
}
