//! The last successful mapping result and its provenance.

use web_demo::runtime::DataFormat;

/// Target text from one successful run, together with the format that
/// produced it. Later edits, format selections and failed runs never rewrite
/// these fields, so the heading, the download filename and the downloaded
/// bytes always describe the same result.
#[derive(Debug)]
pub(super) struct MappedOutput {
    text: String,
    format: DataFormat,
    superseded_by_failure: bool,
}

/// How a retained result relates to the current project, input and formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OutputFreshness {
    Current,
    InputsChanged,
    RunFailed,
}

impl MappedOutput {
    pub(super) fn new(text: String, format: DataFormat) -> Self {
        Self {
            text,
            format,
            superseded_by_failure: false,
        }
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// Records that a run attempted after this result failed; the result is
    /// retained only as the last successful one.
    pub(super) fn mark_superseded_by_failure(&mut self) {
        self.superseded_by_failure = true;
    }

    /// `inputs_changed` reports edits since the latest run attempt, which take
    /// precedence over that attempt's failure.
    pub(super) fn freshness(&self, inputs_changed: bool) -> OutputFreshness {
        if inputs_changed {
            OutputFreshness::InputsChanged
        } else if self.superseded_by_failure {
            OutputFreshness::RunFailed
        } else {
            OutputFreshness::Current
        }
    }

    pub(super) fn heading(&self, inputs_changed: bool) -> String {
        let format = self.format;
        match self.freshness(inputs_changed) {
            OutputFreshness::Current => format!("Output ({format})"),
            OutputFreshness::InputsChanged => {
                format!("Output ({format}, stale: inputs changed since this run)")
            }
            OutputFreshness::RunFailed => {
                format!("Output ({format}, stale: last successful run; the latest run failed)")
            }
        }
    }

    pub(super) fn download_filename(&self) -> String {
        format!("mapped-output.{}", format_extension(self.format))
    }
}

fn format_extension(format: DataFormat) -> &'static str {
    match format {
        DataFormat::Xml => "xml",
        DataFormat::Json => "json",
        DataFormat::Csv => "csv",
        DataFormat::Xbrl => "xbrl",
    }
}
