//! Runtime-only physical source provenance, never inferred from equal data.

use std::{error::Error, fmt, sync::Arc};

pub const MAX_SOURCE_ORIGIN_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRowPathError {
    InactiveFrame,
    GroupedFrame,
    MissingOrigin,
}

impl fmt::Display for SourceRowPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InactiveFrame => "the exact source collection frame is not active",
            Self::GroupedFrame => "a grouped collection does not identify one physical source row",
            Self::MissingOrigin => "the selected row has no physical source provenance",
        })
    }
}
impl Error for SourceRowPathError {}

/// Inspect only the selected row. Equal contents, an enclosing row or a
/// document wrapper cannot supply missing provenance.
pub fn source_row_document_path(row: &crate::Instance) -> Result<&str, SourceRowPathError> {
    if matches!(row, crate::Instance::Repeated(_)) {
        return Err(SourceRowPathError::GroupedFrame);
    }
    let crate::Instance::Group(group) = row else {
        return Err(SourceRowPathError::MissingOrigin);
    };
    group
        .source_origin()
        .map(|origin| origin.document().source_path())
        .ok_or(SourceRowPathError::MissingOrigin)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceOriginError {
    Path { resolved: bool },
    ZeroPosition,
}
impl fmt::Display for SourceOriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path { resolved } => write!(
                f,
                "source {}path must be nonempty, NUL-free and at most {MAX_SOURCE_ORIGIN_PATH_BYTES} UTF-8 bytes",
                if *resolved { "resolved " } else { "" }
            ),
            Self::ZeroPosition => f.write_str("source row position must be positive"),
        }
    }
}
impl Error for SourceOriginError {}

#[derive(Debug)]
struct DocumentOrigin {
    path: String,
    resolved_path: Option<String>,
}

/// One host/reader-created document identity. Cloning preserves identity;
/// constructing a second value never merges identities by path or content.
#[derive(Debug, Clone)]
pub struct SourceDocumentOrigin(Arc<DocumentOrigin>);

impl SourceDocumentOrigin {
    pub fn new(path: &str, resolved_path: Option<&str>) -> Result<Self, SourceOriginError> {
        for (resolved, value) in [(false, Some(path)), (true, resolved_path)] {
            if value.is_some_and(|value| {
                value.is_empty()
                    || value.contains('\0')
                    || value.len() > MAX_SOURCE_ORIGIN_PATH_BYTES
            }) {
                return Err(SourceOriginError::Path { resolved });
            }
        }
        Ok(Self(Arc::new(DocumentOrigin {
            path: path.into(),
            resolved_path: resolved_path.map(str::to_owned),
        })))
    }
    pub fn path(&self) -> &str {
        &self.0.path
    }
    pub fn resolved_path(&self) -> Option<&str> {
        self.0.resolved_path.as_deref()
    }
    pub fn source_path(&self) -> &str {
        self.resolved_path().unwrap_or_else(|| self.path())
    }
    pub fn same_document(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn row(&self, position: usize) -> Result<SourceRowOrigin, SourceOriginError> {
        if position == 0 {
            return Err(SourceOriginError::ZeroPosition);
        }
        Ok(SourceRowOrigin {
            document: self.clone(),
            position,
        })
    }
}
impl PartialEq for SourceDocumentOrigin {
    fn eq(&self, other: &Self) -> bool {
        self.same_document(other)
    }
}
impl Eq for SourceDocumentOrigin {}

/// Original physical row metadata. This is provenance, not endpoint ownership,
/// schema admission or an attestation that a caller is trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRowOrigin {
    document: SourceDocumentOrigin,
    position: usize,
}
impl SourceRowOrigin {
    pub fn document(&self) -> &SourceDocumentOrigin {
        &self.document
    }
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn same_occurrence(&self, other: &Self) -> bool {
        self == other
    }
}
