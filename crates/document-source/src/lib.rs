//! Format-neutral primary document acquisition, separate from decoding and mapping.
//!
//! Hosts select logical requests and supply bytes, optionally with resolved
//! locations. Callers supply their parser and retain responsibility for mapping
//! and output publication. The local file adapter is an explicit host policy,
//! not a requirement of the acquisition contract.

mod local_files;
mod primary_documents;

pub use local_files::{DEFAULT_LOCAL_DOCUMENT_BYTES, LocalFileError, LocalPrimaryFiles};
pub use primary_documents::{
    PrimaryAcquisitionError, PrimaryAssemblyError, PrimaryDocument, PrimarySourceHost,
    acquire_primary_documents, assemble_primary_documents,
};

/// Raw bytes returned by a host. The request remains the logical path;
/// `resolved_path` is a separate host-provided source location.
pub struct SourceData {
    pub bytes: Vec<u8>,
    pub resolved_path: Option<String>,
}

pub const MAX_SOURCE_PATH_BYTES: usize = ir::MAX_SOURCE_ORIGIN_PATH_BYTES;
pub const MAX_SOURCE_DOCUMENTS: usize = 1_000_000;

/// Lexical identity admission only: this does not normalize or resolve paths.
pub fn is_valid_source_path(path: &str) -> bool {
    !path.is_empty() && !path.contains('\0') && path.len() <= MAX_SOURCE_PATH_BYTES
}
