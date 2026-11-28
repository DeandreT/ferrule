//! Saved checks compare a selected complete preview to a separate known-good
//! file. Only the authoring dialog can capture expectations.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use editor_ui::mapping_checks::{CheckResult, NamedCheck};

use super::FerruleApp;
use crate::preview_table::PreviewTable;
use crate::ui_kit;

mod author;
mod store;
use author::Draft;
use store::{CheckSet, Output, Reference, Store, table_digest};

fn headers(table: &PreviewTable) -> Option<&[String]> {
    table.has_headers.then_some(table.headers.as_slice())
}

struct LastRun {
    digest: [u8; 32],
    results: Vec<CheckResult>,
}

impl CheckSet {
    fn selected<'a>(
        &self,
        selection: (&str, &Path, &'a PreviewTable),
    ) -> Result<&'a PreviewTable, String> {
        let (target, path, table) = selection;
        if !self.output.matches(target, path) {
            return Err(format!(
                "This set checks {}. The selected preview is {target} · {}.",
                self.output.label(),
                path.display()
            ));
        }
        Ok(table)
    }

    fn run(&self, selection: (&str, &Path, &PreviewTable)) -> Result<LastRun, String> {
        let table = self.selected(selection)?;
        Ok(LastRun {
            digest: table_digest(table),
            results: self
                .checks
                .iter()
                .map(|check| check.assertion.evaluate(headers(table), &table.rows))
                .collect(),
        })
    }

    fn resolve_result<'a>(
        &'a self,
        run: &LastRun,
        index: usize,
        selection: (&str, &Path, &PreviewTable),
    ) -> Result<Option<(usize, &'a str)>, String> {
        let table = self.selected(selection)?;
        if table_digest(table) != run.digest {
            return Err(
                "The selected preview changed after this check run. Run checks again.".into(),
            );
        }
        let check = self
            .checks
            .get(index)
            .ok_or("The check is no longer present.")?;
        if run.results.get(index).is_none_or(|result| result.passed) {
            return Ok(None);
        }
        check.assertion.failed_cell(headers(table), &table.rows)
    }
}

pub(super) struct Window {
    owner: egui::Id,
    mapping: PathBuf,
    store: Store,
    selected: usize,
    last_run: Option<LastRun>,
    detail: Option<usize>,
    draft: Option<Draft>,
    picker: Option<(Output, Receiver<Option<String>>)>,
    notice: String,
    reference_notice: String,
}

#[derive(Default)]
enum Action {
    #[default]
    None,
    Close,
    New,
    Run,
    Inspect(usize),
    Select(usize),
    Edit,
    LoadRows,
    Save,
    Cancel,
    Undo,
}

impl FerruleApp {
    /// Run menu entry; also callable from the preview results UI. Reading a
    /// sidecar does not require a preview, a decoder, or the original file.
    pub(crate) fn open_saved_checks(&mut self) {
        if self.saved_checks.is_some() {
            self.show_run_report = false;
            return;
        }
        let Some(path) = self.document.saved_path() else {
            self.status = "Save the mapping before opening its check sets.".into();
            return;
        };
        match Window::open(self.document_ui_id, path) {
            Ok(window) => {
                self.saved_checks = Some(window);
                self.show_run_report = false;
            }
            Err(error) => self
                .diagnostics
                .error("Could not open saved checks", format!("{error:#}")),
        }
    }

    pub(super) fn show_saved_checks(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.saved_checks.take() else {
            return;
        };
        if window.owner != self.document_ui_id
            || Some(window.mapping.as_path()) != self.document.saved_path()
        {
            return;
        }
        // The two docked tools use the same workspace edge. Preserve the list
        // and its selected result while the preview shows the resolved cell.
        if self.show_run_report && !window.editing() {
            self.saved_checks = Some(window);
            return;
        }
        window.poll_reference(self, ctx);
        let action = window.draw(ctx);
        if matches!(action, Action::Close) {
            return;
        }
        if let Err(reason) = window.act(action, self) {
            window.notice = reason;
        }
        self.saved_checks = Some(window);
    }
}

impl Window {
    pub(super) fn show_last_check_detail(&self, ui: &mut egui::Ui) {
        let Some(index) = self.detail else { return };
        let Ok(set) = self.selected_set() else { return };
        if let Some((check, result)) = set.checks.get(index).zip(
            self.last_run
                .as_ref()
                .and_then(|run| run.results.get(index)),
        ) {
            ui.strong(format!(
                "Last check: {} · {}",
                check.name,
                set.output.label()
            ));
            ui.label(format!("Expected: {}", check.assertion.expected_detail()));
            ui.label(format!("Actual: {}", result.actual));
        }
    }

    fn open(owner: egui::Id, mapping: &Path) -> anyhow::Result<Self> {
        let mut window = Self {
            owner,
            mapping: mapping.to_owned(),
            store: Store::open(mapping)?,
            selected: 0,
            last_run: None,
            detail: None,
            draft: None,
            picker: None,
            notice: String::new(),
            reference_notice: String::new(),
        };
        window.select(0);
        Ok(window)
    }

    pub(super) fn editing(&self) -> bool {
        self.draft.is_some() || self.picker.is_some()
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.last_run = None;
        self.detail = None;
        self.notice.clear();
        self.reference_notice.clear();
        if let Some(set) = self.store.sets.get(index)
            && !set.reference.path.is_file()
        {
            self.reference_notice = "The original reference file is unavailable. The saved baseline and checks are still available.".into();
        }
    }

    fn act(&mut self, action: Action, app: &mut FerruleApp) -> Result<(), String> {
        match action {
            Action::None | Action::Close => {}
            Action::Select(index) => self.select(index),
            Action::New => {
                let (target, path, _) = app.selected_preview_table()?;
                let output = Output {
                    target: target.to_owned(),
                    artifact: path.to_owned(),
                };
                self.picker = Some((
                    output,
                    super::pick_file("Choose an independent known-good reference", &[]),
                ));
                self.notice.clear();
            }
            Action::Edit => {
                let set = self.selected_set()?.clone();
                self.draft = Some(Draft::edit(self.selected, set));
                self.notice.clear();
            }
            Action::LoadRows => {
                let draft = self.draft.as_mut().ok_or("No check set is being edited.")?;
                draft.set.selected(app.selected_preview_table()?)?;
                let (options, schema) = app.selected_preview_csv_boundary()?;
                let table = draft
                    .set
                    .reference
                    .rows_for_edit(options, schema)
                    .map_err(|e| format!("{e:#}"))?;
                draft.table = Some(table);
                self.notice.clear();
            }
            Action::Save => {
                let draft = self.draft.as_ref().ok_or("No check set is being edited.")?;
                let index = self
                    .store
                    .save(draft.index, draft.set.clone())
                    .map_err(|e| format!("{e:#}"))?;
                self.draft = None;
                self.select(index);
                self.notice = "Check set saved.".into();
            }
            Action::Cancel => {
                self.draft = None;
                self.picker = None;
                self.notice.clear();
            }
            Action::Undo => {
                self.store.undo_or_redo().map_err(|e| format!("{e:#}"))?;
                self.select(self.selected.min(self.store.sets.len().saturating_sub(1)));
            }
            Action::Run => {
                self.last_run = None;
                self.detail = None;
                self.notice.clear();
                self.last_run = Some(self.selected_set()?.run(app.selected_preview_table()?)?);
            }
            Action::Inspect(index) => {
                self.detail = Some(index);
                self.notice.clear();
                let set = self.selected_set()?;
                let run = self.last_run.as_ref().ok_or("Run this check set first.")?;
                // The actual target/path and complete current table are checked
                // synchronously. `field` borrows the assertion, not the preview.
                if let Some((row, field)) =
                    set.resolve_result(run, index, app.selected_preview_table()?)?
                    && !app.inspect_selected_preview_cell(row, field)
                {
                    return Err("The preview cell is unavailable; selection has not moved.".into());
                }
            }
        }
        Ok(())
    }

    fn selected_set(&self) -> Result<&CheckSet, String> {
        self.store
            .sets
            .get(self.selected)
            .ok_or_else(|| "Choose a saved check set.".into())
    }

    fn poll_reference(&mut self, app: &FerruleApp, ctx: &egui::Context) {
        let Some((_, rx)) = &self.picker else { return };
        let path = match rx.try_recv() {
            Ok(path) => path,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => None,
        };
        if let Some((output, _)) = self.picker.take()
            && let Some(path) = path
            && let Err(reason) = self.start_from_reference(app, output, Path::new(&path))
        {
            self.notice = reason;
        }
    }

    fn start_from_reference(
        &mut self,
        app: &FerruleApp,
        output: Output,
        path: &Path,
    ) -> Result<(), String> {
        let (target, artifact, _) = app.selected_preview_table()?;
        if !output.matches(target, artifact) {
            return Err("The selected preview output changed while choosing the reference.".into());
        }
        let (options, schema) = app.selected_preview_csv_boundary()?;
        store::require_independent_file(&self.mapping, &output, path)
            .map_err(|e| format!("{e:#}"))?;
        let (reference, table) = Reference::capture(path, options, schema).map_err(|e| {
            format!(
                "Could not load independent reference {}: {e:#}",
                path.display()
            )
        })?;
        self.draft = Some(Draft::new(output, reference, table));
        self.notice.clear();
        Ok(())
    }

    fn draw(&mut self, ctx: &egui::Context) -> Action {
        let editing = self.editing();
        let has_draft = self.draft.is_some();
        let kind = if editing {
            ui_kit::DialogKind::Modal
        } else {
            ui_kit::DialogKind::Docked
        };
        let response = ui_kit::dialog(
            ctx,
            kind,
            if editing {
                "Edit check set"
            } else {
                "Saved checks"
            },
        )
        .id_salt(("saved_checks", self.owner, editing))
        .width(ui_kit::DialogWidth::Medium)
        .show(
            |ui| {
                let action = if self.picker.is_some() {
                    ui.label("Choose a known-good file separate from the mapping output.");
                    if ui.button("Cancel selection").clicked() {
                        Action::Cancel
                    } else {
                        Action::None
                    }
                } else if let Some(draft) = &mut self.draft {
                    draft.draw(ui)
                } else {
                    self.show_sets(ui)
                };
                if !self.notice.is_empty() {
                    ui.label(&self.notice);
                }
                action
            },
            |ui| {
                if has_draft {
                    match ui_kit::dialog_buttons(
                        ui,
                        "Save check set".into(),
                        Some("Cancel".into()),
                        None,
                    ) {
                        Some(ui_kit::DialogAction::Primary) => Action::Save,
                        Some(ui_kit::DialogAction::Secondary) => Action::Cancel,
                        _ => Action::None,
                    }
                } else if ui.button("Close").clicked() {
                    Action::Close
                } else {
                    Action::None
                }
            },
        );
        if response.close_requested {
            if editing {
                Action::Cancel
            } else {
                Action::Close
            }
        } else if matches!(response.footer, Action::None) {
            response.body
        } else {
            response.footer
        }
    }

    fn show_sets(&self, ui: &mut egui::Ui) -> Action {
        let mut action = Action::None;
        ui.horizontal_wrapped(|ui| {
            if ui.button("New set from reference…").clicked() {
                action = Action::New;
            }
            let undo = if self.store.redo {
                "Redo last check save"
            } else {
                "Undo last check save"
            };
            if ui
                .add_enabled(self.store.can_undo_or_redo(), egui::Button::new(undo))
                .clicked()
            {
                action = Action::Undo;
            }
        });
        ui.weak(format!("Check file: {}", self.store.path.display()));
        let Ok(set) = self.selected_set() else {
            ui.label("No saved check sets. Select a complete CSV preview, then choose an independent reference.");
            return action;
        };
        egui::ComboBox::from_label("Check set")
            .selected_text(&set.name)
            .show_ui(ui, |ui| {
                for (index, item) in self.store.sets.iter().enumerate() {
                    if ui
                        .selectable_label(index == self.selected, &item.name)
                        .clicked()
                    {
                        action = Action::Select(index);
                    }
                }
            });
        ui.label(format!("Output: {}", set.output.label()));
        ui.label(format!(
            "Known-good reference: {}",
            set.reference.path.display()
        ));
        if !self.reference_notice.is_empty() {
            ui.weak(&self.reference_notice);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Run checks on selected preview").clicked() {
                action = Action::Run;
            }
            if ui.button("Edit set…").clicked() {
                action = Action::Edit;
            }
        });
        ui.separator();
        if let Some(run) = &self.last_run {
            let passed = run.results.iter().filter(|r| r.passed).count();
            ui.strong(format!(
                "Last check run: {passed} passed · {} failed",
                run.results.len() - passed
            ));
        } else {
            ui.strong("Saved expectations · not run");
        }
        for (index, NamedCheck { name, assertion }) in set.checks.iter().enumerate() {
            if let Some(result) = self
                .last_run
                .as_ref()
                .and_then(|run| run.results.get(index))
            {
                let status = if result.passed { "Pass" } else { "Fail" };
                if ui
                    .selectable_label(self.detail == Some(index), format!("{status} · {name}"))
                    .clicked()
                {
                    action = Action::Inspect(index);
                }
                if self.detail == Some(index) {
                    ui.label(format!("Expected: {}", assertion.expected_detail()));
                    ui.label(format!("Actual: {}", result.actual));
                }
            } else {
                ui.label(format!("{name}: {}", assertion.expected_detail()));
            }
        }
        action
    }
}

#[cfg(test)]
mod tests;
