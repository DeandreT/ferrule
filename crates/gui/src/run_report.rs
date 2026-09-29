//! Last-run summary and bounded output previews for the native GUI.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod replay;
mod source_rows;

use replay::show_replay;
#[cfg(test)]
use source_rows::source_row_summary;
use source_rows::{format_source_row, index_source_rows, show_source_rows, source_row_details};

pub const MAX_PREVIEW_BYTES: usize = 1024 * 1024;
const MAX_BINARY_PREVIEW_BYTES: usize = 4 * 1024;
pub const MAX_TRACE_EVENTS: usize = 50_000;

#[derive(Debug, Default)]
pub struct TraceReport {
    pub events: Vec<cli::TraceEvent>,
    pub dropped: usize,
    /// Empty for single-project runs; pipeline events reference these stage IDs.
    pub stages: Vec<String>,
    pub event_stages: Vec<u16>,
}

impl TraceReport {
    fn stage_at(&self, index: usize) -> Option<&str> {
        let stage = usize::from(*self.event_stages.get(index)?);
        self.stages.get(stage).map(String::as_str)
    }
}

/// Bounded synchronous trace collector used by native runs.
pub struct TraceCollector {
    events: RefCell<Vec<cli::TraceEvent>>,
    dropped: Cell<usize>,
    limit: usize,
}

impl TraceCollector {
    pub fn new() -> Self {
        Self::with_limit(MAX_TRACE_EVENTS)
    }

    fn with_limit(limit: usize) -> Self {
        Self {
            events: RefCell::new(Vec::with_capacity(limit.min(1024))),
            dropped: Cell::new(0),
            limit,
        }
    }

    pub fn finish(self) -> TraceReport {
        TraceReport {
            events: self.events.into_inner(),
            dropped: self.dropped.get(),
            stages: Vec::new(),
            event_stages: Vec::new(),
        }
    }
}

#[derive(Default)]
struct PipelineTraceState {
    events: Vec<cli::TraceEvent>,
    event_stages: Vec<u16>,
    stages: Vec<String>,
    stage_indices: BTreeMap<String, u16>,
    dropped: usize,
}

/// One global retained prefix across all stages, with each stage ID stored once.
pub struct PipelineTraceCollector {
    state: RefCell<PipelineTraceState>,
    limit: usize,
}

impl PipelineTraceCollector {
    pub fn new() -> Self {
        Self::with_limit(MAX_TRACE_EVENTS)
    }

    fn with_limit(limit: usize) -> Self {
        Self {
            state: RefCell::new(PipelineTraceState::default()),
            limit,
        }
    }

    pub fn record(&self, stage: &str, event: cli::TraceEvent) {
        let mut state = self.state.borrow_mut();
        if state.events.len() >= self.limit {
            state.dropped = state.dropped.saturating_add(1);
            return;
        }
        let stage_index = match state.stage_indices.get(stage).copied() {
            Some(index) => index,
            None => {
                // Pipeline validation limits stages to 1,024 and IDs to 256 bytes.
                let Ok(index) = u16::try_from(state.stages.len()) else {
                    state.dropped = state.dropped.saturating_add(1);
                    return;
                };
                state.stages.push(stage.to_owned());
                state.stage_indices.insert(stage.to_owned(), index);
                index
            }
        };
        state.events.push(event);
        state.event_stages.push(stage_index);
    }

    pub fn finish(self) -> TraceReport {
        let state = self.state.into_inner();
        TraceReport {
            events: state.events,
            dropped: state.dropped,
            stages: state.stages,
            event_stages: state.event_stages,
        }
    }
}

impl Default for PipelineTraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl cli::TraceSink for TraceCollector {
    fn record(&self, event: cli::TraceEvent) {
        let mut events = self.events.borrow_mut();
        if events.len() < self.limit {
            events.push(event);
        } else {
            self.dropped.set(self.dropped.get().saturating_add(1));
        }
    }
}

#[derive(Debug)]
pub struct RunReport {
    pub kind: RunReportKind,
    pub duration: Duration,
    pub records_written: usize,
    pub input_path: PathBuf,
    pub outputs: Vec<RunOutput>,
    pub trace: TraceReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunReportKind {
    Run,
    Preview,
    Pipeline,
}

impl RunReport {
    pub fn from_pipeline_outcome(
        outcome: cli::PipelineRunOutcome,
        pipeline_path: PathBuf,
        duration: Duration,
        trace: TraceReport,
    ) -> Self {
        let records_written = outcome.artifacts.iter().fold(0usize, |sum, artifact| {
            sum.saturating_add(artifact.records_written)
        });
        let mut target_counts = std::collections::BTreeMap::new();
        for artifact in &outcome.artifacts {
            *target_counts
                .entry((artifact.stage.clone(), artifact.target.clone()))
                .or_insert(0usize) += 1;
        }
        let outputs = outcome
            .artifacts
            .into_iter()
            .map(|artifact| {
                let target = artifact.target.as_deref().unwrap_or("Primary");
                let name = if target_counts
                    .get(&(artifact.stage.clone(), artifact.target.clone()))
                    .is_some_and(|count| *count > 1)
                {
                    format!(
                        "{} / {target} - {}",
                        artifact.stage,
                        artifact.path.display()
                    )
                } else {
                    format!("{} / {target}", artifact.stage)
                };
                RunOutput::new(name, artifact.records_written, artifact.path)
            })
            .collect();
        Self {
            kind: RunReportKind::Pipeline,
            duration,
            records_written,
            input_path: pipeline_path,
            outputs,
            trace,
        }
    }

    pub fn from_outcome_with_trace(
        outcome: cli::RunOutcome,
        duration: Duration,
        trace: TraceReport,
    ) -> Self {
        let outputs = outcome
            .artifacts
            .iter()
            .map(RunOutput::from_written)
            .collect();
        Self {
            kind: RunReportKind::Run,
            duration,
            records_written: outcome.records_written,
            input_path: outcome.input_path,
            outputs,
            trace,
        }
    }

    pub fn from_payload_with_trace(
        outcome: cli::PayloadRunOutcome,
        input_path: PathBuf,
        duration: Duration,
        trace: TraceReport,
    ) -> Self {
        let mut target_counts = std::collections::BTreeMap::new();
        for artifact in &outcome.artifacts {
            let count = target_counts
                .entry(artifact.target.as_str())
                .or_insert(0usize);
            *count = count.saturating_add(1);
        }
        let outputs = outcome
            .artifacts
            .iter()
            .map(|output| {
                RunOutput::from_payload(
                    output,
                    target_counts
                        .get(output.target.as_str())
                        .is_some_and(|count| *count > 1),
                )
            })
            .collect();
        Self {
            kind: RunReportKind::Preview,
            duration,
            records_written: outcome.records_written,
            input_path,
            outputs,
            trace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportPage {
    Output,
    Trace,
    History,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryMode {
    Nodes,
    SourceRows,
}

#[derive(Debug)]
pub struct RunReportView {
    pub report: RunReport,
    selected_output: usize,
    page: ReportPage,
    trace_filter: String,
    history_mode: HistoryMode,
    history_by_node: BTreeMap<mapping::NodeId, Vec<usize>>,
    history_node: Option<mapping::NodeId>,
    history_stage: Option<u16>,
    source_rows: Vec<usize>,
    selected_source_row: Option<usize>,
    replay_event: Option<usize>,
}

impl RunReportView {
    pub fn new(report: RunReport) -> Self {
        let replay_event = (!report.trace.events.is_empty()).then_some(0);
        let history_stage = report.trace.event_stages.first().copied();
        let history_by_node = index_node_history_for_stage(&report.trace, history_stage);
        let source_rows = index_source_rows(&report.trace.events);
        let selected_source_row = source_rows.first().copied();
        let history_node = first_node_in_stage(&report.trace, history_stage);
        Self {
            report,
            selected_output: 0,
            page: ReportPage::Output,
            trace_filter: String::new(),
            history_mode: HistoryMode::Nodes,
            history_by_node,
            history_node,
            history_stage,
            source_rows,
            selected_source_row,
            replay_event,
        }
    }

    #[cfg(test)]
    pub fn selected_output(&self) -> usize {
        self.selected_output
    }

    /// Trace indices are stable only within the retained prefix of this report.
    fn replay_from(&mut self, index: usize) -> bool {
        if index >= self.report.trace.events.len() {
            return false;
        }
        self.replay_event = Some(index);
        self.page = ReportPage::Replay;
        true
    }

    fn filtered_trace_indices(&self) -> Vec<usize> {
        let filter = self.trace_filter.trim().to_lowercase();
        self.report
            .trace
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                (filter.is_empty()
                    || self
                        .trace_row(index, event)
                        .to_lowercase()
                        .contains(&filter))
                .then_some(index)
            })
            .collect()
    }

    fn trace_row(&self, index: usize, event: &cli::TraceEvent) -> String {
        let row = trace_row(index, event);
        match self.report.trace.stage_at(index) {
            Some(stage) => format!("stage {stage}  {row}"),
            None => row,
        }
    }

    fn select_history_stage(&mut self, stage: u16) {
        if usize::from(stage) >= self.report.trace.stages.len() {
            return;
        }
        self.history_stage = Some(stage);
        if let Some(index) = self
            .report
            .trace
            .event_stages
            .iter()
            .position(|&event_stage| event_stage == stage)
        {
            self.replay_event = Some(index);
        }
        self.history_by_node = index_node_history_for_stage(&self.report.trace, Some(stage));
        self.history_node = first_node_in_stage(&self.report.trace, Some(stage));
        self.selected_source_row = self
            .source_rows
            .iter()
            .copied()
            .find(|&index| self.report.trace.event_stages.get(index) == Some(&stage));
    }
}

fn first_node_in_stage(trace: &TraceReport, stage: Option<u16>) -> Option<mapping::NodeId> {
    trace.events.iter().enumerate().find_map(|(index, event)| {
        if stage.is_some_and(|stage| trace.event_stages.get(index) != Some(&stage)) {
            return None;
        }
        match event {
            cli::TraceEvent::NodeValue { node, .. } => Some(*node),
            cli::TraceEvent::NodeInputValue { consumer, .. } => Some(*consumer),
            _ => None,
        }
    })
}

fn index_node_history_for_stage(
    trace: &TraceReport,
    stage: Option<u16>,
) -> BTreeMap<mapping::NodeId, Vec<usize>> {
    if stage.is_none() {
        return index_node_history(&trace.events);
    }
    let mut history = BTreeMap::<mapping::NodeId, Vec<usize>>::new();
    for (index, event) in trace.events.iter().enumerate() {
        if trace.event_stages.get(index) != stage.as_ref() {
            continue;
        }
        match event {
            cli::TraceEvent::NodeValue { node, .. } => {
                history.entry(*node).or_default().push(index);
            }
            cli::TraceEvent::NodeInputValue { consumer, .. } => {
                history.entry(*consumer).or_default().push(index);
            }
            _ => {}
        }
    }
    history
}

fn index_node_history(events: &[cli::TraceEvent]) -> BTreeMap<mapping::NodeId, Vec<usize>> {
    let mut history = BTreeMap::<mapping::NodeId, Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        match event {
            cli::TraceEvent::NodeValue { node, .. } => {
                history.entry(*node).or_default().push(index);
            }
            cli::TraceEvent::NodeInputValue { consumer, .. } => {
                history.entry(*consumer).or_default().push(index);
            }
            _ => {}
        }
    }
    history
}

#[derive(Debug)]
pub struct RunOutput {
    pub name: String,
    pub records_written: usize,
    pub path: PathBuf,
    in_memory: bool,
    preview: Option<OutputPreview>,
}

impl RunOutput {
    fn from_written(output: &cli::WrittenOutput) -> Self {
        Self::new(
            output.name.clone(),
            output.records_written,
            output.path.clone(),
        )
    }

    fn from_payload(output: &cli::PayloadArtifact, distinguish_path: bool) -> Self {
        let name = if distinguish_path {
            format!("{} - {}", output.target, output.path.display())
        } else {
            output.target.clone()
        };
        Self {
            name,
            records_written: output.records_written,
            path: output.path.clone(),
            in_memory: true,
            preview: Some(OutputPreview::from_bytes(&output.bytes)),
        }
    }

    fn new(name: String, records_written: usize, path: PathBuf) -> Self {
        Self {
            name,
            records_written,
            path,
            in_memory: false,
            preview: None,
        }
    }

    fn ensure_preview(&mut self) {
        if self.preview.is_none() && !self.in_memory {
            self.preview = Some(OutputPreview::read(&self.path));
        }
    }

    fn refresh_preview(&mut self) {
        if !self.in_memory {
            self.preview = Some(OutputPreview::read(&self.path));
        }
    }

    #[cfg(test)]
    pub(super) fn preview(&mut self) -> &OutputPreview {
        self.ensure_preview();
        match &self.preview {
            Some(preview) => preview,
            None => unreachable!("ensure_preview always initializes the preview"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OutputPreview {
    Text {
        content: String,
        total_bytes: u64,
        truncated: bool,
    },
    Binary {
        content: String,
        total_bytes: u64,
        truncated: bool,
    },
    Unavailable {
        message: String,
    },
}

impl OutputPreview {
    fn from_bytes(bytes: &[u8]) -> Self {
        let total_bytes = bytes.len() as u64;
        let truncated = bytes.len() > MAX_PREVIEW_BYTES;
        let end = bytes.len().min(MAX_PREVIEW_BYTES);
        let mut preview = &bytes[..end];
        match std::str::from_utf8(preview) {
            Ok(content) => Self::Text {
                content: content.to_string(),
                total_bytes,
                truncated,
            },
            Err(error) if truncated && error.error_len().is_none() => {
                preview = &preview[..error.valid_up_to()];
                Self::Text {
                    content: String::from_utf8_lossy(preview).into_owned(),
                    total_bytes,
                    truncated,
                }
            }
            Err(_) => Self::binary(bytes, total_bytes),
        }
    }

    fn binary(bytes: &[u8], total_bytes: u64) -> Self {
        let preview = &bytes[..bytes.len().min(MAX_BINARY_PREVIEW_BYTES)];
        let content = preview
            .chunks(16)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Self::Binary {
            content,
            total_bytes,
            truncated: total_bytes > MAX_BINARY_PREVIEW_BYTES as u64,
        }
    }

    fn read(path: &Path) -> Self {
        match read_preview(path) {
            Ok(preview) => preview,
            Err(error) => Self::Unavailable {
                message: error.to_string(),
            },
        }
    }

    fn total_bytes(&self) -> Option<u64> {
        match self {
            Self::Text { total_bytes, .. } | Self::Binary { total_bytes, .. } => Some(*total_bytes),
            Self::Unavailable { .. } => None,
        }
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, view: &mut RunReportView) {
    let mut window_open = *open;
    let title = match view.report.kind {
        RunReportKind::Run => "Run results",
        RunReportKind::Preview => "Preview results",
        RunReportKind::Pipeline => "Pipeline results",
    };
    egui::Window::new(title)
        .open(&mut window_open)
        .default_size(egui::vec2(860.0, 560.0))
        .min_size(egui::vec2(520.0, 320.0))
        .resizable(true)
        .show(ctx, |ui| show_report(ui, view));
    *open = window_open;
}

fn show_report(ui: &mut egui::Ui, view: &mut RunReportView) {
    let output_count = view.report.outputs.len();
    ui.horizontal_wrapped(|ui| {
        ui.strong(match view.report.kind {
            RunReportKind::Run => "Completed",
            RunReportKind::Preview => "Preview completed",
            RunReportKind::Pipeline => "Pipeline completed",
        });
        ui.separator();
        ui.label(match view.report.kind {
            RunReportKind::Run => {
                format!("Primary: {}", format_records(view.report.records_written))
            }
            RunReportKind::Preview => {
                format!("Records: {}", format_records(view.report.records_written))
            }
            RunReportKind::Pipeline => {
                format!("Records: {}", format_records(view.report.records_written))
            }
        });
        ui.separator();
        ui.label(format!(
            "Run time: {}",
            format_duration(view.report.duration)
        ));
        ui.separator();
        ui.label(format!(
            "{output_count} output{}",
            if output_count == 1 { "" } else { "s" }
        ));
        ui.separator();
        ui.label(format!("{} trace events", view.report.trace.events.len()));
    });
    ui.horizontal(|ui| {
        ui.weak(match view.report.kind {
            RunReportKind::Run => "Input",
            RunReportKind::Preview => "Logical input",
            RunReportKind::Pipeline => "Pipeline",
        });
        let input = view.report.input_path.display().to_string();
        ui.add(
            egui::Label::new(&input)
                .selectable(true)
                .wrap_mode(egui::TextWrapMode::Truncate),
        )
        .on_hover_text(input);
    });
    ui.separator();

    ui.horizontal(|ui| {
        ui.selectable_value(&mut view.page, ReportPage::Output, "Output");
        ui.selectable_value(&mut view.page, ReportPage::Trace, "Trace");
        ui.selectable_value(&mut view.page, ReportPage::History, "History");
        ui.selectable_value(&mut view.page, ReportPage::Replay, "Replay");
    });
    ui.separator();

    match view.page {
        ReportPage::Output => show_outputs(ui, view),
        ReportPage::Trace => show_trace(ui, view),
        ReportPage::History => show_history(ui, view),
        ReportPage::Replay => show_replay(ui, view),
    }
}

fn replay_action(ui: &mut egui::Ui, index: usize, label: &str) -> egui::Response {
    ui.small_button(label)
        .on_hover_text(format!("Replay from event {}", index.saturating_add(1)))
}

fn replayable_trace_row(ui: &mut egui::Ui, index: usize, row: String) -> bool {
    let mut replay = false;
    ui.horizontal(|ui| {
        replay = replay_action(ui, index, "Replay").clicked();
        ui.add(
            egui::Label::new(egui::RichText::new(&row).monospace())
                .selectable(true)
                .wrap_mode(egui::TextWrapMode::Truncate),
        )
        .on_hover_text(row);
    });
    replay
}

fn show_outputs(ui: &mut egui::Ui, view: &mut RunReportView) {
    view.selected_output = view
        .selected_output
        .min(view.report.outputs.len().saturating_sub(1));
    egui::ScrollArea::horizontal()
        .id_salt("run_output_tabs")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, output) in view.report.outputs.iter().enumerate() {
                    ui.selectable_value(&mut view.selected_output, index, &output.name);
                }
            });
        });
    ui.separator();

    let Some(output) = view.report.outputs.get_mut(view.selected_output) else {
        ui.weak(match view.report.kind {
            RunReportKind::Run => "No output files were produced.",
            RunReportKind::Preview => "No output artifacts were produced.",
            RunReportKind::Pipeline => "No output files were produced.",
        });
        return;
    };
    output.ensure_preview();
    let byte_label = output
        .preview
        .as_ref()
        .and_then(OutputPreview::total_bytes)
        .map(format_bytes);
    ui.horizontal_wrapped(|ui| {
        ui.strong(&output.name);
        ui.separator();
        ui.label(format_records(output.records_written));
        if let Some(bytes) = &byte_label {
            ui.separator();
            ui.label(bytes);
        }
    });
    ui.horizontal(|ui| {
        if output.in_memory {
            ui.weak("Logical output");
        }
        let path = output.path.display().to_string();
        ui.add(
            egui::Label::new(&path)
                .selectable(true)
                .wrap_mode(egui::TextWrapMode::Truncate),
        )
        .on_hover_text(&path);
        let copy_tooltip = if output.in_memory {
            "Copy logical output identity"
        } else {
            "Copy output path"
        };
        if crate::icons::button(ui, true, lucide_icons::Icon::Copy, copy_tooltip).clicked() {
            ui.ctx().copy_text(path);
        }
        if !output.in_memory
            && crate::icons::button(
                ui,
                true,
                lucide_icons::Icon::RefreshCw,
                "Refresh output preview",
            )
            .clicked()
        {
            output.refresh_preview();
        }
    });
    ui.separator();

    let Some(preview) = &output.preview else {
        return;
    };
    match preview {
        OutputPreview::Text {
            content, truncated, ..
        } => {
            ui.horizontal(|ui| {
                ui.strong("Preview");
                if *truncated {
                    ui.weak(format!("first {}", format_bytes(MAX_PREVIEW_BYTES as u64)));
                }
                if crate::icons::button(ui, true, lucide_icons::Icon::Copy, "Copy preview")
                    .clicked()
                {
                    ui.ctx().copy_text(content.clone());
                }
            });
            egui::ScrollArea::both()
                .id_salt(("run_output_preview", view.selected_output))
                .auto_shrink([false, false])
                .max_height(ui.available_height().max(120.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(content).monospace())
                            .selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        }
        OutputPreview::Binary {
            content, truncated, ..
        } => {
            ui.horizontal(|ui| {
                ui.strong("Hex preview");
                if *truncated {
                    ui.weak(format!(
                        "first {}",
                        format_bytes(MAX_BINARY_PREVIEW_BYTES as u64)
                    ));
                }
                if crate::icons::button(ui, true, lucide_icons::Icon::Copy, "Copy hex preview")
                    .clicked()
                {
                    ui.ctx().copy_text(content.clone());
                }
            });
            egui::ScrollArea::both()
                .id_salt(("run_binary_preview", view.selected_output))
                .auto_shrink([false, false])
                .max_height(ui.available_height().max(120.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(content).monospace())
                            .selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        }
        OutputPreview::Unavailable { message } => {
            ui.colored_label(ui.visuals().error_fg_color, "Preview unavailable");
            ui.label(message);
        }
    }
}

fn show_trace(ui: &mut egui::Ui, view: &mut RunReportView) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut view.trace_filter)
                .hint_text("Filter trace")
                .desired_width(280.0),
        );
        if !view.trace_filter.is_empty()
            && crate::icons::button(ui, true, lucide_icons::Icon::X, "Clear trace filter").clicked()
        {
            view.trace_filter.clear();
        }
        if view.report.trace.dropped > 0 {
            ui.weak(format!(
                "{} later events omitted",
                view.report.trace.dropped
            ));
        }
    });
    ui.separator();

    let rows = view.filtered_trace_indices();
    if rows.is_empty() {
        ui.weak("No matching trace events.");
        return;
    }

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
    let mut replay_from = None;
    egui::ScrollArea::vertical()
        .id_salt("run_trace")
        .auto_shrink([false, false])
        .show_rows(ui, row_height, rows.len(), |ui, range| {
            for index in &rows[range] {
                let row = view.trace_row(*index, &view.report.trace.events[*index]);
                if replayable_trace_row(ui, *index, row) {
                    replay_from = Some(*index);
                }
            }
        });
    if let Some(index) = replay_from {
        view.replay_from(index);
    }
}

fn show_history(ui: &mut egui::Ui, view: &mut RunReportView) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut view.history_mode, HistoryMode::Nodes, "Graph nodes");
        ui.selectable_value(
            &mut view.history_mode,
            HistoryMode::SourceRows,
            "Source rows",
        );
    });
    ui.separator();
    match view.history_mode {
        HistoryMode::Nodes => show_node_history(ui, view),
        HistoryMode::SourceRows => show_source_rows(ui, view),
    }
}

fn show_node_history(ui: &mut egui::Ui, view: &mut RunReportView) {
    show_history_stage_selector(ui, view);
    ui.horizontal_wrapped(|ui| {
        ui.label("Graph node value history");
        if let Some(node) = view.history_node {
            egui::ComboBox::from_id_salt("run_history_node")
                .selected_text(format!("Node {node}"))
                .show_ui(ui, |ui| {
                    for (&candidate, events) in &view.history_by_node {
                        ui.selectable_value(
                            &mut view.history_node,
                            Some(candidate),
                            format!("Node {candidate} ({} events)", events.len()),
                        );
                    }
                });
        }
        if view.report.trace.dropped > 0 {
            ui.weak(format!(
                "History may be incomplete: {} later trace events omitted",
                view.report.trace.dropped
            ));
        }
    });
    ui.weak("Successful inputs and outputs in evaluation order, with their source context.");
    ui.separator();

    let Some(indices) = view
        .history_node
        .and_then(|node| view.history_by_node.get(&node))
    else {
        ui.weak("No graph node values were recorded for this run.");
        return;
    };
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
    let mut replay_from = None;
    egui::ScrollArea::vertical()
        .id_salt("run_history")
        .auto_shrink([false, false])
        .show_rows(ui, row_height, indices.len(), |ui, range| {
            for (offset, &index) in indices[range.clone()].iter().enumerate() {
                let occurrence = range.start + offset + 1;
                let Some(event) = view.report.trace.events.get(index) else {
                    continue;
                };
                let Some(row) = history_row(occurrence, index, event) else {
                    continue;
                };
                let row = match view.report.trace.stage_at(index) {
                    Some(stage) => format!("stage {stage}  {row}"),
                    None => row,
                };
                if replayable_trace_row(ui, index, row) {
                    replay_from = Some(index);
                }
            }
        });
    if let Some(index) = replay_from {
        view.replay_from(index);
    }
}

pub(super) fn show_history_stage_selector(ui: &mut egui::Ui, view: &mut RunReportView) {
    if view.report.trace.stages.is_empty() {
        return;
    }
    let mut selected = view.history_stage;
    ui.horizontal(|ui| {
        ui.label("Pipeline stage");
        let label = selected
            .and_then(|index| view.report.trace.stages.get(usize::from(index)))
            .map_or("Select stage", String::as_str);
        egui::ComboBox::from_id_salt("run_history_stage")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (index, stage) in view.report.trace.stages.iter().enumerate() {
                    if let Ok(index) = u16::try_from(index) {
                        ui.selectable_value(&mut selected, Some(index), stage);
                    }
                }
            });
    });
    if selected != view.history_stage
        && let Some(stage) = selected
    {
        view.select_history_stage(stage);
    }
}

fn history_row(occurrence: usize, event_index: usize, event: &cli::TraceEvent) -> Option<String> {
    let (direction, positions, value) = match event {
        cli::TraceEvent::NodeValue {
            positions, value, ..
        } => ("output".to_string(), positions, value),
        cli::TraceEvent::NodeInputValue {
            input,
            input_index,
            positions,
            value,
            ..
        } => (
            format!("input {} <- node {input}", input_index + 1),
            positions,
            value,
        ),
        _ => return None,
    };
    let context = if positions.is_empty() {
        "<root>".to_string()
    } else {
        positions
            .iter()
            .map(format_trace_position)
            .collect::<Vec<_>>()
            .join(" > ")
    };
    let truncated = if value.truncated { " [truncated]" } else { "" };
    Some(format!(
        "{:>6}  event {:>6}  {direction}  {context}  {}{truncated}",
        occurrence,
        event_index + 1,
        format_trace_value(value)
    ))
}

fn trace_row(index: usize, event: &cli::TraceEvent) -> String {
    let prefix = format!("{:>6}", index + 1);
    match event {
        cli::TraceEvent::NodeValue {
            node,
            positions,
            value,
        } => {
            let context = positions
                .iter()
                .map(format_trace_position)
                .collect::<Vec<_>>()
                .join(" > ");
            let value = format_trace_value(value);
            if context.is_empty() {
                format!("{prefix}  node {node:<6}  {value}")
            } else {
                format!("{prefix}  node {node:<6}  {context}  {value}")
            }
        }
        cli::TraceEvent::NodeInputValue {
            consumer,
            input,
            input_index,
            positions,
            value,
        } => format!(
            "{prefix}  node {consumer} input {} <- node {input}  {}{}",
            input_index + 1,
            format_trace_value(value),
            format_trace_positions(positions)
        ),
        cli::TraceEvent::ScopeStarted {
            scope,
            iteration,
            positions,
        } => format!(
            "{prefix}  scope {}  start {}{}",
            format_trace_scope(scope),
            format_trace_iteration(iteration),
            format_trace_positions(positions)
        ),
        cli::TraceEvent::IterationCandidate {
            scope,
            ordinal,
            positions,
            source_row,
        } => format!(
            "{prefix}  scope {}  candidate {ordinal}{}{}",
            format_trace_scope(scope),
            format_trace_positions(positions),
            source_row
                .as_ref()
                .map(|row| format!("  source row {}", format_source_row(row)))
                .unwrap_or_default()
        ),
        cli::TraceEvent::FilterDecision {
            scope,
            node,
            phase,
            positions,
            passed,
        } => format!(
            "{prefix}  scope {}  filter node {node} {} {}{}",
            format_trace_scope(scope),
            format_filter_phase(*phase),
            if *passed { "pass" } else { "drop" },
            format_trace_positions(positions)
        ),
        cli::TraceEvent::SortCandidate {
            scope,
            positions,
            keys,
        } => {
            let keys = keys
                .iter()
                .map(|key| {
                    format!(
                        "node {} {}={}",
                        key.node,
                        if key.descending { "desc" } else { "asc" },
                        format_trace_value(&key.value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{prefix}  scope {}  sort keys [{keys}]{}",
                format_trace_scope(scope),
                format_trace_positions(positions)
            )
        }
        cli::TraceEvent::SortPosition {
            scope,
            positions,
            output_index,
        } => format!(
            "{prefix}  scope {}  sorted position {output_index}{}",
            format_trace_scope(scope),
            format_trace_positions(positions)
        ),
        cli::TraceEvent::GroupProduced {
            scope,
            grouping,
            group_index,
            member_count,
            key,
            retained,
            positions,
        } => {
            let key = key
                .as_ref()
                .map(|key| format!(" key={}", format_trace_value(key)))
                .unwrap_or_default();
            format!(
                "{prefix}  scope {}  group {group_index} {} members={member_count}{key} {}{}",
                format_trace_scope(scope),
                format_trace_grouping(*grouping),
                if *retained { "retain" } else { "drop" },
                format_trace_positions(positions)
            )
        }
        cli::TraceEvent::WindowApplied {
            scope,
            window_index,
            window,
            before,
            after,
        } => format!(
            "{prefix}  scope {}  window {window_index} {}  {before} -> {after}",
            format_trace_scope(scope),
            format_trace_window(*window)
        ),
        cli::TraceEvent::TargetFieldWritten {
            scope,
            field,
            binding,
            positions,
            kind,
            value,
        } => {
            let value = value
                .as_ref()
                .map(|value| format!(" value={}", format_trace_value(value)))
                .unwrap_or_default();
            format!(
                "{prefix}  scope {}  field {field} {} write {}{value}{}",
                format_trace_scope(scope),
                format_target_field_binding(*binding),
                format_output_kind(*kind),
                format_trace_positions(positions)
            )
        }
        cli::TraceEvent::TargetProduced {
            scope,
            positions,
            output_path,
            kind,
        } => {
            let output_path = output_path
                .as_ref()
                .map(|path| format!(" path={path}"))
                .unwrap_or_default();
            format!(
                "{prefix}  scope {}  produce {}{output_path}{}",
                format_trace_scope(scope),
                format_output_kind(*kind),
                format_trace_positions(positions)
            )
        }
        cli::TraceEvent::ScopeFinished {
            scope,
            candidates,
            produced,
            kind,
        } => format!(
            "{prefix}  scope {}  finish candidates={candidates} produced={produced} {}",
            format_trace_scope(scope),
            format_output_kind(*kind)
        ),
    }
}

fn format_target_field_binding(binding: cli::TraceTargetFieldBinding) -> String {
    match binding {
        cli::TraceTargetFieldBinding::StaticBinding { value } => {
            format!("static-binding value-node={value}")
        }
        cli::TraceTargetFieldBinding::DynamicBinding { key, value } => {
            format!("dynamic-binding key-node={key} value-node={value}")
        }
        cli::TraceTargetFieldBinding::StaticChild => "static-child".into(),
        cli::TraceTargetFieldBinding::DynamicChild { key } => {
            format!("dynamic-child key-node={key}")
        }
    }
}

fn format_trace_scope(scope: &cli::TraceScope) -> String {
    let mut text = match &scope.target {
        cli::TraceTarget::Primary => "primary".to_string(),
        cli::TraceTarget::Named(name) => format!("named:{name}"),
    };
    if !scope.target_path.is_empty() {
        text.push_str(":/");
        text.push_str(&scope.target_path.join("/"));
    }
    text.push_str(" #");
    if scope.structural_path.is_empty() {
        text.push_str("root");
    } else {
        text.push_str(
            &scope
                .structural_path
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join("."),
        );
    }
    text
}

fn format_trace_iteration(iteration: &cli::TraceIteration) -> String {
    match iteration {
        cli::TraceIteration::Once => "once".to_string(),
        cli::TraceIteration::Source { path } => format!(
            "iterate source {}",
            if path.is_empty() {
                "<current>".to_string()
            } else {
                path.join("/")
            }
        ),
        cli::TraceIteration::DynamicDocuments { source } => format!(
            "dynamic documents {}",
            if source.is_empty() {
                "<current>".to_string()
            } else {
                source.join("/")
            }
        ),
        cli::TraceIteration::Generated { kind } => format!("generate {kind}"),
        cli::TraceIteration::Join { join } => format!("join {}", join.get()),
        cli::TraceIteration::Concatenate { segments } => {
            format!("concatenate {segments} segments")
        }
    }
}

fn format_filter_phase(phase: cli::TraceFilterPhase) -> &'static str {
    match phase {
        cli::TraceFilterPhase::BeforeSort => "before-sort",
        cli::TraceFilterPhase::AfterSort => "after-sort",
        cli::TraceFilterPhase::Selection => "selection",
        cli::TraceFilterPhase::GroupStarting => "group-start",
        cli::TraceFilterPhase::GroupEnding => "group-end",
        cli::TraceFilterPhase::PostGroupMember => "post-group",
    }
}

fn format_trace_grouping(grouping: cli::TraceGrouping) -> String {
    match grouping {
        cli::TraceGrouping::By { node } => format!("by node {node}"),
        cli::TraceGrouping::AdjacentBy { node } => format!("adjacent-by node {node}"),
        cli::TraceGrouping::StartingWith { node } => format!("starting-with node {node}"),
        cli::TraceGrouping::EndingWith { node } => format!("ending-with node {node}"),
        cli::TraceGrouping::IntoBlocks { node, size } => {
            format!("blocks node {node} size={size}")
        }
    }
}

fn format_trace_window(window: cli::TraceWindow) -> String {
    match window {
        cli::TraceWindow::SkipFirst(count) => format!("skip-first {count}"),
        cli::TraceWindow::First(count) => format!("first {count}"),
        cli::TraceWindow::From(position) => format!("from {position}"),
        cli::TraceWindow::FromTo { first, last } => format!("from {first} to {last}"),
        cli::TraceWindow::Last(count) => format!("last {count}"),
    }
}

fn format_trace_value(value: &cli::TraceValue) -> String {
    let suffix = if value.truncated { "..." } else { "" };
    format!("{}({}{suffix})", value.value_type, value.preview)
}

fn format_output_kind(kind: cli::TraceOutputKind) -> &'static str {
    match kind {
        cli::TraceOutputKind::Scalar => "scalar",
        cli::TraceOutputKind::Group => "group",
        cli::TraceOutputKind::Repeated => "repeated",
        cli::TraceOutputKind::MappedSequence => "mapped-sequence",
        cli::TraceOutputKind::DocumentSet => "document-set",
    }
}

fn format_trace_positions(positions: &[cli::TracePosition]) -> String {
    if positions.is_empty() {
        String::new()
    } else {
        format!(
            "  {}",
            positions
                .iter()
                .map(format_trace_position)
                .collect::<Vec<_>>()
                .join(" > ")
        )
    }
}

fn format_trace_position(position: &cli::TracePosition) -> String {
    let collection = if position.collection.is_empty() {
        "<root>".to_string()
    } else {
        position.collection.join("/")
    };
    let mut text = format!("{collection}[{}]", position.index);
    if position.grouped {
        text.push_str(" group");
    }
    if let Some(join) = position.join {
        text.push_str(&format!(" join={}", join.get()));
    }
    if let Some((join, index)) = position.join_position {
        text.push_str(&format!(" tuple={}[{index}]", join.get()));
    }
    if let Some(path) = &position.document_path {
        text.push_str(" @");
        text.push_str(path);
    }
    text
}

fn read_preview(path: &Path) -> std::io::Result<OutputPreview> {
    let mut file = std::fs::File::open(path)?;
    let total_bytes = file.metadata()?.len();
    let capacity = total_bytes.min(MAX_PREVIEW_BYTES as u64) as usize;
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take((MAX_PREVIEW_BYTES + 4) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = total_bytes > MAX_PREVIEW_BYTES as u64;
    if truncated && bytes.len() > MAX_PREVIEW_BYTES {
        bytes.truncate(MAX_PREVIEW_BYTES);
    }

    match std::str::from_utf8(&bytes) {
        Ok(content) => Ok(OutputPreview::Text {
            content: content.to_string(),
            total_bytes,
            truncated,
        }),
        Err(error) if truncated && error.error_len().is_none() => {
            bytes.truncate(error.valid_up_to());
            Ok(OutputPreview::Text {
                content: String::from_utf8_lossy(&bytes).into_owned(),
                total_bytes,
                truncated,
            })
        }
        Err(_) => Ok(OutputPreview::binary(&bytes, total_bytes)),
    }
}

fn format_records(records: usize) -> String {
    format!("{records} record{}", if records == 1 { "" } else { "s" })
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1 {
        format!("{} us", duration.as_micros())
    } else if duration.as_secs_f64() < 1.0 {
        format!("{} ms", duration.as_millis())
    } else {
        format!("{:.2} s", duration.as_secs_f64())
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
#[path = "run_report_tests.rs"]
mod tests;
