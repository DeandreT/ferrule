//! A captured mapping preview can be inspected without touching either the
//! project or the runner. A pins one selected artifact; B is replaced only by
//! another completed preview. Both share their decoded cells through an Arc.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mapping::{Project, Scope};
use sha2::{Digest as _, Sha256};

use super::preview_service::PreviewInputSource;
use super::{FerruleApp, MappingDocument};
use crate::preview::{PreviewDraft, PreviewTarget};
use crate::preview_table::{CsvLayout, PreviewTable};
use crate::run_report::{
    MAX_PREVIEW_BYTES, OutputPreview, RunReportKind, RunReportView, TraceReport,
};

mod compare;
mod provenance;
mod scope;
#[cfg(test)]
mod tests;
mod view;

use compare::{Differences, MatchBy};
use scope::{RowMatch, RowScope};

struct Artifact {
    sha: String,
    csv: bool,
}

pub(super) struct Capture {
    target: PreviewTarget,
    fingerprint: String,
    input_path: PathBuf,
    input_sha: Option<String>,
    excerpt: String,
    input_truncated: bool,
    source: PreviewInputSource,
    documents: bool,
    scope: Result<RowScope, String>,
    scope_counts: Option<(usize, usize)>,
    layout: Result<CsvLayout, String>,
    trace: Option<provenance::Provenance>,
    artifacts: Vec<Artifact>,
}

fn target_parts<'a>(
    project: &'a Project,
    target: &PreviewTarget,
) -> Option<(&'a mapping::FormatOptions, &'a ir::SchemaNode, &'a Scope)> {
    match target {
        PreviewTarget::Primary => Some((&project.target_options, &project.target, &project.root)),
        PreviewTarget::Named(name) => named_target(project, name)
            .ok()
            .map(|(_, target)| (&target.options, &target.schema, &target.root)),
    }
}

fn named_target<'a>(
    project: &'a Project,
    name: &str,
) -> Result<(usize, &'a mapping::NamedTarget), String> {
    let mut matches = project
        .extra_targets
        .iter()
        .enumerate()
        .filter(|(_, target)| target.name == name);
    let target = matches
        .next()
        .ok_or_else(|| format!("No target named `{name}`."))?;
    if matches.next().is_some() {
        return Err(format!(
            "Ambiguous target `{name}`: more than one target has that name."
        ));
    }
    Ok(target)
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Capture {
    /// Runs on the completed preview worker while its input and serializer
    /// bytes are still owned there. File inputs are never reopened as evidence.
    pub(super) fn new(
        project: &Project,
        draft: &PreviewDraft,
        source: PreviewInputSource,
        outcome: &cli::PayloadRunOutcome,
        trace: &TraceReport,
    ) -> Option<Self> {
        let (options, schema, root) = target_parts(project, &draft.target)?;
        let input_path = PathBuf::from(draft.input_identity.trim());
        let input = (source == PreviewInputSource::EditorText).then_some(draft.input_text.as_str());
        let scope = match input {
            None => Err("Source bytes were not retained by file acquisition. Preview editor text to compare input records.".into()),
            Some(text) if cli::preview_reads_xml(&input_path, &project.source_options) =>
                RowScope::capture(text, project, root),
            Some(_) => Err("Input group expansion is available for a repeating XML input collection.".into()),
        };
        let excerpt = input.unwrap_or_default();
        let end = excerpt.floor_char_boundary(excerpt.len().min(MAX_PREVIEW_BYTES));
        Some(Self {
            target: draft.target.clone(),
            fingerprint: crate::layout_store::project_fingerprint(project),
            input_path,
            input_sha: input.map(|text| sha(text.as_bytes())),
            excerpt: excerpt[..end].to_owned(),
            input_truncated: end != excerpt.len(),
            source,
            documents: false,
            scope,
            scope_counts: provenance::scope_counts(&draft.target, trace),
            layout: CsvLayout::new(options, schema),
            trace: (outcome.artifacts.len() == 1)
                .then(|| provenance::Provenance::capture(project, root, &draft.target, trace))
                .flatten(),
            artifacts: outcome
                .artifacts
                .iter()
                .map(|artifact| Artifact {
                    sha: sha(&artifact.bytes),
                    csv: cli::preview_writes_csv(&artifact.path, options),
                })
                .collect(),
        })
    }

    pub(super) fn documents(
        project: &Project,
        input_path: &Path,
        outcome: &cli::PayloadRunOutcome,
    ) -> Self {
        // A filename document may use a raw CRLF writer. Its serializer is
        // separate from ordinary payload CSV; do not split it by extension.
        Self {
            target: PreviewTarget::Primary,
            fingerprint: crate::layout_store::project_fingerprint(project),
            input_path: input_path.to_owned(),
            input_sha: None,
            excerpt: String::new(),
            input_truncated: false,
            source: PreviewInputSource::ConfiguredFiles,
            documents: true,
            scope: Err("Document input bytes were not captured.".into()),
            scope_counts: None,
            layout: Err("Primary document output is displayed as a document.".into()),
            trace: None,
            artifacts: outcome
                .artifacts
                .iter()
                .map(|a| Artifact {
                    sha: sha(&a.bytes),
                    csv: false,
                })
                .collect(),
        }
    }
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Page {
    #[default]
    Inspect,
    Compare,
    Scope,
    Summary,
}

struct Pinned {
    target: PreviewTarget,
    path: PathBuf,
    input_path: PathBuf,
    input_sha: Option<String>,
    sha: String,
    table: Arc<PreviewTable>,
}

#[derive(Default)]
pub(super) struct Inspection {
    capture: Option<Capture>,
    table: Option<Result<Arc<PreviewTable>, String>>,
    output: Option<usize>,
    pinned: Option<Pinned>,
    last_draft: Option<PreviewDraft>,
    run_values: Vec<super::host_parameters::HostParameterEntry>,
    pub(super) attempt: Option<String>,
    pub(super) requested_field: Option<(PreviewTarget, String)>,
    page: Page,
    row: usize,
    column: usize,
    row_focus: bool,
    sample: usize,
    key: MatchBy,
    diff: Option<Result<Differences, String>>,
    change: usize,
    pair_open: bool,
    scope_keys: Vec<String>,
    matches: Option<Result<Vec<RowMatch>, String>>,
    group: usize,
    group_item: Option<usize>,
    expanded: Option<usize>,
    cell_trace: Option<((usize, usize), Option<String>)>,
    note: Option<String>,
}

impl Inspection {
    pub(super) fn restore_output_selection(
        &self,
        next: &mut RunReportView,
        previous: Option<&RunReportView>,
    ) {
        if let Some(previous) = previous.filter(|p| p.report.kind == RunReportKind::Preview)
            && let Some(old) = previous.report.outputs.get(previous.selected_output())
            && let Some(index) = next
                .report
                .outputs
                .iter()
                .position(|o| o.path == old.path && o.name == old.name)
        {
            next.select_output(index);
        }
    }

    pub(super) fn complete(
        &mut self,
        capture: Option<Capture>,
        report: &RunReportView,
        draft: Option<PreviewDraft>,
        run_values: Vec<super::host_parameters::HostParameterEntry>,
    ) {
        let same = self
            .capture
            .as_ref()
            .zip(capture.as_ref())
            .is_some_and(|(a, b)| a.target == b.target);
        self.capture = capture;
        self.last_draft = draft;
        self.run_values = run_values;
        self.attempt = None;
        self.output = None;
        self.table = None;
        self.note = None;
        if !same {
            self.scope_keys.clear();
            self.page = Page::Inspect;
        }
        self.select(report);
    }

    fn select(&mut self, report: &RunReportView) {
        let index = report.selected_output();
        if self.output == Some(index) {
            return;
        }
        self.output = Some(index);
        self.row = 0;
        self.column = 0;
        self.group = 0;
        self.group_item = None;
        self.expanded = None;
        self.sample = 0;
        self.change = 0;
        self.diff = None;
        self.matches = None;
        self.pair_open = false;
        self.cell_trace = None;
        self.note = None;
        self.table = self.capture.as_ref().and_then(|c| {
            let artifact = c.artifacts.get(index)?;
            let output = report.report.outputs.get(index)?;
            Some((|| {
                if !artifact.csv {
                    return Err("This output is a document or raw bytes; it has no CSV data rows.".into());
                }
                let text = match output.memory_preview() {
                    Some(OutputPreview::Text { content,truncated:false,.. }) => content,
                    Some(OutputPreview::Text { truncated:true,.. }) => return Err("CSV is truncated at the 1 MiB preview limit. Row inspection requires a complete artifact.".into()),
                    _ => return Err("CSV is not complete UTF-8 text. Inspect its raw preview.".into()),
                };
                let table = c.layout.as_ref().map_err(Clone::clone)?.decode(text.as_bytes())?;
                if table.rows.len() != output.records_written {
                    return Err("Decoded record count differs from the writer's count. Inspect the raw artifact.".into());
                }
                Ok(Arc::new(table))
            })())
        });
    }

    fn focus_cell(&mut self, row: usize, column: usize) {
        self.page = Page::Inspect;
        self.row = row;
        self.column = column;
        self.row_focus = true;
        self.note = None;
    }
}

impl FerruleApp {
    fn fresh_preview(&self) -> Result<(&Capture, &RunReportView, &PreviewTable), String> {
        let inspection = &self.preview_inspection;
        if let Some(reason) = &inspection.attempt {
            return Err(reason.clone());
        }
        let capture = inspection
            .capture
            .as_ref()
            .ok_or("No mapping preview has been captured.")?;
        let report = self
            .run_report
            .as_ref()
            .filter(|r| r.report.kind == RunReportKind::Preview)
            .ok_or("Select an in-memory mapping preview.")?;
        if !self.preview_current(capture) {
            return Err("Stale preview: refresh after changing the mapping.".into());
        }
        if inspection.output != Some(report.selected_output()) {
            return Err("Output selection has changed.".into());
        }
        let table = inspection
            .table
            .as_ref()
            .ok_or("No output artifact was produced.")?
            .as_deref()
            .map_err(Clone::clone)?;
        Ok((capture, report, table))
    }

    fn preview_current(&self, capture: &Capture) -> bool {
        // Direct edits in this frame may not have reached the history observer.
        // Exact navigation/check decisions compare live content; the passive
        // status badge uses the observer's cached fingerprint.
        self.preview_inputs_current(capture)
            && crate::layout_store::project_fingerprint(&self.project) == capture.fingerprint
    }

    fn preview_inputs_current(&self, capture: &Capture) -> bool {
        (capture.documents || self.preview_input_source == capture.source)
            && self.host_parameters.entries == self.preview_inspection.run_values
            && self.preview_draft.as_ref().is_none_or(|draft| {
                self.preview_inspection
                    .last_draft
                    .as_ref()
                    .is_some_and(|last| {
                        draft.target == last.target
                            && draft.input_identity == last.input_identity
                            && draft.output_identity == last.output_identity
                            && draft.input_text == last.input_text
                    })
            })
    }

    pub(crate) fn selected_preview_table(&self) -> Result<(&str, &Path, &PreviewTable), String> {
        let (capture, report, table) = self.fresh_preview()?;
        Ok((
            capture.target.label(),
            &report.report.outputs[report.selected_output()].path,
            table,
        ))
    }

    pub(crate) fn selected_preview_csv_boundary(
        &self,
    ) -> Result<(&mapping::FormatOptions, &ir::SchemaNode), String> {
        let (capture, _, _) = self.fresh_preview()?;
        let (options, schema, _) =
            target_parts(&self.project, &capture.target).ok_or("Target no longer exists.")?;
        Ok((options, schema))
    }

    pub(crate) fn inspect_selected_preview_cell(&mut self, row: usize, field: &str) -> bool {
        let Ok((_, _, table)) = self.selected_preview_table() else {
            return false;
        };
        if row >= table.rows.len() {
            return false;
        }
        let Ok(column) = table.column(field) else {
            return false;
        };
        self.preview_inspection.focus_cell(row, column);
        self.show_run_report = true;
        true
    }

    pub(super) fn focus_captured_field(&mut self, target: &PreviewTarget, field: &str) {
        let row = self.preview_inspection.row;
        let column = self.fresh_preview().and_then(|(capture, _, table)| {
            if &capture.target != target {
                return Err("The captured preview belongs to another target.".into());
            }
            if row >= table.rows.len() {
                return Err("The selected output has no such data row.".into());
            }
            table.column(field)
        });
        match column {
            Ok(column) => {
                self.preview_inspection.focus_cell(row, column);
                self.show_run_report = true;
            }
            Err(reason) => {
                self.status = format!("Cannot inspect `{field}`: {reason}");
                self.preview_inspection.note = Some(self.status.clone());
                self.preview_inspection.row_focus = false;
            }
        }
    }

    pub(crate) fn inspect_preview_field(&mut self, target: PreviewTarget, field: &str) {
        if self
            .preview_inspection
            .capture
            .as_ref()
            .is_some_and(|c| c.target == target)
            && self
                .run_report
                .as_ref()
                .is_some_and(|r| r.report.kind == RunReportKind::Preview)
        {
            self.focus_captured_field(&target, field);
            return;
        }
        if !self.can_preview() {
            return;
        }
        match &target {
            PreviewTarget::Primary => {
                self.mapping_workspace.active = MappingDocument::Main;
                self.mapping_workspace.focused = MappingDocument::Main;
            }
            PreviewTarget::Named(name) => {
                let index = match named_target(&self.project, name) {
                    Ok((index, _)) => index,
                    Err(reason) => {
                        self.status = reason;
                        return;
                    }
                };
                self.open_target_tab(index);
            }
        }
        self.begin_preview();
        self.preview_inspection.requested_field = Some((target, field.to_owned()));
    }
}
