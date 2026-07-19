use egui_snarl::ui::{SnarlStyle, SnarlWidget};
use egui_snarl::{NodeId as SnarlNodeId, Snarl};

use crate::canvas::CanvasNode;
use crate::graph_viewer::GraphViewer;

const CANVAS_ID: &str = "mapping_canvas";
const TARGET_FIT_EXTENT: f32 = 150.0;

pub fn show(
    snarl: &mut Snarl<CanvasNode>,
    viewer: &mut GraphViewer<'_>,
    view_generation: u64,
    style: SnarlStyle,
    ui: &mut egui::Ui,
) {
    let canvas_id = egui::Id::new((CANVAS_ID, view_generation));
    let fit_marker = canvas_id.with("initial_fit_complete");
    let initialize_fit = ui
        .ctx()
        .data(|data| !data.get_temp::<bool>(fit_marker).unwrap_or(false));
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
    if let Some((node, position)) = shifted_target
        && let Some(info) = snarl.get_node_info_mut(node)
    {
        info.pos = position;
    }
    if initialize_fit {
        ui.ctx().data_mut(|data| data.insert_temp(fit_marker, true));
    }
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
}
