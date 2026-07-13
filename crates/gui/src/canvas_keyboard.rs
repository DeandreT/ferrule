use egui_snarl::Snarl;
use egui_snarl::ui::SnarlWidget;

use crate::canvas::CanvasNode;
use crate::graph_viewer::GraphViewer;

const CANVAS_ID: &str = "mapping_canvas";

pub fn show(
    snarl: &mut Snarl<CanvasNode>,
    viewer: &mut GraphViewer<'_>,
    view_generation: u64,
    ui: &mut egui::Ui,
) {
    let canvas_id = egui::Id::new((CANVAS_ID, view_generation));
    let selected = SnarlWidget::new().id(canvas_id).get_selected_nodes(ui);
    let delete = !ui.ctx().egui_wants_keyboard_input()
        && ui.ctx().input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        });
    if delete {
        viewer.remove_snarl_nodes(&selected, snarl);
    }
    SnarlWidget::new().id(canvas_id).show(snarl, viewer, ui);
}
