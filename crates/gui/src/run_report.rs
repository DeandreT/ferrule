//! Last-run summary and bounded output previews for the native GUI.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_PREVIEW_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct RunReport {
    pub duration: Duration,
    pub records_written: usize,
    pub input_path: PathBuf,
    pub outputs: Vec<RunOutput>,
}

impl RunReport {
    pub fn from_outcome(outcome: cli::RunOutcome, duration: Duration) -> Self {
        let mut outputs = if outcome.primary_outputs.is_empty() {
            vec![RunOutput::new(
                "Primary".to_string(),
                outcome.records_written,
                outcome.output_path.clone(),
            )]
        } else {
            outcome
                .primary_outputs
                .iter()
                .map(RunOutput::from_written)
                .collect()
        };
        outputs.extend(outcome.extra_outputs.iter().map(RunOutput::from_written));
        Self {
            duration,
            records_written: outcome.records_written,
            input_path: outcome.input_path,
            outputs,
        }
    }
}

#[derive(Debug)]
pub struct RunReportView {
    pub report: RunReport,
    selected_output: usize,
}

impl RunReportView {
    pub fn new(report: RunReport) -> Self {
        Self {
            report,
            selected_output: 0,
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
        };

        let mut report = RunReport::from_outcome(outcome, Duration::from_millis(12));

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
        };

        let report = RunReport::from_outcome(outcome, Duration::from_millis(4));

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
        std::fs::remove_file(first).expect("first output is removed");
        std::fs::remove_file(second).expect("second output is removed");
    }
}
