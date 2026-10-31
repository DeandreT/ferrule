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

    /// Scrolls an endpoint list one row when an arrow key would move from the
    /// visible field `pin` onto the hidden-fields pin next to it, and returns
    /// the pin that then shows the next field.
    pub(crate) fn scroll_endpoint_field(
        &mut self,
        pin: CanvasPin,
        step: isize,
        snarl: &mut Snarl<CanvasNode>,
    ) -> Option<CanvasPin> {
        let node = *snarl.get_node(pin.node())?;
        let total = match node {
            CanvasNode::SourceBlock(block) => self.source_blocks.get(block)?.leaves.len(),
            CanvasNode::TargetBlock(block) => self.target_blocks.get(block)?.leaves.len(),
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => return None,
        };
        let Some(EndpointDisplayPin::Visible(_)) = self.endpoint_display_pin(node, pin.index())
        else {
            return None;
        };
        let neighbour = self.endpoint_display_pin(node, pin.index().checked_add_signed(step)?)?;
        let hidden = match neighbour {
            EndpointDisplayPin::HiddenBefore => step < 0,
            EndpointDisplayPin::HiddenAfter => step > 0,
            EndpointDisplayPin::Visible(_) => false,
        };
        if !hidden || !self.endpoint_scroll.scroll_rows(node, total, step) {
            return None;
        }
        let visible = self.endpoint_scroll.visible_indices(node, total);
        let revealed = if step < 0 {
            visible.first()
        } else {
            visible.last()
        };
        let display = self
            .endpoint_scroll
            .visual_pin_for_semantic(node, *revealed?, total)?;
        self.sync_endpoint_view(snarl);
        Some(pin.with_index(display))
    }
}

pub(super) fn show_filter(
    ui: &mut Ui,
    node: CanvasNode,
    total: usize,
    state: &mut crate::canvas_endpoints::EndpointScrollState,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::ui_kit::space::XS;
        for (icon, label, connected_only) in [
            (lucide_icons::Icon::List, "All fields", false),
            (lucide_icons::Icon::Cable, "Connected fields", true),
        ] {
            if ui
                .add(egui::Button::selectable(
                    state.connected_only(node) == connected_only,
                    crate::icons::text(icon, crate::ui_kit::METRICS.icon_small),
                ))
                .accessible_name(label)
                .on_hover_text(label)
                .clicked()
                && state.set_connected_only(node, connected_only)
            {
                ui.ctx().request_repaint();
            }
        }
        let displayed = state.visible_indices(node, total).len();
        ui.add(egui::Label::new(format!("{displayed}/{total}")).extend())
            .on_hover_text(format!(
                "{} connected fields; {} displayed of {total} schema fields. Search also includes unconnected fields.",
                state.connected_count(node, total),
                displayed,
            ));
    });
}
