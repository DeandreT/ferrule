//! Last-run summary and bounded output previews for the native GUI.

use std::cell::{Cell, RefCell};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_PREVIEW_BYTES: usize = 1024 * 1024;
pub const MAX_TRACE_EVENTS: usize = 50_000;

#[derive(Debug, Default)]
pub struct TraceReport {
    pub events: Vec<cli::TraceEvent>,
    pub dropped: usize,
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
        }
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
    pub duration: Duration,
    pub records_written: usize,
    pub input_path: PathBuf,
    pub outputs: Vec<RunOutput>,
    pub trace: TraceReport,
}

impl RunReport {
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
            duration,
            records_written: outcome.records_written,
            input_path: outcome.input_path,
            outputs,
            trace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportPage {
    Output,
    Trace,
}

#[derive(Debug)]
pub struct RunReportView {
    pub report: RunReport,
    selected_output: usize,
    page: ReportPage,
    trace_filter: String,
}

impl RunReportView {
    pub fn new(report: RunReport) -> Self {
        Self {
            report,
            selected_output: 0,
            page: ReportPage::Output,
            trace_filter: String::new(),
        }
    }

    #[cfg(test)]
    pub fn selected_output(&self) -> usize {
        self.selected_output
    }
}

#[derive(Debug)]
pub struct RunOutput {
    pub name: String,
    pub records_written: usize,
    pub path: PathBuf,
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

    fn new(name: String, records_written: usize, path: PathBuf) -> Self {
        Self {
            name,
            records_written,
            path,
            preview: None,
        }
    }

    fn ensure_preview(&mut self) {
        if self.preview.is_none() {
            self.preview = Some(OutputPreview::read(&self.path));
        }
    }

    fn refresh_preview(&mut self) {
        self.preview = Some(OutputPreview::read(&self.path));
    }

    #[cfg(test)]
    fn preview(&mut self) -> &OutputPreview {
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
        total_bytes: u64,
    },
    Unavailable {
        message: String,
    },
}

impl OutputPreview {
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
            Self::Text { total_bytes, .. } | Self::Binary { total_bytes } => Some(*total_bytes),
            Self::Unavailable { .. } => None,
        }
    }
}

pub fn show(ctx: &egui::Context, open: &mut bool, view: &mut RunReportView) {
    let mut window_open = *open;
    egui::Window::new("Run results")
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
        ui.strong("Completed");
        ui.separator();
        ui.label(format!(
            "Primary: {}",
            format_records(view.report.records_written)
        ));
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
        ui.weak("Input");
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
    });
    ui.separator();

    match view.page {
        ReportPage::Output => show_outputs(ui, view),
        ReportPage::Trace => show_trace(ui, view),
    }
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
        ui.weak("No output files were produced.");
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
        let path = output.path.display().to_string();
        ui.add(
            egui::Label::new(&path)
                .selectable(true)
                .wrap_mode(egui::TextWrapMode::Truncate),
        )
        .on_hover_text(&path);
        if crate::icons::button(ui, true, lucide_icons::Icon::Copy, "Copy output path").clicked() {
            ui.ctx().copy_text(path);
        }
        if crate::icons::button(
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
        OutputPreview::Binary { .. } => {
            ui.strong("Binary output");
            ui.weak("A text preview is not available for this file.");
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

    let filter = view.trace_filter.trim().to_lowercase();
    let rows = view
        .report
        .trace
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            (filter.is_empty() || trace_row(index, event).to_lowercase().contains(&filter))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        ui.weak("No matching trace events.");
        return;
    }

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
    egui::ScrollArea::vertical()
        .id_salt("run_trace")
        .auto_shrink([false, false])
        .show_rows(ui, row_height, rows.len(), |ui, range| {
            for index in &rows[range] {
                let row = trace_row(*index, &view.report.trace.events[*index]);
                ui.add(
                    egui::Label::new(egui::RichText::new(&row).monospace())
                        .selectable(true)
                        .wrap_mode(egui::TextWrapMode::Truncate),
                )
                .on_hover_text(row);
            }
        });
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
            let value = serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"));
            if context.is_empty() {
                format!("{prefix}  node {node:<6}  {value}")
            } else {
                format!("{prefix}  node {node:<6}  {context}  {value}")
            }
        }
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
        } => format!(
            "{prefix}  scope {}  candidate {ordinal}{}",
            format_trace_scope(scope),
            format_trace_positions(positions)
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
        Err(_) => Ok(OutputPreview::Binary { total_bytes }),
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
mod tests {
    use super::*;

    fn trace_event(node: mapping::NodeId, value: ir::Value) -> cli::TraceEvent {
        cli::TraceEvent::NodeValue {
            node,
            positions: Vec::new(),
            value,
        }
    }

    fn trace_scope() -> cli::TraceScope {
        cli::TraceScope {
            target: cli::TraceTarget::Named("Audit".into()),
            target_path: vec!["Orders".into(), "Line".into()],
            structural_path: vec![1, 3],
        }
    }

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ferrule_gui_run_report_{name}_{}",
            std::process::id()
        ))
    }

    #[test]
    fn text_previews_are_bounded_without_splitting_utf8() {
        let path = temporary_path("text");
        let mut content = "a".repeat(MAX_PREVIEW_BYTES - 1);
        content.push('é');
        std::fs::write(&path, content).expect("preview fixture is written");

        let preview = read_preview(&path).expect("preview is read");
        let OutputPreview::Text {
            content,
            total_bytes,
            truncated,
        } = preview
        else {
            panic!("UTF-8 preview should remain text");
        };
        assert!(truncated);
        assert_eq!(total_bytes, MAX_PREVIEW_BYTES as u64 + 1);
        assert_eq!(content.len(), MAX_PREVIEW_BYTES - 1);
        std::fs::remove_file(path).expect("preview fixture is removed");
    }

    #[test]
    fn binary_and_missing_outputs_have_explicit_preview_states() {
        let path = temporary_path("binary");
        std::fs::write(&path, [0xff, 0x00, 0x80]).expect("preview fixture is written");
        assert_eq!(
            read_preview(&path).expect("preview is read"),
            OutputPreview::Binary { total_bytes: 3 }
        );
        std::fs::remove_file(&path).expect("preview fixture is removed");
        assert!(matches!(
            OutputPreview::read(&path),
            OutputPreview::Unavailable { .. }
        ));
    }

    #[test]
    fn report_construction_defers_output_reads_until_selected() {
        let missing = temporary_path("lazy");
        let outcome = cli::RunOutcome {
            records_written: 2,
            input_path: PathBuf::from("input.xml"),
            output_path: missing.clone(),
            primary_outputs: Vec::new(),
            extra_outputs: Vec::new(),
            artifacts: vec![cli::WrittenOutput {
                name: "Primary".into(),
                records_written: 2,
                path: missing,
            }],
        };

        let mut report = RunReport::from_outcome_with_trace(
            outcome,
            Duration::from_millis(12),
            TraceReport::default(),
        );

        assert!(report.outputs[0].preview.is_none());
        assert!(matches!(
            report.outputs[0].preview(),
            OutputPreview::Unavailable { .. }
        ));
    }

    #[test]
    fn dynamic_and_extra_outputs_keep_their_declared_order() {
        let outcome = cli::RunOutcome {
            records_written: 3,
            input_path: PathBuf::from("input.xml"),
            output_path: PathBuf::from("dynamic-base"),
            primary_outputs: vec![cli::WrittenOutput {
                name: "Primary 1".into(),
                records_written: 2,
                path: PathBuf::from("first.xml"),
            }],
            extra_outputs: vec![cli::WrittenOutput {
                name: "Audit".into(),
                records_written: 1,
                path: PathBuf::from("audit.json"),
            }],
            artifacts: vec![
                cli::WrittenOutput {
                    name: "Primary 1".into(),
                    records_written: 2,
                    path: PathBuf::from("first.xml"),
                },
                cli::WrittenOutput {
                    name: "Audit".into(),
                    records_written: 1,
                    path: PathBuf::from("audit.json"),
                },
            ],
        };

        let report = RunReport::from_outcome_with_trace(
            outcome,
            Duration::from_millis(4),
            TraceReport::default(),
        );

        assert_eq!(
            report
                .outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            ["Primary 1", "Audit"]
        );
        assert!(report.outputs.iter().all(|output| output.preview.is_none()));
    }

    #[test]
    fn summary_units_are_compact_and_deterministic() {
        assert_eq!(format_records(1), "1 record");
        assert_eq!(format_records(2), "2 records");
        assert_eq!(format_duration(Duration::from_millis(1250)), "1.25 s");
        assert_eq!(format_bytes(1536), "1.5 KiB");
    }

    #[test]
    fn trace_collection_is_bounded_and_reports_omissions() {
        let collector = TraceCollector::with_limit(2);
        cli::TraceSink::record(&collector, trace_event(1, ir::Value::Int(10)));
        cli::TraceSink::record(&collector, trace_event(2, ir::Value::String("kept".into())));
        cli::TraceSink::record(&collector, trace_event(3, ir::Value::Bool(false)));

        let trace = collector.finish();
        assert_eq!(trace.events.len(), 2);
        assert_eq!(trace.dropped, 1);
        assert!(trace_row(1, &trace.events[1]).contains("node 2"));
    }

    #[test]
    fn scope_trace_rows_expose_searchable_control_context() {
        let started = cli::TraceEvent::ScopeStarted {
            scope: trace_scope(),
            iteration: cli::TraceIteration::Source {
                path: vec!["Order".into(), "Line".into()],
            },
            positions: Vec::new(),
        };
        let filtered = cli::TraceEvent::FilterDecision {
            scope: trace_scope(),
            node: 42,
            phase: cli::TraceFilterPhase::BeforeSort,
            positions: vec![cli::TracePosition {
                collection: vec!["Order".into()],
                index: 3,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            }],
            passed: false,
        };
        let window = cli::TraceEvent::WindowApplied {
            scope: trace_scope(),
            window_index: 2,
            window: cli::TraceWindow::FromTo { first: 2, last: 4 },
            before: 8,
            after: 3,
        };

        assert!(trace_row(0, &started).contains("named:Audit:/Orders/Line #1.3"));
        assert!(trace_row(0, &started).contains("iterate source Order/Line"));
        assert!(trace_row(1, &filtered).contains("filter node 42 before-sort drop"));
        assert!(trace_row(1, &filtered).contains("Order[3]"));
        assert!(trace_row(2, &window).contains("window 2 from 2 to 4  8 -> 3"));
    }

    #[test]
    fn results_window_renders_and_loads_only_the_selected_preview() {
        let first = temporary_path("window-first");
        let second = temporary_path("window-second");
        std::fs::write(&first, "<result>ok</result>").expect("first output is written");
        std::fs::write(&second, "not selected").expect("second output is written");
        let report = RunReport {
            duration: Duration::from_millis(8),
            records_written: 1,
            input_path: PathBuf::from("input.xml"),
            outputs: vec![
                RunOutput::new("Primary".into(), 1, first.clone()),
                RunOutput::new("Audit".into(), 1, second.clone()),
            ],
            trace: TraceReport {
                events: vec![trace_event(7, ir::Value::String("ok".into()))],
                dropped: 0,
            },
        };
        let mut view = RunReportView::new(report);
        let mut open = true;
        let context = egui::Context::default();
        crate::icons::install(&context);

        let output = context.run_ui(Default::default(), |ui| {
            show(ui.ctx(), &mut open, &mut view);
        });

        assert!(open);
        assert!(!output.shapes.is_empty());
        assert!(view.report.outputs[0].preview.is_some());
        assert!(view.report.outputs[1].preview.is_none());

        view.page = ReportPage::Trace;
        let output = context.run_ui(Default::default(), |ui| {
            show(ui.ctx(), &mut open, &mut view);
        });
        assert!(!output.shapes.is_empty());
        std::fs::remove_file(first).expect("first output is removed");
        std::fs::remove_file(second).expect("second output is removed");
    }
}
