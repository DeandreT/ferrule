//! File persistence for [`crate::PdfLayout`]. Ordinary layout JSON retains its
//! legacy parser. The file-only versioned envelope preserves finite binary64
//! coordinates and metrics exactly, using the same codec as project files.
//! New saves and versioned reads are capped at 64 MiB; enclosing formats may
//! impose smaller limits independently.
use crate::{FileCodecError, PdfLayout};
/// Maximum UTF-8 size of a new or versioned PDF layout file.
pub const MAX_DOCUMENT_BYTES: usize = crate::file_codec::MAX_DOCUMENT_BYTES;
/// Decode UTF-8 layout text without performing filesystem I/O.
pub fn decode_str(text: &str) -> Result<PdfLayout, FileCodecError> {
    crate::file_codec::decode_str(text, "pdf_layout")
}
/// Decode layout bytes, rejecting invalid UTF-8.
pub fn decode_bytes(bytes: &[u8]) -> Result<PdfLayout, FileCodecError> {
    crate::file_codec::decode_bytes(bytes, "pdf_layout")
}
/// Encode a faithful layout file with pretty formatting and a trailing newline.
pub fn encode_pretty(layout: &PdfLayout) -> Result<String, FileCodecError> {
    crate::file_codec::encode_pretty(layout, "pdf_layout")
}
