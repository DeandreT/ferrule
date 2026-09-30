//! File persistence for [`crate::Project`]. Unmarked JSON retains ordinary serde
//! behavior. Saves use ordinary JSON when its complete model round-trips exactly;
//! otherwise a version-2 JSON envelope preserves every finite float tag and bit.
//! Versioned files and new saves are capped at 64 MiB and 127 JSON containers.
//! The envelope costs one container depth. Its float table is authoritative;
//! editing a decimal requires matching metadata. Ordinary model serde is unchanged.
use crate::{FileCodecError, Project};
/// Maximum UTF-8 size of a new or versioned project file.
pub const MAX_DOCUMENT_BYTES: usize = crate::file_codec::MAX_DOCUMENT_BYTES;
/// Decode UTF-8 file text without performing filesystem I/O.
pub fn decode_str(text: &str) -> Result<Project, FileCodecError> {
    crate::file_codec::decode_str(text, "project")
}
/// Decode file bytes, rejecting invalid UTF-8.
pub fn decode_bytes(bytes: &[u8]) -> Result<Project, FileCodecError> {
    crate::file_codec::decode_bytes(bytes, "project")
}
/// Encode a faithful file document with pretty formatting and a trailing newline.
pub fn encode_pretty(project: &Project) -> Result<String, FileCodecError> {
    crate::file_codec::encode_pretty(project, "project")
}
