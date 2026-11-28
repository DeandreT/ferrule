use super::*;
use crate::canvas_search::FieldPin;
use crate::preview::PreviewTarget;

/// Dock settings before the canvas's explicit "Show field wiring" action.
#[derive(Clone, Copy, Default)]
struct PreviousInspector {
    shown: bool,
    dock_open: bool,
    dock: SideDock,
    pane: WorkspacePane,
}

impl FerruleApp {
    /// Return from a preview column to its exact owning canvas and physical
    /// root field. A nested leaf or a matching name on another target cannot
    /// substitute for a missing column.
    pub(crate) fn reveal_preview_field(&mut self, target: &PreviewTarget, field: &str) -> bool {
        let (document, schema) = match target {
            PreviewTarget::Primary => (MappingDocument::Main, &self.project.target),
            PreviewTarget::Named(name) => {
                let mut matching = self
                    .project
                    .extra_targets
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| &t.name == name);
                let Some((index, found)) = matching.next() else {
                    return false;
                };
                if matching.next().is_some() {
                    return false;
                }
                (MappingDocument::Target(index), &found.schema)
            }
        };
        if field == ir::XML_TYPE_FIELD {
            return false;
        }
        let blocks = target_blocks(schema);
        let mut matches = blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.chain.is_empty())
            .flat_map(|(index, b)| {
                b.leaves
                    .iter()
                    .enumerate()
                    .filter(move |(_, leaf)| leaf.field == field)
                    .map(move |(semantic, _)| FieldPin {
                        node: CanvasNode::TargetBlock(index),
                        semantic,
                    })
            });
        let Some(pin) = matches.next() else {
            return false;
        };
        if matches.next().is_some() {
            return false;
        }

        let canvas = match document {
            MappingDocument::Main => &mut self.main_canvas,
            MappingDocument::Target(index) => {
                if !self.ensure_target_canvas(index) {
                    return false;
                }
                self.mapping_workspace
                    .target_canvases
                    .get_mut(&index)
                    .expect("ensured target canvas")
            }
            MappingDocument::Function(_) => return false,
        };
        if !canvas.snarl.node_ids().any(|(_, node)| *node == pin.node) {
            return false;
        }
        canvas.search.request_reveal(pin);
        if self.mapping_workspace.active != document {
            self.selected_scope.clear();
        }
        if !self.mapping_workspace.tabs.contains(&document) {
            self.mapping_workspace.tabs.push(document);
        }
        self.mapping_workspace.active = document;
        self.mapping_workspace.focused = document;
        self.narrow_pane = WorkspacePane::Canvas;
        self.show_run_report = false;
        true
    }

    pub(in crate::app) fn field_inspector_navigation(
        &mut self,
        ctx: &egui::Context,
        tools_id: egui::Id,
        open: bool,
    ) {
        let key = tools_id.with("before_field_inspector");
        if open {
            ctx.data_mut(|data| {
                data.insert_temp(tools_id.with("focus_field_inspector"), true);
                if data.get_temp::<PreviousInspector>(key).is_none() {
                    data.insert_temp(
                        key,
                        PreviousInspector {
                            shown: self.show_inspector_panel,
                            dock_open: self.compact_dock_open,
                            dock: self.compact_dock,
                            pane: self.narrow_pane,
                        },
                    );
                }
            });
            self.show_inspector_for_navigation();
            ctx.request_repaint();
        } else if ctx.data(|data| data.get_temp::<PreviousInspector>(key).is_some())
            && crate::canvas_tools::field_wiring(ctx, tools_id).is_none()
            && let Some(previous) = ctx.data_mut(|data| data.remove_temp::<PreviousInspector>(key))
        {
            self.show_inspector_panel = previous.shown;
            self.compact_dock_open = previous.dock_open;
            self.compact_dock = previous.dock;
            self.narrow_pane = previous.pane;
            ctx.request_repaint();
        }
    }

    pub(super) fn show_field_wiring_panel(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let (tools_id, target) = match self.mapping_workspace.active {
            MappingDocument::Main => (self.main_canvas.tools_id, PreviewTarget::Primary),
            MappingDocument::Target(i) => {
                let Some(canvas) = self.mapping_workspace.target_canvases.get(&i) else {
                    return;
                };
                let Some(target) = self.project.extra_targets.get(i) else {
                    return;
                };
                (canvas.tools_id, PreviewTarget::Named(target.name.clone()))
            }
            MappingDocument::Function(_) => return,
        };
        let Some(wiring) = crate::canvas_tools::field_wiring(ui.ctx(), tools_id) else {
            return;
        };
        ui.separator();
        ui.strong("Field wiring");
        ui.label(&wiring.label);
        ui.weak("Static connections. Preview values are shown separately.");
        egui::ScrollArea::vertical()
            .id_salt((tools_id, "wiring_path"))
            .max_height((ui.available_height() * 0.32).clamp(64.0, 220.0))
            .show(ui, |ui| {
                if wiring.connections.is_empty() {
                    ui.weak("No incoming connection.");
                }
                for (_, connection) in &wiring.connections {
                    ui.label(connection);
                }
            });
        let mut dismiss = false;
        let focus_action = ui
            .ctx()
            .data_mut(|data| data.remove_temp::<bool>(tools_id.with("focus_field_inspector")))
            .unwrap_or(false);
        ui.horizontal_wrapped(|ui| {
            let root_field = wiring.chain.is_empty();
            let action = ui.add_enabled(
                editing_enabled && self.can_preview(),
                egui::Button::new(if root_field {
                    "Preview this field"
                } else {
                    "Preview target…"
                }),
            );
            if focus_action && action.enabled() {
                action.request_focus();
            }
            if action.clicked() {
                if root_field {
                    self.inspect_preview_field(target, &wiring.field);
                } else {
                    self.begin_preview();
                    self.status = format!("Preview target for {}", wiring.label);
                }
            }
            let close = ui.button("Close field wiring");
            if focus_action && !action.enabled() {
                close.request_focus();
            }
            if !root_field {
                action.on_hover_text("Nested fields are shown in their target document.");
            }
            dismiss = close.clicked();
        });
        // In the narrow layout the canvas isn't drawn behind the inspector.
        let inspector_only = self.last_layout_class == Some(LayoutClass::Narrow)
            && self.narrow_pane == WorkspacePane::Inspector;
        if inspector_only
            && !crate::ui_kit::modal_open(ui.ctx())
            && !egui::Popup::is_any_open(ui.ctx())
        {
            dismiss |=
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
        if dismiss {
            crate::canvas_tools::dismiss_wiring(ui.ctx(), tools_id);
            self.field_inspector_navigation(ui.ctx(), tools_id, false);
        }
        ui.separator();
    }
}
