//! Ordered JSON occurrence traversal and a reached, host-owned loading boundary.

use std::{fmt, iter::FusedIterator, sync::Arc};

use crate::{JsonSourceDocument, JsonSourceKind, JsonSourceValue};

pub const MAX_JSON_SOURCE_DOCUMENTS: usize = 1_000_000;

/// One exact value occurrence, retaining its document and structural parents.
/// Clones refer to the same occurrence; equal labels or content do not.
#[derive(Clone, Debug)]
pub struct JsonSourceOccurrence {
    document: Arc<JsonSourceDocument>,
    path: Vec<usize>,
}

impl JsonSourceOccurrence {
    /// Establish a new document occurrence, even if its label/content repeat.
    pub fn new(document: JsonSourceDocument) -> Self {
        Self {
            document: Arc::new(document),
            path: Vec::new(),
        }
    }

    pub fn document(&self) -> &JsonSourceDocument {
        &self.document
    }

    pub fn value(&self) -> &JsonSourceValue {
        let mut value = self.document.root();
        for &index in &self.path {
            value = if value.kind() == JsonSourceKind::Object {
                &value.members()[index].1
            } else {
                &value.items()[index]
            };
        }
        value
    }

    pub fn same_occurrence(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.document, &other.document) && self.path == other.path
    }

    pub fn parent(&self) -> Option<Self> {
        let mut parent = self.clone();
        parent.path.pop()?;
        Some(parent)
    }

    /// Zero or one occurrence, without conversion or an additional parent frame.
    pub fn of_kind(&self, kind: JsonSourceKind) -> Option<Self> {
        (self.value().kind() == kind).then(|| self.clone())
    }

    /// All exact decoded-name matches in source order. Other kinds yield none.
    pub fn members_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Self> + 'a {
        self.value()
            .members()
            .iter()
            .enumerate()
            .filter(move |(_, (key, _))| key == name)
            .map(|(index, _)| self.child(index))
    }

    /// Array items in source order. Other kinds yield none.
    pub fn items(&self) -> impl Iterator<Item = Self> + '_ {
        (0..self.value().items().len()).map(|index| self.child(index))
    }

    fn child(&self, index: usize) -> Self {
        let mut child = self.clone();
        child.path.push(index);
        child
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum JsonSourceLoadError<E> {
    InvalidLimit,
    Documents { maximum: usize },
    Host(E),
}

impl<E: fmt::Display> fmt::Display for JsonSourceLoadError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => {
                f.write_str("JSON source document limit exceeds the hard maximum")
            }
            Self::Documents { maximum } => {
                write!(f, "JSON source exceeds the {maximum}-document limit")
            }
            Self::Host(error) => write!(f, "JSON source host: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for JsonSourceLoadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            _ => None,
        }
    }
}

/// A single-pass, ordered, lazy document sequence. Neither enumeration nor loading
/// happens at construction. The first error ends the sequence permanently.
///
/// The host owns request/path policy, parsing, any required validation and input
/// cleanup. It must complete these before returning a document. This boundary
/// never opens files or silently substitutes strict schema projection. Per-document
/// parser limits and this load-count limit are not a retained-memory ceiling.
pub struct JsonSourceDocuments<I, F> {
    requests: I,
    load: F,
    maximum: usize,
    count: usize,
    finished: bool,
}

impl<I, F, E> JsonSourceDocuments<I, F>
where
    I: Iterator<Item = Result<String, E>>,
    F: FnMut(&str) -> Result<JsonSourceDocument, E>,
{
    pub fn new(requests: I, load: F, maximum: usize) -> Result<Self, JsonSourceLoadError<E>> {
        if maximum > MAX_JSON_SOURCE_DOCUMENTS {
            return Err(JsonSourceLoadError::InvalidLimit);
        }
        Ok(Self {
            requests,
            load,
            maximum,
            count: 0,
            finished: false,
        })
    }
}

impl<I, F, E> Iterator for JsonSourceDocuments<I, F>
where
    I: Iterator<Item = Result<String, E>>,
    F: FnMut(&str) -> Result<JsonSourceDocument, E>,
{
    type Item = Result<JsonSourceOccurrence, JsonSourceLoadError<E>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        // Stay terminal after exhaustion or any reached error, including a host
        // panic caught outside the iterator. Only a successful load reopens it.
        self.finished = true;
        let request = match self.requests.next()? {
            Ok(request) => request,
            Err(error) => return Some(Err(JsonSourceLoadError::Host(error))),
        };
        if self.count == self.maximum {
            return Some(Err(JsonSourceLoadError::Documents {
                maximum: self.maximum,
            }));
        }
        match (self.load)(&request) {
            Ok(document) => {
                self.count += 1;
                self.finished = false;
                Some(Ok(JsonSourceOccurrence::new(document)))
            }
            Err(error) => Some(Err(JsonSourceLoadError::Host(error))),
        }
    }
}

impl<I, F, E> FusedIterator for JsonSourceDocuments<I, F>
where
    I: Iterator<Item = Result<String, E>>,
    F: FnMut(&str) -> Result<JsonSourceDocument, E>,
{
}
