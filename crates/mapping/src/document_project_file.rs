//! Lossless file persistence for [`crate::DocumentProject`].
//!
//! Uses the existing bounded mapping-file codec, with a distinct document kind.
//! Float metadata covers the complete embedded project and ordinary model serde
//! remains separate from this file persistence contract.

use crate::{DocumentProject, FileCodecError};

pub const MAX_DOCUMENT_BYTES: usize = crate::file_codec::MAX_DOCUMENT_BYTES;

pub fn decode_str(text: &str) -> Result<DocumentProject, FileCodecError> {
    crate::file_codec::decode_str(text, "document_project")
}

pub fn decode_bytes(bytes: &[u8]) -> Result<DocumentProject, FileCodecError> {
    crate::file_codec::decode_bytes(bytes, "document_project")
}

pub fn encode_pretty(document: &DocumentProject) -> Result<String, FileCodecError> {
    crate::file_codec::encode_pretty(document, "document_project")
}
