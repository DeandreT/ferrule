use super::*;

use std::collections::BTreeMap;

use mapping::PipelineInput;

pub(super) struct PipelineEditorUi {
    pub document: crate::pipeline_edit::PipelineEditorDocument,
    selected_stage: Option<usize>,
    rename_draft: String,
    host_drafts: BTreeMap<(usize, usize), String>,
    error: Option<String>,
}

#[derive(Clone)]
pub(super) enum PipelineEditorAction {
    Close,
    CloseApp,
    Open(PathBuf),
    Create(PathBuf),
}

enum PipelineEditCommand {
    Rename(usize, String),
    SetInput(usize, usize, PipelineInput),
    AddMissingBindings(usize),
    Remove(usize),
}

impl PipelineEditorUi {
    fn new(document: crate::pipeline_edit::PipelineEditorDocument) -> Self {
        let selected_stage = (!document.pipeline.stages.is_empty()).then_some(0);
        let rename_draft = document
            .pipeline
            .stages
            .first()
            .map_or_else(String::new, |stage| stage.id.clone());
        let mut editor = Self {
            document,
            selected_stage,
            rename_draft,
            host_drafts: BTreeMap::new(),
            error: None,
        };
        editor.refresh_host_drafts();
        editor
    }

    fn select(&mut self, index: usize) {
        self.selected_stage = Some(index);
        self.rename_draft = self.document.pipeline.stages[index].id.clone();
    }

    fn has_unapplied_rename(&self) -> bool {
        self.selected_stage.is_some_and(|index| {
            self.document
                .pipeline
                .stages
                .get(index)
                .is_some_and(|stage| stage.id != self.rename_draft)
        })
    }

    fn has_unapplied_text(&self) -> bool {
        if self.has_unapplied_rename() {
            return true;
        }
        self.document
            .pipeline
            .stages
            .iter()
            .enumerate()
            .any(|(stage_index, stage)| {
                std::iter::once(&stage.source)
                    .chain(stage.extra_sources.iter().map(|binding| &binding.from))
                    .enumerate()
                    .any(|(binding_index, input)| match input {
                        PipelineInput::Host { name } => self
                            .host_drafts
                            .get(&(stage_index, binding_index))
                            .is_some_and(|draft| draft != name),
                        PipelineInput::StageTarget { .. } => false,
                    })
            })
    }

    fn is_dirty(&self) -> bool {
        self.document.is_dirty() || self.has_unapplied_text()
    }

    fn refresh_host_drafts(&mut self) {
        self.host_drafts.clear();
        for (stage_index, stage) in self.document.pipeline.stages.iter().enumerate() {
            for (binding_index, input) in std::iter::once(&stage.source)
                .chain(stage.extra_sources.iter().map(|binding| &binding.from))
                .enumerate()
            {
                let name = match input {
                    PipelineInput::Host { name } => name.clone(),
                    PipelineInput::StageTarget { .. } => {
                        format!("{}-input-{binding_index}", stage.id)
                    }
                };
                self.host_drafts.insert((stage_index, binding_index), name);
            }
        }
    }

    fn apply(&mut self, command: PipelineEditCommand) -> anyhow::Result<()> {
        match command {
            PipelineEditCommand::Rename(index, id) => {
                self.document.rename_stage(index, &id)?;
                self.rename_draft = id;
            }
            PipelineEditCommand::SetInput(stage, binding, input) => {
                self.document.set_input(stage, binding, input.clone())?;
                if let PipelineInput::Host { name } = input {
                    self.host_drafts.insert((stage, binding), name);
                }
            }
            PipelineEditCommand::AddMissingBindings(index) => {
                self.document.add_missing_static_bindings(index)?;
                self.refresh_host_drafts();
            }
            PipelineEditCommand::Remove(index) => {
                self.document.remove_stage(index)?;
                self.selected_stage = if self.document.pipeline.stages.is_empty() {
                    None
                } else {
                    Some(index.min(self.document.pipeline.stages.len() - 1))
                };
                if let Some(selected) = self.selected_stage {
                    self.rename_draft = self.document.pipeline.stages[selected].id.clone();
                }
                self.refresh_host_drafts();
            }
        }
        self.error = None;
        Ok(())
    }
}

impl FerruleApp {
    pub(super) fn request_pipeline_editor_action(&mut self, action: PipelineEditorAction) {
        if self
            .pipeline_editor
            .as_ref()
            .is_some_and(PipelineEditorUi::is_dirty)
        {
            self.pending_pipeline_editor_action = Some(action);
        } else {
            self.perform_pipeline_editor_action(action);
        }
    }

    fn perform_pipeline_editor_action(&mut self, action: PipelineEditorAction) {
        let result = match action {
            PipelineEditorAction::Close => {
                self.pipeline_editor = None;
                return;
            }
            PipelineEditorAction::CloseApp => {
                unreachable!("application close is handled by the pipeline guard")
            }
            PipelineEditorAction::Open(path) => {
                crate::pipeline_edit::PipelineEditorDocument::load(&path)
            }
            PipelineEditorAction::Create(path) => {
                crate::pipeline_edit::PipelineEditorDocument::create(&path)
            }
        };
        match result {
            Ok(document) => {
                self.status = format!("editing pipeline {}", document.path.display());
                self.pipeline_editor = Some(PipelineEditorUi::new(document));
            }
            Err(error) => {
                self.status = "failed to open pipeline editor".into();
                self.diagnostics
                    .error("Pipeline editor", format!("{error:#}"));
            }
        }
    }

    pub(super) fn add_pipeline_stage_from_path(&mut self, path: &Path) {
        let Some(editor) = &mut self.pipeline_editor else {
            return;
        };
        if editor.has_unapplied_text() {
            editor.error = Some("Apply staged text edits before adding a project".into());
            return;
        }
        match editor.document.add_project(path) {
            Ok(index) => {
                editor.select(index);
                editor.refresh_host_drafts();
                editor.error = None;
                self.status = format!("added stage from {}", path.display());
            }
            Err(error) => editor.error = Some(format!("{error:#}")),
        }
    }

    pub(super) fn show_pipeline_editor(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.pipeline_editor else {
            return;
        };
        let mut visible = true;
        let mut command = None;
        let mut selected = None;
        let mut add_project = false;
        let mut save = false;
        let mut run_saved = false;
        let mut close = false;
        let issues = editor.document.issues();
        let dirty = editor.is_dirty();
        let unapplied = editor.has_unapplied_text();
        egui::Window::new(if dirty {
            "Pipeline editor *"
        } else {
            "Pipeline editor"
        })
        .open(&mut visible)
        .default_width(730.0)
        .min_width(550.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(editor.document.path.display().to_string());
            ui.small("This pipeline document is separate from the open mapping project.");
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!unapplied, egui::Button::new("Add Project as Stage..."))
                    .clicked()
                {
                    add_project = true;
                }
                if ui.button("Validate").clicked() {
                    editor.error = if issues.is_empty() {
                        None
                    } else {
                        Some(format!("{} validation issue(s)", issues.len()))
                    };
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (index, stage) in editor.document.pipeline.stages.iter().enumerate() {
                    if ui
                        .selectable_label(editor.selected_stage == Some(index), &stage.id)
                        .clicked()
                    {
                        selected = Some(index);
                    }
                }
            });
            if let Some(index) = editor.selected_stage {
                if let Some(stage) = editor.document.pipeline.stages.get(index).cloned() {
                    ui.separator();
                    ui.strong("Selected stage");
                    ui.horizontal(|ui| {
                        ui.label("Stage ID");
                        ui.text_edit_singleline(&mut editor.rename_draft);
                        if ui
                            .add_enabled(
                                editor.rename_draft != stage.id,
                                egui::Button::new("Rename"),
                            )
                            .clicked()
                        {
                            command = Some(PipelineEditCommand::Rename(
                                index,
                                editor.rename_draft.clone(),
                            ));
                        }
                        if ui.button("Remove stage").clicked() {
                            command = Some(PipelineEditCommand::Remove(index));
                        }
                    });
                    if let Some(path) = &stage.mapping_path {
                        ui.small(format!("Mapping identity: {path}"));
                    }
                    ui.separator();
                    ui.strong("Input bindings");
                    let missing = stage
                        .project
                        .extra_sources
                        .iter()
                        .filter(|source| {
                            source.dynamic_path.is_none()
                                && !stage
                                    .extra_sources
                                    .iter()
                                    .any(|binding| binding.name == source.name)
                        })
                        .count();
                    if missing > 0
                        && ui
                            .add_enabled(
                                !unapplied,
                                egui::Button::new(format!("Add {missing} missing host binding(s)")),
                            )
                            .clicked()
                    {
                        command = Some(PipelineEditCommand::AddMissingBindings(index));
                    }
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            if let Some(edit) = show_binding(
                                ui,
                                &editor.document.pipeline,
                                &mut editor.host_drafts,
                                index,
                                0,
                                "Primary source",
                                &stage.source,
                            ) {
                                command = Some(edit);
                            }
                            for (binding_index, binding) in stage.extra_sources.iter().enumerate() {
                                if let Some(edit) = show_binding(
                                    ui,
                                    &editor.document.pipeline,
                                    &mut editor.host_drafts,
                                    index,
                                    binding_index + 1,
                                    &binding.name,
                                    &binding.from,
                                ) {
                                    command = Some(edit);
                                }
                            }
                        });
                }
            } else {
                ui.weak("Add a project to create the first stage.");
            }
            if issues.is_empty() {
                ui.weak("Pipeline valid");
            } else {
                ui.separator();
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("{} validation issue(s)", issues.len()),
                );
                egui::ScrollArea::vertical()
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for issue in &issues {
                            ui.label(format!("• {issue}"));
                        }
                    });
            }
            if let Some(error) = &editor.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if unapplied {
                ui.weak("Apply staged text edits before saving or changing stages.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        editor.document.is_dirty() && !unapplied && issues.is_empty(),
                        egui::Button::new("Save pipeline"),
                    )
                    .clicked()
                {
                    save = true;
                }
                if ui
                    .add_enabled(
                        !dirty && editor.document.is_saved(),
                        egui::Button::new("Run saved pipeline..."),
                    )
                    .clicked()
                {
                    run_saved = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
        if let Some(index) = selected
            && let Some(editor) = &mut self.pipeline_editor
        {
            if editor.has_unapplied_rename() {
                editor.error = Some("Apply the stage ID before switching stages".into());
            } else {
                editor.select(index);
            }
        }
        if let Some(command) = command
            && let Some(editor) = &mut self.pipeline_editor
        {
            if matches!(&command, PipelineEditCommand::Remove(_)) && editor.has_unapplied_text() {
                editor.error = Some("Apply staged text edits before removing a stage".into());
            } else if let Err(error) = editor.apply(command) {
                editor.error = Some(format!("{error:#}"));
            }
        }
        if add_project {
            self.pending_dialog = Some((
                DialogKind::AddPipelineStageProject,
                pick_file("ferrule project", &["json"]),
            ));
        }
        if save {
            let editor = self
                .pipeline_editor
                .as_mut()
                .expect("pipeline editor exists");
            match editor.document.save() {
                Ok(()) => {
                    editor.error = None;
                    self.status = format!("saved pipeline {}", editor.document.path.display());
                }
                Err(error) => editor.error = Some(format!("{error:#}")),
            }
        }
        if run_saved {
            let result = self
                .pipeline_editor
                .as_ref()
                .expect("pipeline editor exists")
                .document
                .ensure_unchanged();
            match result {
                Ok(()) => {
                    let path = self
                        .pipeline_editor
                        .as_ref()
                        .expect("pipeline editor exists")
                        .document
                        .path
                        .clone();
                    self.load_pipeline_for_run(&path);
                }
                Err(error) => {
                    if let Some(editor) = &mut self.pipeline_editor {
                        editor.error = Some(format!("{error:#}"));
                    }
                }
            }
        }
        if close || !visible {
            self.request_pipeline_editor_action(PipelineEditorAction::Close);
        }
    }

    pub(super) fn show_pipeline_editor_guard(&mut self, ctx: &egui::Context) {
        if self.pending_pipeline_editor_action.is_none() {
            return;
        }
        let mut discard = false;
        let mut keep = false;
        egui::Window::new("Unsaved pipeline changes")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("The pipeline has unsaved edits.");
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() {
                        keep = true;
                    }
                    if ui.button("Discard changes").clicked() {
                        discard = true;
                    }
                });
            });
        if keep {
            self.pending_pipeline_editor_action = None;
        }
        if discard {
            self.discard_pending_pipeline_editor_action(ctx);
        }
    }

    pub(super) fn guard_app_close_requested(&mut self, ctx: &egui::Context, close_requested: bool) {
        if !close_requested || self.allow_close {
            return;
        }
        if self.pending_file_run.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_file_run = true;
            if self
                .pending_file_run
                .as_ref()
                .is_some_and(|run| !matches!(run.phase, run_ui::FileRunPhase::Publishing))
            {
                self.file_run_command(run_ui::FileRunCommand::Cancel);
            }
            return;
        }
        if self
            .pipeline_editor
            .as_ref()
            .is_some_and(PipelineEditorUi::is_dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_pipeline_editor_action
                .get_or_insert(PipelineEditorAction::CloseApp);
        } else if self.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_destructive_action
                .get_or_insert(DestructiveAction::Close);
        }
    }

    pub(super) fn discard_pending_pipeline_editor_action(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_pipeline_editor_action.take() else {
            return;
        };
        match action {
            PipelineEditorAction::CloseApp => {
                self.pipeline_editor = None;
                if let Some(close) = self.request_destructive_action(DestructiveAction::Close) {
                    self.perform_destructive_action(close, ctx);
                }
            }
            other => self.perform_pipeline_editor_action(other),
        }
    }
}

fn show_binding(
    ui: &mut egui::Ui,
    pipeline: &mapping::Pipeline,
    drafts: &mut BTreeMap<(usize, usize), String>,
    stage_index: usize,
    binding_index: usize,
    label: &str,
    input: &PipelineInput,
) -> Option<PipelineEditCommand> {
    let mut edit = None;
    ui.group(|ui| {
        ui.strong(label);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("pipeline-binding", stage_index, binding_index))
                .selected_text(input_label(input))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(matches!(input, PipelineInput::Host { .. }), "Host input")
                        .clicked()
                    {
                        let name = drafts
                            .get(&(stage_index, binding_index))
                            .cloned()
                            .unwrap_or_else(|| {
                                format!("{}-input-{binding_index}", pipeline.stages[stage_index].id)
                            });
                        edit = Some(PipelineEditCommand::SetInput(
                            stage_index,
                            binding_index,
                            PipelineInput::Host { name },
                        ));
                    }
                    for (other_index, stage) in pipeline.stages.iter().enumerate() {
                        if other_index == stage_index {
                            continue;
                        }
                        for target in std::iter::once(None).chain(
                            stage
                                .project
                                .extra_targets
                                .iter()
                                .map(|target| Some(target.name.as_str())),
                        ) {
                            let choice = PipelineInput::StageTarget {
                                stage: stage.id.clone(),
                                target: target.map(str::to_owned),
                            };
                            if ui
                                .selectable_label(input == &choice, input_label(&choice))
                                .clicked()
                            {
                                edit = Some(PipelineEditCommand::SetInput(
                                    stage_index,
                                    binding_index,
                                    choice,
                                ));
                            }
                        }
                    }
                });
            if let PipelineInput::Host { name } = input {
                let draft = drafts
                    .entry((stage_index, binding_index))
                    .or_insert_with(|| name.clone());
                ui.add(egui::TextEdit::singleline(draft).hint_text("Host name"));
                if ui
                    .add_enabled(draft != name, egui::Button::new("Apply name"))
                    .clicked()
                {
                    edit = Some(PipelineEditCommand::SetInput(
                        stage_index,
                        binding_index,
                        PipelineInput::Host {
                            name: draft.clone(),
                        },
                    ));
                }
            }
        });
    });
    edit
}

fn input_label(input: &PipelineInput) -> String {
    match input {
        PipelineInput::Host { name } => format!("Host: {name}"),
        PipelineInput::StageTarget { stage, target } => match target {
            Some(target) => format!("{stage} / {target}"),
            None => format!("{stage} / Primary"),
        },
    }
}
