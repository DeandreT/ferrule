//! Saved-project library generation through the public atomic writer.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use anyhow::{Context, bail};

use super::{DialogKind, FerruleApp, SaveContinuation};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LibraryLanguage {
    #[default]
    Rust,
    CSharp,
}

impl LibraryLanguage {
    fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::CSharp => "C#",
        }
    }
}

pub(super) struct LibraryGenerationDraft {
    pub(super) language: LibraryLanguage,
    pub(super) destination: String,
    pub(super) runtime_path: String,
    error: Option<String>,
}

impl LibraryGenerationDraft {
    fn new(project_path: &Path) -> Self {
        let name = project_path
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("mapping");
        let destination = format!("{name}-library");
        Self {
            language: LibraryLanguage::Rust,
            destination,
            runtime_path: String::new(),
            error: None,
        }
    }

    fn validate_settings_presence(&self) -> anyhow::Result<()> {
        if self.destination.trim().is_empty() {
            bail!("Enter a new folder for the generated library.");
        }
        if self.language == LibraryLanguage::Rust && self.runtime_path.trim().is_empty() {
            bail!("Choose the codegen-runtime folder for the Rust library.");
        }
        Ok(())
    }

    fn request(&self, project_path: &Path) -> anyhow::Result<LibraryGenerationRequest> {
        self.validate_settings_presence()?;
        let destination = setting_path(project_path, &self.destination);
        if destination.exists() {
            bail!(
                "Choose a new folder. {} already exists.",
                destination.display()
            );
        }
        if destination
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .is_none()
        {
            bail!("The new library folder needs a UTF-8 name.");
        }
        let target = match self.language {
            LibraryLanguage::Rust => {
                let runtime_path = setting_path(project_path, &self.runtime_path);
                if !runtime_path.is_dir() || !runtime_path.join("Cargo.toml").is_file() {
                    bail!("Choose the codegen-runtime folder containing Cargo.toml.");
                }
                cli::GenerateTarget::Rust { runtime_path }
            }
            LibraryLanguage::CSharp => cli::GenerateTarget::CSharp,
        };
        Ok(LibraryGenerationRequest {
            project_path: project_path.to_path_buf(),
            destination,
            target,
            language: self.language,
        })
    }
}

fn project_folder(project_path: &Path) -> &Path {
    project_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn setting_path(project_path: &Path, text: &str) -> PathBuf {
    let path = Path::new(text);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_folder(project_path).join(path)
    }
}

struct LibraryGenerationRequest {
    project_path: PathBuf,
    destination: PathBuf,
    target: cli::GenerateTarget,
    language: LibraryLanguage,
}

pub(super) struct PendingLibraryGeneration {
    receiver: Receiver<Result<cli::GenerateOutcome, String>>,
    language: LibraryLanguage,
}

fn pick_folder(current: &Path) -> Receiver<Option<String>> {
    let (sender, receiver) = mpsc::channel();
    let current = current.to_path_buf();
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new();
        if current.is_dir() {
            dialog = dialog.set_directory(current);
        }
        let result = dialog.pick_folder().map(|path| path.display().to_string());
        let _ = sender.send(result);
    });
    receiver
}

impl FerruleApp {
    pub(super) fn begin_library_generation(&mut self) {
        if self.rest_run_busy() {
            return;
        }
        if self.library_generation_draft.is_some() || self.pending_library_generation.is_some() {
            return;
        }
        self.library_generation_draft =
            Some(LibraryGenerationDraft::new(self.document.suggested_path()));
    }

    fn request_library_generation(&mut self, context: &egui::Context) {
        if self.rest_run_busy() {
            return;
        }
        if self.pending_dialog.is_some() || self.pending_library_generation.is_some() {
            return;
        }
        let Some(draft) = &self.library_generation_draft else {
            return;
        };
        let settings = match self.document.saved_path() {
            Some(saved) => draft.request(saved).map(|_| ()),
            // Save As chooses the folder used by relative settings. Check only
            // their presence until the project has actually been saved there.
            None => draft.validate_settings_presence(),
        };
        if let Err(error) = settings {
            self.library_generation_failed(format!("{error:#}"));
            return;
        }
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            if let Some(draft) = &mut self.library_generation_draft {
                draft.error =
                    Some("Fix the mapping issues below before generating a library.".into());
            }
            self.status = "library generation blocked by mapping issues".into();
            self.diagnostics.validation(&self.project, issues);
            return;
        }
        if let Some(draft) = &mut self.library_generation_draft {
            draft.error = None;
        }
        self.save_with_continuation(Some(SaveContinuation::GenerateLibrary), context);
    }

    pub(super) fn generate_saved_library(&mut self) {
        if self.pending_library_generation.is_some() {
            return;
        }
        let request = (|| -> anyhow::Result<LibraryGenerationRequest> {
            let saved = self
                .document
                .saved_path()
                .context("Save the mapping before generating a library.")?;
            self.library_generation_draft
                .as_ref()
                .context("Library generation settings are unavailable.")?
                .request(saved)
        })();
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                self.library_generation_failed(format!("{error:#}"));
                return;
            }
        };
        let language = request.language;
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("ferrule-library-generation".into())
            .spawn(move || {
                let result = cli::generate_project(
                    &request.project_path,
                    &request.destination,
                    request.target,
                )
                .map_err(|error| format!("{error:#}"));
                let _ = sender.send(result);
            });
        if let Err(error) = spawned {
            self.library_generation_failed(format!("Could not start library generation: {error}"));
            return;
        }
        self.pending_library_generation = Some(PendingLibraryGeneration { receiver, language });
        self.status = format!("generating {} library", language.label());
    }

    fn library_generation_failed(&mut self, message: String) {
        if let Some(draft) = &mut self.library_generation_draft {
            draft.error = Some(message.clone());
        }
        self.status = "library generation failed".into();
        self.diagnostics.error("Library generation failed", message);
    }

    pub(super) fn library_generation_save_failed(&mut self, error: &anyhow::Error) {
        if let Some(draft) = &mut self.library_generation_draft {
            draft.error = Some(format!("The mapping could not be saved: {error:#}"));
        }
    }

    pub(super) fn library_generation_save_cancelled(&mut self) {
        if self.pending_save_continuation == Some(SaveContinuation::GenerateLibrary) {
            self.status = "library generation cancelled before saving".into();
        }
    }

    pub(super) fn poll_library_generation(&mut self, context: &egui::Context) {
        let Some(pending) = &self.pending_library_generation else {
            return;
        };
        let language = pending.language;
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                context.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err(
                "Library generation stopped before reporting a result. Check the destination before retrying.".into(),
            ),
        };
        self.pending_library_generation = None;
        match result {
            Ok(outcome) => {
                self.library_generation_draft = None;
                self.status = format!(
                    "generated {} library in {} ({} files)",
                    language.label(),
                    outcome.output_directory.display(),
                    outcome.files_written,
                );
            }
            Err(message) => self.library_generation_failed(message),
        }
        if std::mem::take(&mut self.close_after_library_generation) {
            self.guard_app_close_requested(context, true);
            if self.pending_pipeline_editor_action.is_none()
                && self.pending_destructive_action.is_none()
                && self.pending_file_run.is_none()
                && self.pending_pipeline_run.is_none()
            {
                self.allow_close = true;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub(super) fn guard_library_generation_close_requested(
        &mut self,
        context: &egui::Context,
        close_requested: bool,
    ) -> bool {
        if close_requested && self.pending_library_generation.is_some() {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_library_generation = true;
            self.status = "finishing library generation before closing".into();
            true
        } else {
            false
        }
    }

    pub(super) fn stage_library_parent_folder(&mut self, parent: PathBuf) {
        if let Some(draft) = &mut self.library_generation_draft {
            let name = Path::new(&draft.destination)
                .file_name()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| std::ffi::OsStr::new("mapping-library"));
            draft.destination = parent.join(name).display().to_string();
            draft.error = None;
        }
    }

    pub(super) fn stage_library_runtime_folder(&mut self, path: String) {
        if let Some(draft) = &mut self.library_generation_draft {
            draft.runtime_path = path;
            draft.error = None;
        }
    }

    pub(super) fn show_library_generation(&mut self, context: &egui::Context) {
        let Some(mut draft) = self.library_generation_draft.take() else {
            return;
        };
        let running = self.pending_library_generation.is_some();
        let interactive = !running && self.pending_dialog.is_none();
        let mut open = true;
        let mut generate = false;
        let mut cancel = false;
        let mut window = egui::Window::new("Generate Library")
            .collapsible(false)
            .default_width(520.0);
        if interactive {
            window = window.open(&mut open);
        }
        window.show(context, |ui| {
            ui.label("Generate library source for the entire project. Changes are saved before generation.");
            ui.add_enabled_ui(interactive, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Language");
                    ui.selectable_value(&mut draft.language, LibraryLanguage::Rust, "Rust");
                    ui.selectable_value(&mut draft.language, LibraryLanguage::CSharp, "C#");
                });
                ui.label("New library folder");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.destination).desired_width(340.0));
                    if ui.button("Choose parent...").clicked() {
                        let destination =
                            setting_path(self.document.suggested_path(), &draft.destination);
                        self.pending_dialog = Some((
                            DialogKind::BrowseLibraryParent,
                            pick_folder(project_folder(&destination)),
                        ));
                    }
                });
                ui.weak("Use a new folder name. Existing folders are kept unchanged.");
                if draft.language == LibraryLanguage::Rust {
                    ui.label("Rust runtime folder");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.runtime_path)
                                .desired_width(340.0),
                        );
                        if ui.button("Choose runtime...").clicked() {
                            let current =
                                setting_path(self.document.suggested_path(), &draft.runtime_path);
                            self.pending_dialog =
                                Some((DialogKind::BrowseLibraryRuntime, pick_folder(&current)));
                        }
                    });
                    ui.weak("Choose ferrule's codegen-runtime folder containing Cargo.toml.");
                }
                ui.weak("Relative paths use the saved mapping's folder.");
            });
            if let Some(error) = &draft.error {
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.colored_label(self.palette.error, error);
                    });
            }
            ui.separator();
            if running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Generating library. This window can close when generation finishes.");
                });
            } else {
                ui.horizontal(|ui| {
                    generate = ui
                        .add_enabled(interactive, egui::Button::new("Save and generate"))
                        .clicked();
                    cancel = ui
                        .add_enabled(interactive, egui::Button::new("Cancel"))
                        .clicked();
                });
            }
        });
        if running || (open && !cancel) {
            self.library_generation_draft = Some(draft);
        }
        if generate {
            self.request_library_generation(context);
        }
    }
}

#[cfg(test)]
mod tests;
