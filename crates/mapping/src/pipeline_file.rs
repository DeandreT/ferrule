//! File persistence for [`crate::Pipeline`], including every embedded project.
//! Uses the same bounded, file-only lossless format as [`crate::project_file`].
use crate::{FileCodecError, Pipeline};
/// Maximum UTF-8 size of a new or versioned pipeline file.
pub const MAX_DOCUMENT_BYTES: usize = crate::file_codec::MAX_DOCUMENT_BYTES;
/// Decode UTF-8 file text without performing filesystem I/O.
pub fn decode_str(text: &str) -> Result<Pipeline, FileCodecError> {
    crate::file_codec::decode_str(text, "pipeline")
}
/// Decode file bytes, rejecting invalid UTF-8.
pub fn decode_bytes(bytes: &[u8]) -> Result<Pipeline, FileCodecError> {
    crate::file_codec::decode_bytes(bytes, "pipeline")
}
/// Encode a faithful file document with pretty formatting and a trailing newline.
pub fn encode_pretty(pipeline: &Pipeline) -> Result<String, FileCodecError> {
    crate::file_codec::encode_pretty(pipeline, "pipeline")
}
