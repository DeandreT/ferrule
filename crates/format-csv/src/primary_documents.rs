//! Complete primary document acquisition: one filename, one host selection,
//! every selected document parsed before any mapping runs.

use std::error::Error;
use std::fmt;

use ir::{DocumentMember, Instance, SchemaNode};
use mapping::{PrimaryDocumentShape, PrimaryFileSelection};

use crate::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition,
    CsvSourceData, MAX_CSV_SOURCE_DOCUMENTS,
};

/// The host side of primary acquisition. `select` interprets the evaluated
/// filename under the plan's selection and returns logical requests in load
/// order; `load` returns the bytes of one request. Neither parses or maps.
/// [`crate::LocalPrimaryFiles`] is the explicit local filesystem host.
pub trait PrimarySourceHost {
    type Error;

    fn select(
        &self,
        filename: &str,
        selection: PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error>;

    fn load(&self, request: &str) -> Result<CsvSourceData, Self::Error>;
}

/// A stage-preserving failure of primary document acquisition: `F` is the
/// filename failure, `E` the host's and `P` the document parser's.
#[derive(Debug)]
pub enum PrimaryAcquisitionError<F, E, P> {
    InvalidLimit {
        maximum: usize,
    },
    Filename(F),
    Selection(E),
    /// A direct filename selected other than exactly one request.
    DirectSelection {
        found: usize,
    },
    /// A request that cannot name a document: empty, NUL or over-long.
    Request {
        path: String,
    },
    Documents {
        maximum: usize,
    },
    Load {
        path: String,
        source: E,
    },
    Input {
        path: String,
        source: P,
    },
}

impl<F: fmt::Display, E: fmt::Display, P: fmt::Display> fmt::Display
    for PrimaryAcquisitionError<F, E, P>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit { maximum } => write!(
                f,
                "primary document limit {maximum} exceeds the hard maximum"
            ),
            Self::Filename(error) => write!(f, "primary document filename: {error}"),
            Self::Selection(error) => write!(f, "primary document selection: {error}"),
            Self::DirectSelection { found } => write!(
                f,
                "a direct primary filename selected {found} documents instead of one"
            ),
            Self::Request { path } => {
                write!(f, "primary document request {path:?} is not a usable path")
            }
            Self::Documents { maximum } => {
                write!(f, "primary documents exceed the {maximum}-document limit")
            }
            Self::Load { path, source } => {
                write!(f, "primary document load {path:?}: {source}")
            }
            Self::Input { path, source } => {
                write!(f, "primary document input {path:?}: {source}")
            }
        }
    }
}

impl<F, E, P> Error for PrimaryAcquisitionError<F, E, P>
where
    F: Error + 'static,
    E: Error + 'static,
    P: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Filename(error) => Some(error),
            Self::Selection(error) | Self::Load { source: error, .. } => Some(error),
            Self::Input { source, .. } => Some(source),
            Self::InvalidLimit { .. }
            | Self::DirectSelection { .. }
            | Self::Request { .. }
            | Self::Documents { .. } => None,
        }
    }
}

/// One parsed document with the request that named it and the host's
/// resolved location, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimaryDocument<T> {
    pub request: String,
    pub resolved_path: Option<String>,
    pub value: T,
}

/// Evaluate the filename once, select once, then load and parse each request
/// in order. The first failure ends acquisition before any later request is
/// loaded; every document is parsed before the caller maps anything.
pub fn acquire_primary_documents<F, H: PrimarySourceHost, T, P>(
    filename: impl FnOnce() -> Result<String, F>,
    selection: PrimaryFileSelection,
    host: &H,
    maximum: usize,
    mut parse: impl FnMut(&str, CsvSourceData) -> Result<T, P>,
) -> Result<Vec<PrimaryDocument<T>>, PrimaryAcquisitionError<F, H::Error, P>> {
    if maximum > MAX_CSV_SOURCE_DOCUMENTS {
        return Err(PrimaryAcquisitionError::InvalidLimit { maximum });
    }
    let filename = filename().map_err(PrimaryAcquisitionError::Filename)?;
    let requests = host
        .select(&filename, selection)
        .map_err(PrimaryAcquisitionError::Selection)?;
    if selection.is_direct() && requests.len() != 1 {
        return Err(PrimaryAcquisitionError::DirectSelection {
            found: requests.len(),
        });
    }
    let mut documents = Vec::new();
    for request in requests {
        if documents.len() == maximum {
            return Err(PrimaryAcquisitionError::Documents { maximum });
        }
        if crate::source_document::validate_path(&request, "path").is_err() {
            return Err(PrimaryAcquisitionError::Request { path: request });
        }
        let data = match host.load(&request) {
            Ok(data) => data,
            Err(source) => {
                return Err(PrimaryAcquisitionError::Load {
                    path: request,
                    source,
                });
            }
        };
        let resolved_path = data.resolved_path.clone();
        let value = match parse(&request, data) {
            Ok(value) => value,
            Err(source) => {
                return Err(PrimaryAcquisitionError::Input {
                    path: request,
                    source,
                });
            }
        };
        documents.push(PrimaryDocument {
            request,
            resolved_path,
            value,
        });
    }
    Ok(documents)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryAssemblyError {
    /// A row format's document did not parse to rows.
    NotRows { path: String },
    /// A direct route did not select exactly one document.
    DocumentCount { found: usize },
    /// A document-set member needs a non-empty path and location.
    Member { path: String },
}

impl fmt::Display for PrimaryAssemblyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRows { path } => write!(f, "primary document {path:?} has no rows"),
            Self::DocumentCount { found } => write!(
                f,
                "a direct primary document route needs one document, found {found}"
            ),
            Self::Member { path } => {
                write!(f, "primary document {path:?} cannot join a document set")
            }
        }
    }
}

impl Error for PrimaryAssemblyError {}

/// The one mapping source of the selected documents: every row in selection
/// then row order, the one direct document, or a document set whose members
/// keep each request as their path and the host's location as their source.
pub fn assemble_primary_documents(
    shape: PrimaryDocumentShape,
    documents: Vec<PrimaryDocument<Instance>>,
) -> Result<Instance, PrimaryAssemblyError> {
    match shape {
        PrimaryDocumentShape::Rows => {
            let mut rows = Vec::new();
            for document in documents {
                let Instance::Repeated(items) = document.value else {
                    return Err(PrimaryAssemblyError::NotRows {
                        path: document.request,
                    });
                };
                rows.extend(items);
            }
            Ok(Instance::Repeated(rows))
        }
        PrimaryDocumentShape::Document => {
            let found = documents.len();
            let mut documents = documents.into_iter();
            match (documents.next(), documents.next()) {
                (Some(document), None) => Ok(document.value),
                _ => Err(PrimaryAssemblyError::DocumentCount { found }),
            }
        }
        PrimaryDocumentShape::DocumentSet => documents
            .into_iter()
            .map(|document| {
                let member = match document.resolved_path {
                    Some(resolved) => {
                        DocumentMember::new_source(&document.request, resolved, document.value)
                    }
                    None => DocumentMember::new(&document.request, document.value),
                };
                member.ok_or(PrimaryAssemblyError::Member {
                    path: document.request,
                })
            })
            .collect::<Result<_, _>>()
            .map(Instance::DocumentSet),
    }
}

/// Evaluate the filename, select, then load and parse every selected document
/// in order. The first failure ends acquisition before any later request is
/// loaded. Success is one repeated row instance across all documents, each row
/// retaining its own document origin, for a single mapping execution.
pub fn acquire_primary_csv_rows<'a, F: 'a, H: PrimarySourceHost>(
    filename: impl FnOnce() -> Result<String, F> + 'a,
    selection: PrimaryFileSelection,
    host: &'a H,
    schema: &'a SchemaNode,
    options: CsvReadOptions,
    decoding: CsvByteDecodingPolicy,
    maximum: usize,
) -> Result<Instance, CsvAcquisitionError<F, H::Error>>
where
    H::Error: 'a,
{
    let rows = CsvSourceAcquisition::new(
        filename,
        move |name| {
            host.select(name, selection)
                .map(|requests| requests.into_iter().map(Ok))
        },
        move |request| host.load(request),
        schema,
        options,
        decoding,
        maximum,
    )?;
    let mut values = Vec::new();
    for row in rows {
        // The parsed document keeps its rows; the mapping input owns copies.
        values.push(row?.value().clone());
    }
    Ok(Instance::Repeated(values))
}
