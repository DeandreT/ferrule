//! Explicit physical X12 output policy. Never inferred from the host platform.

use serde::{Deserialize, Serialize};

use crate::Project;

pub const X12_SEGMENT_LINE_ENDING_V1_CAPABILITY: &str = "x12-segment-line-ending-v1";

/// Formatting after a visible one-character X12 segment terminator.
///
/// LF retains the legacy suppression after LF/CR terminators. CRLF requires a
/// visible terminator; it never changes the delimiter declared by ISA.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", try_from = "String")]
pub enum X12SegmentLineEnding {
    #[default]
    Lf,
    CrLf,
}

impl TryFrom<String> for X12SegmentLineEnding {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "lf" => Ok(Self::Lf),
            "crlf" => Ok(Self::CrLf),
            _ => Err("unsupported X12 segment line ending"),
        }
    }
}

impl X12SegmentLineEnding {
    pub const fn is_lf(&self) -> bool {
        matches!(self, Self::Lf)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

impl Project {
    /// Include invalid placements: file readers must not silently drop them.
    pub fn uses_x12_segment_line_ending(&self) -> bool {
        std::iter::once(&self.source_options)
            .chain(std::iter::once(&self.target_options))
            .chain(self.extra_sources.iter().map(|source| &source.options))
            .chain(self.extra_targets.iter().map(|target| &target.options))
            .any(|options| !options.x12_segment_line_ending.is_lf())
    }
}
