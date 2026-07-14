//! Bounded visual extraction of PDF source instances.

mod extract;
mod layout;

use std::path::Path;

use ir::Instance;
use mapping::PdfLayout;
use thiserror::Error;

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PAGES: usize = 256;
pub const MAX_EVENTS: usize = 1_000_000;
pub const MAX_OUTPUT_NODES: usize = 1_000_000;
pub const MAX_VALUE_BYTES: usize = 1_048_576;
pub const MAX_INSTANCE_DEPTH: usize = 64;

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF input exceeds the {MAX_INPUT_BYTES}-byte limit")]
    InputTooLarge,
    #[error("PDF document exceeds the {MAX_PAGES}-page limit")]
    TooManyPages,
    #[error("PDF document exceeds the {MAX_EVENTS}-event limit")]
    TooManyEvents,
    #[error("PDF decoded text exceeds the {MAX_VALUE_BYTES}-byte limit")]
    DecodedTextTooLarge,
    #[error("PDF output exceeds the {MAX_OUTPUT_NODES}-node limit")]
    TooManyOutputNodes,
    #[error("PDF output value exceeds the {MAX_VALUE_BYTES}-byte limit at `{0}`")]
    ValueTooLarge(String),
    #[error("PDF output exceeds the {MAX_INSTANCE_DEPTH}-level depth limit")]
    InstanceTooDeep,
    #[error("invalid PDF: {0}")]
    InvalidPdf(String),
    #[error("invalid PDF layout at runtime: {0}")]
    InvalidLayout(String),
    #[error("invalid PDF layout at runtime: command resolved outside its candidate region")]
    InvalidCandidateRegion,
}

pub fn read(path: &Path, layout: &PdfLayout) -> Result<Instance, PdfError> {
    let bytes = std::fs::read(path)?;
    from_bytes(&bytes, layout)
}

pub fn from_bytes(bytes: &[u8], layout: &PdfLayout) -> Result<Instance, PdfError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(PdfError::InputTooLarge);
    }
    let pages = extract::extract_pages(bytes)?;
    layout::evaluate(&pages, layout)
}
