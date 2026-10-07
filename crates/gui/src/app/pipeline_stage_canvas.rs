use super::*;

pub(super) struct StageCanvasSession {
    pub(super) editor: Box<FerruleApp>,
    path: PathBuf,
    index: usize,
    stage_id: String,
    pipeline_key: String,
    project_key: String,
    pub(super) error: Option<String>,
    close: Option<StageClose>,
}

#[derive(Clone, Copy)]
enum StageClose {
    Pipeline,
    App,
}

impl StageCanvasSession {
    fn dirty(&self) -> bool {
        self.editor.join_authoring_draft.is_some()
            || crate::project_state::project_snapshot_key(&self.editor.project) != self.project_key
    }
}

impl FerruleApp {
    pub(super) fn stage_document_actions_blocked(&self) -> bool {
        self.pipeline_stage_canvas.is_some() || self.embedded_stage_namespace.is_some()
    }

    pub(super) fn embedded_canvas_id(&self, ordinary: egui::Id) -> egui::Id {
        self.embedded_stage_namespace
            .map_or(ordinary, |namespace| namespace.with(ordinary))
    }

    pub(super) fn begin_pipeline_stage_canvas(&mut self, index: usize) {
        if !self.ui_project_editing_enabled()
            || self.embedded_stage_namespace.is_some()
            || self.pipeline_run_draft.is_some()
            || self.pending_pipeline_editor_action.is_some()
            || self.pending_save_continuation.is_some()
            || self.new_function_draft.is_some()
        {
            return;
        }
        self.observe_editor_history(std::time::Instant::now(), false);
        self.commit_pending_history();
        let Some(pipeline_editor) = self.pipeline_editor.as_mut() else {
            return;
        };
        if pipeline_editor.selected_stage != Some(index) {
            return;
        }
        if pipeline_editor.has_unapplied_text() {
            pipeline_editor
                .set_stage_canvas_error("Apply staged text edits before opening a stage canvas");
            return;
        }
        let Some(stage) = pipeline_editor.document.pipeline.stages.get(index) else {
            return;
        };
        let path = pipeline_editor.document.path.clone();
        let stage_id = stage.id.clone();
        let project_key = crate::project_state::project_snapshot_key(&stage.project);
        let pipeline_key =
            crate::project_state::pipeline_snapshot_key(&pipeline_editor.document.pipeline);
        let Some(generation) = self.stage_canvas_generation.checked_add(1) else {
            pipeline_editor.set_stage_canvas_error(
                "Stage session identities are exhausted; reopen the application",
            );
            return;
        };
        self.stage_canvas_generation = generation;
        let namespace = egui::Id::new((
            "embedded_pipeline_stage",
            generation,
            &path,
            index,
            &stage_id,
            &pipeline_key,
        ));
        let mut editor = Box::new(FerruleApp {
            project: stage.project.clone(),
            document: DocumentLocation::untitled(&path),
            embedded_stage_namespace: Some(namespace),
            appearance: self.appearance,
            theme: self.theme,
            palette: self.palette,
            show_minimap: self.show_minimap,
            ..Default::default()
        });
        editor.main_canvas = CanvasDocumentState::main(&editor.project);
        editor.mapping_workspace = MappingWorkspace::default();
        editor.mark_clean();
        editor.rebase_history();
        self.pipeline_stage_canvas = Some(StageCanvasSession {
            editor,
            path,
            index,
            stage_id,
            pipeline_key,
            project_key,
            error: None,
            close: None,
        });
    }

    pub(super) fn apply_pipeline_stage_canvas(&mut self) -> bool {
        let Some(session) = self.pipeline_stage_canvas.as_ref() else {
            return false;
        };
        let result = (|| {
            let pipeline = self
                .pipeline_editor
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("The pipeline editor was closed"))?;
            anyhow::ensure!(
                pipeline.document.path == session.path,
                "The pipeline document changed"
            );
            anyhow::ensure!(
                pipeline.selected_stage == Some(session.index),
                "The selected stage changed"
            );
            anyhow::ensure!(
                !pipeline.has_unapplied_text(),
                "Apply staged pipeline text edits first"
            );
            anyhow::ensure!(
                session.editor.join_authoring_draft.is_none(),
                "Apply or cancel the staged join edit first"
            );
            pipeline.document.replace_stage_project(
                session.index,
                &session.stage_id,
                &session.pipeline_key,
                &session.editor.project,
            )
        })();
        match result {
            Ok(()) => {
                self.pipeline_stage_canvas = None;
                self.status =
                    "applied stage mapping; save the pipeline before Preview or Run".into();
                true
            }
            Err(error) => {
                if let Some(session) = self.pipeline_stage_canvas.as_mut() {
                    session.error = Some(format!("{error:#}"));
                }
                false
            }
        }
    }

    fn request_pipeline_stage_canvas_close(&mut self, context: &egui::Context, close: StageClose) {
        let Some(session) = self.pipeline_stage_canvas.as_mut() else {
            return;
        };
        if session.dirty() {
            session.close = Some(close);
        } else {
            self.pipeline_stage_canvas = None;
            if matches!(close, StageClose::App) {
                self.resume_stage_app_close(context);
            }
        }
    }

    pub(super) fn guard_pipeline_stage_canvas_close(&mut self, context: &egui::Context) -> bool {
        let Some(session) = self.pipeline_stage_canvas.as_mut() else {
            return false;
        };
        if !session.dirty() {
            self.pipeline_stage_canvas = None;
            return false;
        }
        context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        session.close = Some(StageClose::App);
        true
    }

    fn resume_stage_app_close(&mut self, context: &egui::Context) {
        self.guard_app_close_requested(context, true);
        if self.pending_pipeline_editor_action.is_none()
            && self.pending_destructive_action.is_none()
            && self.pending_file_run.is_none()
            && self.pending_pipeline_run.is_none()
            && self.pending_rest_run.is_none()
            && let Some(action) = self.request_destructive_action(DestructiveAction::Close)
        {
            self.perform_destructive_action(action, context);
        }
    }

    pub(super) fn show_pipeline_stage_close_guard(&mut self, context: &egui::Context) {
        let Some(close) = self
            .pipeline_stage_canvas
            .as_ref()
            .and_then(|session| session.close)
        else {
            return;
        };
        let mut apply = false;
        let mut discard = false;
        let mut keep = false;
        egui::Window::new("Unapplied stage mapping changes")
            .id(egui::Id::new("pipeline_stage_close_guard"))
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label(
                    "Apply the stage mapping to the pipeline, discard this draft, or keep editing.",
                );
                if let Some(error) = self
                    .pipeline_stage_canvas
                    .as_ref()
                    .and_then(|session| session.error.as_ref())
                {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    apply = ui.button("Apply stage changes").clicked();
                    discard = ui.button("Discard stage changes").clicked();
                    keep = ui.button("Keep editing stage").clicked();
                });
            });
        if keep && let Some(session) = self.pipeline_stage_canvas.as_mut() {
            session.close = None;
        }
        let finished = if apply {
            self.apply_pipeline_stage_canvas()
        } else if discard {
            self.pipeline_stage_canvas = None;
            true
        } else {
            false
        };
        if finished && matches!(close, StageClose::App) {
            self.resume_stage_app_close(context);
        }
    }

    pub(super) fn show_pipeline_stage_canvas(&mut self, ui: &mut egui::Ui) {
        let Some(mut session) = self.pipeline_stage_canvas.take() else {
            return;
        };
        let context = ui.ctx().clone();
        let namespace = session
            .editor
            .embedded_stage_namespace
            .expect("stage editor has a namespace");
        let enabled = session.close.is_none();
        let mut apply = false;
        let mut cancel = false;
        session.editor.handle_history_shortcuts(&context, enabled);
        egui::Panel::top(namespace.with("toolbar")).show(ui, |ui| {
            ui.strong(format!("Pipeline stage: {}", session.stage_id));
            ui.small(session.path.display().to_string());
            ui.weak("Edit this embedded mapping. Boundary schemas and paths are fixed; canvas layout is temporary.");
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    apply = ui.button("Apply stage mapping").clicked();
                    cancel = ui.button("Cancel stage editing").clicked();
                    if ui.add_enabled(session.editor.can_undo(), egui::Button::new("Undo stage edit")).clicked() {
                        session.editor.undo_project();
                    }
                    if ui.add_enabled(session.editor.history.can_redo(), egui::Button::new("Redo stage edit")).clicked() {
                        session.editor.redo_project();
                    }
                    if ui.button("Validate stage").clicked() { session.editor.validate_now(); }
                });
                let mut function = None;
                egui::ComboBox::from_id_salt(namespace.with("existing_functions"))
                    .selected_text("Existing functions").show_ui(ui, |ui| {
                        for (id, label) in session.editor.function_names() {
                            if ui.selectable_label(false, label).clicked() { function = Some(id); }
                        }
                    });
                if let Some(function) = function { session.editor.open_function_tab(function); }
                session.editor.show_mapping_tabs(ui, enabled);
            });
            if let Some(error) = &session.error { ui.colored_label(ui.visuals().error_fg_color, error); }
        });
        egui::Panel::left(namespace.with("sources"))
            .default_size(220.0)
            .show(ui, |ui| {
                session.editor.show_source_explorer(ui, enabled);
            });
        egui::Panel::right(namespace.with("inspector"))
            .default_size(300.0)
            .show(ui, |ui| {
                session.editor.show_inspector(ui, enabled);
            });
        if !session.editor.diagnostics.is_empty() {
            egui::Panel::bottom(namespace.with("diagnostics"))
                .default_size(120.0)
                .show(ui, |ui| {
                    if let Some(diagnostic) = session.editor.diagnostics.show(ui) {
                        session.editor.navigate_to_diagnostic(&diagnostic);
                    }
                });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            session.editor.show_mapping_workspace_canvas(ui, enabled);
        });
        let coalesce = context.input(|input| {
            input.pointer.primary_down()
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Cut
                            | egui::Event::Paste(_)
                            | egui::Event::Text(_)
                            | egui::Event::Key { .. }
                            | egui::Event::Ime(_)
                    )
                })
        });
        if let Some(delay) = session
            .editor
            .observe_editor_history(std::time::Instant::now(), coalesce)
        {
            context.request_repaint_after(delay);
        }
        self.pipeline_stage_canvas = Some(session);
        if apply {
            self.apply_pipeline_stage_canvas();
        }
        if cancel {
            self.request_pipeline_stage_canvas_close(&context, StageClose::Pipeline);
        }
        // No child App::ui, file dialogs, persistence, save, Preview, Run, or request workers.
    }
}
