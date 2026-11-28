//! The Run command reviews the saved design and host values as one immutable
//! request. Destination edits belong to this request, not to the mapping.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use cli::{InputRole, PlannedAcquisition, PlannedPublication};

use super::FerruleApp;
use super::route_preflight::SAVED_RUN_OBSERVERS;
use super::run_ui::nonempty_path;
use crate::preview::PreviewTarget;
use crate::project_state::project_snapshot_key;
use crate::ui_kit;

fn unspecified(path: &Option<String>) -> bool {
    path.as_deref().is_none_or(|text| text.trim().is_empty())
}

/// A user can supply missing static filenames in the review. Let the route
/// planner enumerate the selected reads and writes using the project folder
/// for those unfilled slots. The folder is never accepted as a filename.
pub(super) fn plan_review(
    project: &mapping::Project,
    path: &Path,
    request: cli::RunRequest<'_>,
) -> anyhow::Result<cli::RoutePlan> {
    let mut planning = std::borrow::Cow::Borrowed(project);
    if unspecified(&project.target_path) && !project.root.produces_documents() {
        planning.to_mut().target_path = Some(".".into());
    }
    for (index, target) in project.extra_targets.iter().enumerate() {
        if unspecified(&target.path) && !target.root.produces_documents() {
            planning.to_mut().extra_targets[index].path = Some(".".into());
        }
    }
    cli::plan_file_run(&planning, path, request)
}

pub(super) struct ReviewedRun {
    project: mapping::Project,
    project_path: PathBuf,
    document: Option<(mapping::PrimarySourcePlan, cli::PrimaryDocumentPolicy)>,
    input_path: Option<PathBuf>,
    output_path: Option<PathBuf>,
    selection: Option<PreviewTarget>,
    pub(super) parameters: engine::RuntimeParameters,
}

impl ReviewedRun {
    pub(super) fn execute<'a>(
        &'a self,
        mut options: cli::RunOptions<'a>,
        reader: Option<&dyn cli::FilesystemPdfReader>,
        before_publish: &dyn Fn() -> bool,
    ) -> anyhow::Result<cli::RunOutcome> {
        if let Some((plan, policy)) = &self.document {
            return cli::run_primary_document_create_new(
                &mapping::DocumentProject::new(self.project.clone(), plan.clone()),
                &self.project_path,
                self.output_path
                    .as_deref()
                    .context("missing reviewed output")?,
                policy,
                Some(&self.parameters),
                before_publish,
            );
        }
        options.input_path = self.input_path.as_deref();
        options.output_path = self.output_path.as_deref();
        options.target = self.selection.as_ref().map(PreviewTarget::selection);
        cli::PreparedFileRun::prepare(&self.project, &self.project_path, options.request())?
            .execute_create_new(&options.observers(), reader)
    }
}

struct Input {
    label: String,
    path: Option<PathBuf>,
}

struct Destination {
    target: PreviewTarget,
    folder: PathBuf,
    /// For static files: relative to the reviewed folder, or an absolute path.
    /// None denotes a mapped document set; only its base is known yet.
    filename: Option<String>,
    problem: Option<String>,
}

impl Destination {
    fn unfilled(target: PreviewTarget, folder: &Path) -> Self {
        Self {
            target,
            folder: folder.into(),
            filename: Some(String::new()),
            problem: None,
        }
    }

    fn new(target: PreviewTarget, path: &Path, documents: bool) -> anyhow::Result<Self> {
        let path = std::path::absolute(path)?;
        let (folder, filename) = if documents {
            (path, None)
        } else {
            let filename = path.file_name().map_or(Ok(""), |name| {
                name.to_str().context("output filename is not Unicode")
            })?;
            (
                path.parent().unwrap_or(&path).to_owned(),
                Some(filename.to_owned()),
            )
        };
        Ok(Self {
            target,
            folder,
            filename,
            problem: None,
        })
    }

    fn path(&self) -> PathBuf {
        match &self.filename {
            Some(filename) => self.folder.join(filename),
            None => self.folder.clone(),
        }
    }

    fn inspect(&self, inputs: &[Input], project: &Path) -> anyhow::Result<()> {
        let Some(filename) = &self.filename else {
            return writable_folder(&self.folder);
        };
        ensure!(!filename.trim().is_empty(), "Enter an output filename.");
        ensure!(!filename.contains("://"), "Choose a local file.");
        ensure!(
            !filename.ends_with(std::path::is_separator),
            "Enter a filename, not a folder."
        );
        let path = self.path();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .context("Enter a filename.")?;
        ensure!(
            !name.chars().any(char::is_control),
            "Invalid output filename."
        );
        #[cfg(windows)]
        validate_windows_filename(name)?;
        let parent = path.parent().context("Choose an output folder.")?;
        writable_folder(parent)?;
        let identity = file_identity(&path);
        ensure!(
            identity != file_identity(project),
            "This is the saved project. Choose another name."
        );
        ensure!(
            !inputs
                .iter()
                .filter_map(|input| input.path.as_deref())
                .any(|p| file_identity(p) == identity),
            "This path is an input. Choose another name."
        );
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => bail!("This is a directory. Choose a filename."),
            Ok(_) => bail!("This name is occupied. Choose another filename."),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("Cannot check {}", path.display())),
        }
    }
}

fn file_identity(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).unwrap_or_else(|_| {
        match (
            path.parent().and_then(|p| fs::canonicalize(p).ok()),
            path.file_name(),
        ) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => path.to_owned(),
        }
    });
    #[cfg(windows)]
    let path = {
        let mut spelling = path.into_os_string();
        spelling.make_ascii_lowercase();
        PathBuf::from(spelling)
    };
    path
}

#[cfg(windows)]
fn validate_windows_filename(name: &str) -> anyhow::Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    let numbered_device = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    ensure!(
        !name.contains(['<', '>', ':', '"', '|', '?', '*'])
            && !name.ends_with([' ', '.'])
            && !numbered_device
            && !matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ),
        "Invalid Windows filename."
    );
    Ok(())
}

fn writable_folder(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Cannot access output folder {}", path.display()))?;
    ensure!(
        metadata.is_dir(),
        "{} is not an output directory.",
        path.display()
    );
    #[cfg(unix)]
    {
        use rustix::fs::{Access, AtFlags, CWD, accessat};
        accessat(
            CWD,
            path,
            Access::WRITE_OK | Access::EXEC_OK,
            AtFlags::EACCESS,
        )
        .with_context(|| format!("Cannot create files in {}", path.display()))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Request add-file and add-subdirectory (for staging) rights on the
        // existing directory. This does not create a probe or reserve a name.
        const ADD_FILE_AND_DIRECTORY: u32 = 0x0002 | 0x0004;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        fs::OpenOptions::new()
            .access_mode(ADD_FILE_AND_DIRECTORY)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .with_context(|| format!("Cannot create files in {}", path.display()))?;
    }
    Ok(())
}

pub(super) struct DestinationReview {
    run: ReviewedRun,
    document_id: egui::Id,
    inputs: Vec<Input>,
    outputs: Vec<Destination>,
    checked: bool,
    plan_error: Option<String>,
}

impl DestinationReview {
    fn new(app: &FerruleApp) -> anyhow::Result<Self> {
        let project_path = std::path::absolute(
            app.document
                .saved_path()
                .context("Save the project before Run.")?,
        )?;
        let document = app
            .primary_document
            .as_ref()
            .map(|session| Ok::<_, anyhow::Error>((session.plan.clone(), session.policy()?)))
            .transpose()?;
        let run = ReviewedRun {
            project: app.project.clone(),
            project_path,
            document,
            input_path: nonempty_path(&app.input_path),
            output_path: nonempty_path(&app.output_path),
            selection: (app.project.extra_targets.is_empty()).then_some(PreviewTarget::Primary),
            parameters: app.host_parameters.compile().map_err(anyhow::Error::msg)?,
        };
        let mut review = Self {
            run,
            document_id: app.document_ui_id,
            inputs: Vec::new(),
            outputs: Vec::new(),
            checked: false,
            plan_error: None,
        };
        review.refresh_plan();
        Ok(review)
    }

    fn refresh_plan(&mut self) {
        self.inputs.clear();
        self.outputs.clear();
        self.plan_error = self.read_plan().err().map(|e| format!("{e:#}"));
        self.recheck();
    }

    fn read_plan(&mut self) -> anyhow::Result<()> {
        let run = &self.run;
        let project_folder = run
            .project_path
            .parent()
            .context("Missing project folder")?;
        if let Some((plan, policy)) = &run.document {
            let document = mapping::DocumentProject::new(run.project.clone(), plan.clone());
            let ctx =
                engine::ExecutionContext::new(&run.project_path).with_parameters(&run.parameters);
            let filename = engine::evaluate_primary_filename(&document, &ctx)?;
            let path = std::path::absolute(&policy.input_root)?.join(filename);
            let label = match plan.selection {
                mapping::PrimaryFileSelection::Direct => "Primary input",
                mapping::PrimaryFileSelection::FileNameWildcard { .. } => {
                    "Primary input pattern (selected at Run)"
                }
            };
            self.inputs.push(Input {
                label: label.into(),
                path: Some(path),
            });
            let output = match (&run.output_path, &run.project.target_path) {
                (Some(path), _) => Destination::new(PreviewTarget::Primary, path, false)?,
                (None, Some(path)) if !path.trim().is_empty() => {
                    Destination::new(PreviewTarget::Primary, &project_folder.join(path), false)?
                }
                _ => Destination::unfilled(PreviewTarget::Primary, project_folder),
            };
            self.outputs.push(output);
            return Ok(());
        }
        let primary_override = match run.selection {
            Some(PreviewTarget::Named(_)) => None,
            _ => run.output_path.as_deref(),
        };
        let request = cli::RunRequest {
            input_path: run.input_path.as_deref(),
            output_path: primary_override,
            target: run.selection.as_ref().map(PreviewTarget::selection),
            runtime_parameters: Some(&run.parameters),
            observers: SAVED_RUN_OBSERVERS,
            ..Default::default()
        };
        let plan = plan_review(&run.project, &run.project_path, request)?;
        for input in plan.acquisitions() {
            match input {
                PlannedAcquisition::Design(_) => {}
                PlannedAcquisition::Location { role, path, .. } => {
                    let label = match role {
                        InputRole::Primary => "Primary input".into(),
                        InputRole::Named(name) => format!("Input: {name}"),
                        InputRole::PipelineHost(_) => bail!("Unexpected pipeline input"),
                    };
                    let remote = path.to_str().and_then(|p| p.split_once("://")).is_some_and(
                        |(scheme, _)| {
                            scheme.eq_ignore_ascii_case("http")
                                || scheme.eq_ignore_ascii_case("https")
                        },
                    );
                    let path = if remote {
                        path.clone()
                    } else {
                        std::path::absolute(path)?
                    };
                    self.inputs.push(Input {
                        label,
                        path: Some(path),
                    });
                }
                PlannedAcquisition::Dynamic { name } => self.inputs.push(Input {
                    label: format!("Input: {name} (resolved during Run)"),
                    path: None,
                }),
                _ => bail!("Unexpected input in a saved-file run"),
            }
        }
        // RoutePlan lists all selected publications in order, primary first
        // for All. A primary schema name can also be a named target's label.
        for (index, output) in plan.publications().iter().enumerate() {
            let (name, path, documents) = match output {
                PlannedPublication::File { target, path } => (target, path, false),
                PlannedPublication::Documents { target, base } => (target, base, true),
                _ => bail!("Unexpected publication in a saved-file run"),
            };
            let target = run.selection.clone().unwrap_or_else(|| {
                if index == 0 {
                    PreviewTarget::Primary
                } else {
                    PreviewTarget::Named(name.clone())
                }
            });
            let missing = match &target {
                PreviewTarget::Primary => {
                    primary_override.is_none() && unspecified(&run.project.target_path)
                }
                PreviewTarget::Named(name) => run
                    .project
                    .extra_targets
                    .iter()
                    .find(|t| t.name == *name)
                    .is_some_and(|t| unspecified(&t.path)),
            };
            self.outputs.push(if !documents && missing {
                Destination::unfilled(target, project_folder)
            } else {
                Destination::new(target, path, documents)?
            });
        }
        Ok(())
    }

    fn recheck(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for output in &mut self.outputs {
            output.problem = output
                .inspect(&self.inputs, &self.run.project_path)
                .err()
                .map(|error| format!("{error:#}"));
            if output.problem.is_none()
                && output.filename.is_some()
                && !seen.insert(file_identity(&output.path()))
            {
                output.problem = Some("Another selected output uses this path.".into());
            }
        }
        self.checked = true;
    }

    fn ready(&self) -> bool {
        self.checked
            && self.plan_error.is_none()
            && !self.outputs.is_empty()
            && self.outputs.iter().all(|output| output.problem.is_none())
    }

    fn into_run(mut self) -> anyhow::Result<ReviewedRun> {
        ensure!(self.ready(), "Apply and recheck the selected destinations.");
        let single = self.run.selection.is_some();
        for output in &self.outputs {
            let path = output.path();
            match &output.target {
                PreviewTarget::Primary => self.run.output_path = Some(path),
                PreviewTarget::Named(_) if single => self.run.output_path = Some(path),
                PreviewTarget::Named(name) if output.filename.is_some() => {
                    let target = self
                        .run
                        .project
                        .extra_targets
                        .iter_mut()
                        .find(|t| t.name == *name)
                        .context("Reviewed target is missing")?;
                    target.path = Some(
                        path.to_str()
                            .context("Output path is not Unicode")?
                            .to_owned(),
                    );
                }
                // All uses each named document target's original mapped base.
                PreviewTarget::Named(_) => {}
            }
        }
        Ok(self.run)
    }
}

impl FerruleApp {
    pub(super) fn review_run_destinations(&mut self) {
        match DestinationReview::new(self) {
            Ok(review) => {
                self.run_destinations = Some(review);
                self.status = "review run destinations".into();
            }
            Err(error) => {
                self.status = "run blocked".into();
                self.diagnostics.error("Run blocked", format!("{error:#}"));
            }
        }
    }

    pub(super) fn confirm_run_destinations(&mut self) {
        let Some(review) = &mut self.run_destinations else {
            return;
        };
        if !review.ready() {
            return;
        }
        if review.document_id != self.document_ui_id
            || self
                .document
                .saved_path()
                .and_then(|p| std::path::absolute(p).ok())
                .as_ref()
                != Some(&review.run.project_path)
            || project_snapshot_key(&self.project) != project_snapshot_key(&review.run.project)
        {
            self.run_destinations = None;
            self.status = "run blocked".into();
            self.diagnostics.error(
                "Run blocked",
                "The mapping changed. Review its destinations again.",
            );
            return;
        }
        let review = self.run_destinations.as_mut().expect("open review");
        review.recheck();
        if !review.ready() {
            return;
        }
        let review = self.run_destinations.take().expect("checked review");
        match review.into_run() {
            Ok(run) => self.launch_file_run(Some(run)),
            Err(error) => {
                self.status = "run blocked".into();
                self.diagnostics.error("Run blocked", format!("{error:#}"));
            }
        }
    }

    pub(super) fn show_run_destinations(&mut self, ctx: &egui::Context) {
        let Some(review) = &mut self.run_destinations else {
            return;
        };
        // The footer can enqueue Run; the handler also checks the live draft
        // after any text or target change in this same frame.
        let ready = review.ready();
        let response = ui_kit::dialog(ctx, ui_kit::DialogKind::Modal, "Review run destinations")
            .width(ui_kit::DialogWidth::Wide)
            .show(
                |ui| {
                    if review.run.document.is_none() && !review.run.project.extra_targets.is_empty()
                    {
                        let previous = review.run.selection.clone();
                        egui::ComboBox::from_label("Outputs to run")
                            .selected_text(
                                previous
                                    .as_ref()
                                    .map_or("All targets", PreviewTarget::label),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut review.run.selection, None, "All targets");
                                ui.selectable_value(
                                    &mut review.run.selection,
                                    Some(PreviewTarget::Primary),
                                    "Primary",
                                );
                                for target in &review.run.project.extra_targets {
                                    ui.selectable_value(
                                        &mut review.run.selection,
                                        Some(PreviewTarget::Named(target.name.clone())),
                                        format!("Named: {}", target.name),
                                    );
                                }
                            });
                        if previous != review.run.selection {
                            review.refresh_plan();
                        }
                    }
                    ui_kit::section(ui, "Inputs", |ui| {
                        for input in &review.inputs {
                            ui.strong(&input.label);
                            if let Some(path) = &input.path {
                                ui.monospace(path.display().to_string());
                            }
                        }
                    });
                    ui_kit::section(ui, "Selected outputs", |ui| {
                        for output in &mut review.outputs {
                            let label = match &output.target {
                                PreviewTarget::Primary => "Primary output".into(),
                                PreviewTarget::Named(name) => format!("Named output: {name}"),
                            };
                            ui.strong(label);
                            if let Some(name) = &mut output.filename {
                                let label = ui.label("Filename or path");
                                let edit = ui.text_edit_singleline(name).labelled_by(label.id);
                                if edit.changed() {
                                    review.checked = false;
                                }
                            } else {
                                ui.label(
                                    "Document filenames are mapped during Run beneath this folder.",
                                );
                            }
                            ui.monospace(output.path().display().to_string());
                            if !review.checked {
                                ui.weak("Apply and recheck this path.");
                            } else if let Some(problem) = &output.problem {
                                ui.colored_label(ui.visuals().error_fg_color, problem);
                            } else if output.filename.is_some() {
                                ui.label("Available. Run will only create a new file.");
                            }
                        }
                        if let Some(error) = &review.plan_error {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        if ui.button("Apply and recheck").clicked() {
                            review.recheck();
                        }
                    });
                },
                |ui| {
                    ui_kit::dialog_buttons(
                        ui,
                        ui_kit::DialogButton::new("Run")
                            .disabled_if(!ready, "Apply and recheck a free destination first."),
                        Some("Cancel".into()),
                        None,
                    )
                },
            );
        if response.close_requested || response.footer == Some(ui_kit::DialogAction::Secondary) {
            self.run_destinations = None;
            self.status = "run cancelled".into();
        } else if response.footer == Some(ui_kit::DialogAction::Primary) {
            self.confirm_run_destinations();
        }
    }
}

#[cfg(test)]
#[path = "run_destinations_tests.rs"]
mod tests;
