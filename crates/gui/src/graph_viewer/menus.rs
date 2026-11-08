//! Canvas and mapping-node context menus.

use super::*;

impl GraphViewer<'_> {
    pub(super) fn show_canvas_menu(
        &mut self,
        pos: egui::Pos2,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        ui.set_style(ui.ctx().global_style());
        if ui
            .add_enabled(
                self.function_output.is_none()
                    && self
                        .source_paths
                        .lookup_binding_supported(self.root_scope, &[])
                    && self.source_paths.first_owned_lookup(0).is_some(),
                egui::Button::new("Lookup from one input"),
            )
            .clicked()
        {
            self.error = self.insert_owned_lookup(snarl, pos).err();
            ui.close();
            return;
        }
        ui.separator();
        for (op, label) in crate::path_picker::OWNED_AGGREGATE_OPS {
            if ui
                .add_enabled(
                    self.function_output.is_none()
                        && self
                            .source_paths
                            .lookup_binding_supported(self.root_scope, &[])
                        && self.source_paths.first_owned_aggregate(op).is_some(),
                    egui::Button::new(format!("{label} from one input")),
                )
                .clicked()
            {
                self.error = self.insert_owned_aggregate(snarl, pos, op).err();
                ui.close();
                return;
            }
        }
        ui.separator();
        if let Some(template) = node_palette::show_available(
            ui,
            self.primary_root_authoring && self.source_paths.first_primary_root_field().is_some(),
            self.primary_root_authoring && self.source_paths.first_primary_root_type().is_some(),
            self.function_output.is_none()
                && self.source_paths.first_open_scalar_object().is_some(),
            self.function_output.is_none()
                && self.source_paths.primary_source_document_path_available(),
            self.function_output.is_none() && self.source_paths.xml_source_elements_available(),
            self.function_output.is_none()
                && self
                    .project_references
                    .user_functions()
                    .and_then(crate::filter_map_editor::default_stages)
                    .is_some(),
        ) {
            self.error = self
                .insert_palette_node_recorded(snarl, pos, template)
                .err();
            ui.close();
        }
    }

    pub(super) fn show_mapping_node_menu(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        ui.set_style(ui.ctx().global_style());
        crate::canvas_tools::size_controls(ui, snarl[node]);
        ui.separator();
        let Some(mapping_id) = Self::mapping_id(snarl[node]) else {
            return;
        };
        if self.function_output.is_some() {
            let select = ui.add_enabled(
                self.protected_output != Some(mapping_id),
                egui::Button::new("Use as function output"),
            );
            if select.clicked() {
                self.select_function_output(mapping_id);
                ui.close();
            }
        }
        if self.function_output.is_none()
            && matches!(
                self.graph.nodes.get(&mapping_id),
                Some(
                    Node::SequenceExists { .. }
                        | Node::SequenceItemAt { .. }
                        | Node::SequenceAggregate { .. }
                )
            )
            && ui.button("Duplicate sequence consumer").clicked()
        {
            crate::app::sequence_consumer_duplication::request(ui.ctx(), mapping_id);
            ui.close();
        }
        let references = self.blocking_references_to(mapping_id);
        ui.separator();
        let remove = ui
            .add_enabled(
                references.is_empty(),
                egui::Button::new(
                    egui::RichText::new("Remove node")
                        .color(crate::theme::active_palette(ui.ctx()).error),
                ),
            )
            .on_disabled_hover_text(format!("Used by: {}", references.join(", ")));
        if remove.clicked() {
            self.remove_snarl_nodes_recorded(&[node], snarl);
            ui.close();
        }
    }
}
