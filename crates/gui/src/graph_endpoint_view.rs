//! Endpoint presentation controls and canonical connection/view reconciliation.

use super::*;

impl GraphViewer<'_> {
    /// Refresh membership before converting semantic pins to the visible rows.
    /// Do not infer connected fields from a proxy pin or a filtered wire set.
    pub(crate) fn sync_endpoint_view(&mut self, snarl: &mut Snarl<CanvasNode>) -> bool {
        let owned_items = if self.function_output.is_none() {
            graph_references::sequence_item_ids(
                self.graph,
                self.root_scope,
                self.extra_targets,
                self.inactive_target_scopes,
                self.project_references,
            )
        } else {
            Default::default()
        };
        let unavailable_blocks = self.project_references.source_conflicts();
        let changed = crate::app::update_endpoint_connections_with_policy(
            self.graph,
            self.root_scope,
            self.source_blocks,
            self.target_blocks,
            self.endpoint_scroll,
            snarl,
            crate::app::SourcePinPolicy {
                owned_items: &owned_items,
                unavailable_blocks: &unavailable_blocks,
            },
        );
        crate::app::sync_endpoint_wires_with_policy(
            self.graph,
            self.root_scope,
            self.source_blocks,
            self.target_blocks,
            self.endpoint_scroll,
            snarl,
            crate::app::SourcePinPolicy {
                owned_items: &owned_items,
                unavailable_blocks: &unavailable_blocks,
            },
        );
        changed
    }
}

pub(super) fn show_filter(
    ui: &mut Ui,
    node: CanvasNode,
    total: usize,
    state: &mut crate::canvas_endpoints::EndpointScrollState,
) {
    ui.horizontal_wrapped(|ui| {
        for (label, connected_only) in [("All fields", false), ("Connected fields", true)] {
            if ui
                .selectable_label(state.connected_only(node) == connected_only, label)
                .clicked()
                && state.set_connected_only(node, connected_only)
            {
                ui.ctx().request_repaint();
            }
        }
        let displayed = state.visible_indices(node, total).len();
        ui.label(format!("{displayed} / {total}"))
            .on_hover_text(format!(
                "{} connected fields; {} displayed of {total} schema fields. Search also includes unconnected fields.",
                state.connected_count(node, total),
                displayed,
            ));
    });
}
