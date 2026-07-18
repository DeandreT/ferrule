use serde::{Deserialize, Serialize};

/// Runtime identity for a pathless flat tabular document boundary.
///
/// Detailed CSV and XLSX settings remain in [`crate::FormatOptions`]. This
/// discriminator is used only when an instance path has no recognized format
/// extension, so explicit filenames continue to select their own adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabularBoundaryKind {
    Csv,
    Xlsx,
}
