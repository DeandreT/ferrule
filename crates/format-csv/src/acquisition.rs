//! Lazy primary-filename acquisition with an explicit host selection boundary.

use std::{error::Error, fmt, iter::FusedIterator};

use ir::SchemaNode;

use crate::{
    CsvByteDecodingPolicy, CsvDocumentRows, CsvReadOptions, CsvRowOccurrence, CsvSourceDocument,
    CsvSourceError, MAX_CSV_SOURCE_DOCUMENTS,
};

/// Raw bytes returned by the host for one selected request. The request itself
/// remains the logical path; the host may separately supply a resolved location.
pub struct CsvSourceData {
    pub bytes: Vec<u8>,
    pub resolved_path: Option<String>,
}

#[derive(Debug)]
pub enum CsvAcquisitionError<F, E> {
    InvalidLimit {
        maximum: usize,
    },
    Filename(F),
    Selection(E),
    Request(E),
    Documents {
        maximum: usize,
    },
    Load {
        path: String,
        source: E,
    },
    Input {
        path: String,
        source: CsvSourceError,
    },
}

impl<F: fmt::Display, E: fmt::Display> fmt::Display for CsvAcquisitionError<F, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit { maximum } => write!(
                f,
                "CSV acquisition document limit {maximum} exceeds the hard maximum"
            ),
            Self::Filename(error) => write!(f, "CSV acquisition filename: {error}"),
            Self::Selection(error) => write!(f, "CSV acquisition selection: {error}"),
            Self::Request(error) => write!(f, "CSV acquisition request: {error}"),
            Self::Documents { maximum } => {
                write!(f, "CSV acquisition exceeds the {maximum}-document limit")
            }
            Self::Load { path, source } => write!(f, "CSV acquisition load {path:?}: {source}"),
            Self::Input { path, source } => write!(f, "CSV acquisition input {path:?}: {source}"),
        }
    }
}

impl<F: Error + 'static, E: Error + 'static> Error for CsvAcquisitionError<F, E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Filename(error) => Some(error),
            Self::Selection(error) | Self::Request(error) | Self::Load { source: error, .. } => {
                Some(error)
            }
            Self::Input { source, .. } => Some(source),
            Self::InvalidLimit { .. } | Self::Documents { .. } => None,
        }
    }
}

type Requests<'a, E> = Box<dyn Iterator<Item = Result<String, E>> + 'a>;
type Selector<'a, E> = Box<dyn FnOnce(&str) -> Result<Requests<'a, E>, E> + 'a>;
type Loader<'a, E> = Box<dyn FnMut(&str) -> Result<CsvSourceData, E> + 'a>;

/// One acquisition attempt, not a mapping execution or filesystem policy.
/// Nothing is evaluated, selected or loaded until the first `next`. The first
/// reached failure or panic ends this attempt permanently. A failed document
/// publishes none of its rows; already returned rows retain their identities.
pub struct CsvSourceAcquisition<'a, F, E> {
    filename: Option<Box<dyn FnOnce() -> Result<String, F> + 'a>>,
    select: Option<Selector<'a, E>>,
    load: Loader<'a, E>,
    requests: Option<Requests<'a, E>>,
    schema: &'a SchemaNode,
    options: CsvReadOptions,
    decoding: CsvByteDecodingPolicy,
    maximum: usize,
    count: usize,
    current: Option<CsvDocumentRows>,
    finished: bool,
}

impl<'a, F: 'a, E: 'a> CsvSourceAcquisition<'a, F, E> {
    pub fn new<I, Evaluate, Select, Load>(
        filename: Evaluate,
        select: Select,
        load: Load,
        schema: &'a SchemaNode,
        options: CsvReadOptions,
        decoding: CsvByteDecodingPolicy,
        maximum: usize,
    ) -> Result<Self, CsvAcquisitionError<F, E>>
    where
        I: Iterator<Item = Result<String, E>> + 'a,
        Evaluate: FnOnce() -> Result<String, F> + 'a,
        Select: FnOnce(&str) -> Result<I, E> + 'a,
        Load: FnMut(&str) -> Result<CsvSourceData, E> + 'a,
    {
        if maximum > MAX_CSV_SOURCE_DOCUMENTS {
            return Err(CsvAcquisitionError::InvalidLimit { maximum });
        }
        Ok(Self {
            filename: Some(Box::new(filename)),
            select: Some(Box::new(move |name| {
                select(name).map(|items| Box::new(items) as Requests<'a, E>)
            })),
            load: Box::new(load),
            requests: None,
            schema,
            options,
            decoding,
            maximum,
            count: 0,
            current: None,
            finished: false,
        })
    }
}

impl<F, E> Iterator for CsvSourceAcquisition<'_, F, E> {
    type Item = Result<CsvRowOccurrence, CsvAcquisitionError<F, E>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        // Also stays terminal when caller code unwinds from one of the callbacks.
        self.finished = true;
        if let Some(evaluate) = self.filename.take() {
            let filename = match evaluate() {
                Ok(filename) => filename,
                Err(error) => return Some(Err(CsvAcquisitionError::Filename(error))),
            };
            let select = self.select.take().expect("one selector for one filename");
            self.requests = match select(&filename) {
                Ok(requests) => Some(requests),
                Err(error) => return Some(Err(CsvAcquisitionError::Selection(error))),
            };
        }
        loop {
            if let Some(row) = self.current.as_mut().and_then(Iterator::next) {
                self.finished = false;
                return Some(Ok(row));
            }
            self.current = None;
            let path = match self
                .requests
                .as_mut()
                .expect("initialized request stream")
                .next()?
            {
                Ok(path) => path,
                Err(error) => return Some(Err(CsvAcquisitionError::Request(error))),
            };
            if self.count == self.maximum {
                return Some(Err(CsvAcquisitionError::Documents {
                    maximum: self.maximum,
                }));
            }
            if let Err(source) = crate::source_document::validate_path(&path, "path") {
                return Some(Err(CsvAcquisitionError::Input { path, source }));
            }
            let data = match (self.load)(&path) {
                Ok(data) => data,
                Err(source) => return Some(Err(CsvAcquisitionError::Load { path, source })),
            };
            let document = match CsvSourceDocument::parse_bytes(
                &path,
                data.resolved_path.as_deref(),
                &data.bytes,
                self.schema,
                &self.options,
                self.decoding,
            ) {
                Ok(document) => document,
                Err(source) => return Some(Err(CsvAcquisitionError::Input { path, source })),
            };
            self.count += 1;
            self.current = Some(document.rows());
        }
    }
}

impl<F, E> FusedIterator for CsvSourceAcquisition<'_, F, E> {}
