//! Explicit physical CSV occurrences, separate from ordinary instance traversal.

use std::{iter::FusedIterator, sync::Arc};

use ir::{Instance, SchemaNode, SourceDocumentOrigin};
use thiserror::Error;

use crate::{CsvByteDecodingPolicy, CsvReadError, CsvReadOptions};

pub const MAX_CSV_SOURCE_PATH_BYTES: usize = document_source::MAX_SOURCE_PATH_BYTES;
pub const MAX_CSV_SOURCE_DOCUMENTS: usize = document_source::MAX_SOURCE_DOCUMENTS;

#[derive(Debug, Error)]
pub enum CsvSourceError {
    #[error(
        "CSV source {field} must be nonempty, NUL-free and at most {MAX_CSV_SOURCE_PATH_BYTES} UTF-8 bytes"
    )]
    Path { field: &'static str },
    #[error(transparent)]
    Read(#[from] CsvReadError),
}

#[derive(Debug)]
struct Document {
    origin: SourceDocumentOrigin,
    rows: Vec<Instance>,
}

/// One complete parsed physical document. A clone retains its identity; a new
/// parse creates a different identity even when the paths and bytes are equal.
#[derive(Clone, Debug)]
pub struct CsvSourceDocument(Arc<Document>);

impl CsvSourceDocument {
    /// No filesystem access or path normalization. The host supplies both paths.
    /// A failed parse publishes neither a document nor a prefix of its rows.
    pub fn parse_bytes(
        path: &str,
        resolved_path: Option<&str>,
        bytes: &[u8],
        schema: &SchemaNode,
        options: &CsvReadOptions,
        decoding: CsvByteDecodingPolicy,
    ) -> Result<Self, CsvSourceError> {
        validate_path(path, "path")?;
        if let Some(path) = resolved_path {
            validate_path(path, "resolved path")?;
        }
        let rows =
            crate::from_bytes_with_decoding_policy_bounded(bytes, schema, options, decoding)?;
        let origin =
            SourceDocumentOrigin::new(path, resolved_path).expect("source paths were validated");
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let Instance::Group(group) = row else {
                    unreachable!("CSV parser returns group rows")
                };
                Instance::Group(
                    group
                        .with_source_origin(origin.row(index + 1).expect("one-based row position")),
                )
            })
            .collect();
        Ok(Self(Arc::new(Document { origin, rows })))
    }

    pub fn path(&self) -> &str {
        self.0.origin.path()
    }

    pub fn resolved_path(&self) -> Option<&str> {
        self.0.origin.resolved_path()
    }

    pub fn origin(&self) -> &SourceDocumentOrigin {
        &self.0.origin
    }

    pub fn source_path(&self) -> &str {
        self.resolved_path().unwrap_or_else(|| self.path())
    }

    pub fn same_document(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn rows(&self) -> CsvDocumentRows {
        CsvDocumentRows {
            document: self.clone(),
            next: 0,
        }
    }
}

pub(crate) fn validate_path(path: &str, field: &'static str) -> Result<(), CsvSourceError> {
    if !document_source::is_valid_source_path(path) {
        Err(CsvSourceError::Path { field })
    } else {
        Ok(())
    }
}

/// An exact row occurrence retains its own physical document and local position.
/// It can outlive the enumerator or be regrouped without losing its origin.
#[derive(Clone, Debug)]
pub struct CsvRowOccurrence {
    document: CsvSourceDocument,
    index: usize,
}

impl CsvRowOccurrence {
    pub fn document(&self) -> &CsvSourceDocument {
        &self.document
    }

    pub fn value(&self) -> &Instance {
        &self.document.0.rows[self.index]
    }

    /// One-based data row position, excluding a skipped header.
    pub fn position(&self) -> usize {
        self.index + 1
    }

    pub fn same_occurrence(&self, other: &Self) -> bool {
        self.index == other.index && self.document.same_document(&other.document)
    }
}

pub struct CsvDocumentRows {
    document: CsvSourceDocument,
    next: usize,
}

impl Iterator for CsvDocumentRows {
    type Item = CsvRowOccurrence;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next == self.document.0.rows.len() {
            return None;
        }
        let row = CsvRowOccurrence {
            document: self.document.clone(),
            index: self.next,
        };
        self.next += 1;
        Some(row)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.document.0.rows.len() - self.next;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CsvDocumentRows {}
impl FusedIterator for CsvDocumentRows {}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CsvSourceLoadError<E> {
    #[error("CSV source document limit exceeds the hard maximum")]
    InvalidLimit,
    #[error("CSV source exceeds the {maximum}-document limit")]
    Documents { maximum: usize },
    #[error("CSV source host: {0}")]
    Host(#[source] E),
}

/// Explicit cross-document row enumeration. It does not change `DocumentSet`
/// traversal, invoke a mapper per file, or merge rows' physical identities.
///
/// Requests and loads are lazy and ordered. Empty documents still consume a
/// document slot. The first reached error or panic permanently ends this run;
/// the next document is never requested. The host owns enumeration, path
/// resolution, byte loading and parsing through `CsvSourceDocument::parse_bytes`.
/// Count and per-document parser limits are not a retained-memory ceiling.
pub struct CsvSourceRows<I, F> {
    requests: I,
    load: F,
    maximum: usize,
    count: usize,
    current: Option<CsvDocumentRows>,
    finished: bool,
}

impl<I, F, E> CsvSourceRows<I, F>
where
    I: Iterator<Item = Result<String, E>>,
    F: FnMut(&str) -> Result<CsvSourceDocument, E>,
{
    pub fn new(requests: I, load: F, maximum: usize) -> Result<Self, CsvSourceLoadError<E>> {
        if maximum > MAX_CSV_SOURCE_DOCUMENTS {
            return Err(CsvSourceLoadError::InvalidLimit);
        }
        Ok(Self {
            requests,
            load,
            maximum,
            count: 0,
            current: None,
            finished: false,
        })
    }
}

impl<I, F, E> Iterator for CsvSourceRows<I, F>
where
    I: Iterator<Item = Result<String, E>>,
    F: FnMut(&str) -> Result<CsvSourceDocument, E>,
{
    type Item = Result<CsvRowOccurrence, CsvSourceLoadError<E>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        self.finished = true;
        loop {
            if let Some(row) = self.current.as_mut().and_then(Iterator::next) {
                self.finished = false;
                return Some(Ok(row));
            }
            self.current = None;
            let request = match self.requests.next()? {
                Ok(request) => request,
                Err(error) => return Some(Err(CsvSourceLoadError::Host(error))),
            };
            if self.count == self.maximum {
                return Some(Err(CsvSourceLoadError::Documents {
                    maximum: self.maximum,
                }));
            }
            match (self.load)(&request) {
                Ok(document) => {
                    self.count += 1;
                    self.current = Some(document.rows());
                }
                Err(error) => return Some(Err(CsvSourceLoadError::Host(error))),
            }
        }
    }
}

impl<I, F, E> FusedIterator for CsvSourceRows<I, F>
where
    I: Iterator<Item = Result<String, E>>,
    F: FnMut(&str) -> Result<CsvSourceDocument, E>,
{
}
