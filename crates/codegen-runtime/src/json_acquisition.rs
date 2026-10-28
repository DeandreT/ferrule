//! Reached primary JSON acquisition composed with ordered typed projection.

use std::{convert::Infallible, error::Error, fmt, iter::FusedIterator};

use ir::Instance;
use mapping::JsonSourceShape;

use crate::{
    JsonProjectionError, JsonProjectionLimits, JsonSourceDocument, MAX_JSON_SOURCE_DOCUMENTS,
    project_json_source_document,
};

#[derive(Debug)]
pub enum JsonAcquisitionError<F, E> {
    Projection(JsonProjectionError),
    Grouping(crate::JsonGroupingError),
    InvalidLimit,
    Filename(F),
    Selection(E),
    Request(E),
    Documents,
    Load { path: String, source: E },
}

impl<F: fmt::Display, E: fmt::Display> fmt::Display for JsonAcquisitionError<F, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Projection(error) => error.fmt(f),
            Self::Grouping(error) => error.fmt(f),
            Self::InvalidLimit => f.write_str("invalid JSON acquisition document limit"),
            Self::Filename(error) => write!(f, "JSON acquisition filename: {error}"),
            Self::Selection(error) => write!(f, "JSON acquisition selection: {error}"),
            Self::Request(error) => write!(f, "JSON acquisition request: {error}"),
            Self::Documents => f.write_str("JSON acquisition document limit reached"),
            Self::Load { path, source } => write!(f, "JSON acquisition load {path:?}: {source}"),
        }
    }
}

impl<F: Error + 'static, E: Error + 'static> Error for JsonAcquisitionError<F, E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Projection(error) => Some(error),
            Self::Grouping(error) => Some(error),
            Self::Filename(error) => Some(error),
            Self::Selection(error) | Self::Request(error) | Self::Load { source: error, .. } => {
                Some(error)
            }
            _ => None,
        }
    }
}

type Requests<'a, E> = Box<dyn Iterator<Item = Result<String, E>> + 'a>;
type Selector<'a, E> = Box<dyn FnOnce(&str) -> Result<Requests<'a, E>, E> + 'a>;
type Loader<'a, E> = Box<dyn FnMut(&str) -> Result<JsonSourceDocument, E> + 'a>;

/// One terminal-on-error acquisition attempt. Filename evaluation, selection
/// and loading are deferred to `next`. The host finishes parsing and input
/// cleanup before returning a document; this type performs no file access.
pub struct JsonSourceAcquisition<'a, F, E> {
    filename: Option<Box<dyn FnOnce() -> Result<String, F> + 'a>>,
    select: Option<Selector<'a, E>>,
    requests: Option<Requests<'a, E>>,
    load: Loader<'a, E>,
    shape: &'a JsonSourceShape,
    grouping: Option<&'a mapping::JsonGroupingPreparation>,
    limits: JsonProjectionLimits,
    maximum: usize,
    count: usize,
    finished: bool,
}

impl<'a, F: 'a, E: 'a> JsonSourceAcquisition<'a, F, E> {
    pub fn new<I: Iterator<Item = Result<String, E>> + 'a>(
        filename: impl FnOnce() -> Result<String, F> + 'a,
        select: impl FnOnce(&str) -> Result<I, E> + 'a,
        load: impl FnMut(&str) -> Result<JsonSourceDocument, E> + 'a,
        shape: &'a JsonSourceShape,
        limits: JsonProjectionLimits,
        maximum: usize,
    ) -> Result<Self, JsonAcquisitionError<F, E>> {
        validate(shape, limits, maximum)?;
        Ok(Self {
            filename: Some(Box::new(filename)),
            select: Some(Box::new(move |path| {
                select(path).map(|requests| Box::new(requests) as Requests<'a, E>)
            })),
            requests: None,
            load: Box::new(load),
            shape,
            grouping: None,
            limits,
            maximum,
            count: 0,
            finished: false,
        })
    }

    pub fn with_grouping_preparation(
        mut self,
        grouping: &'a mapping::JsonGroupingPreparation,
    ) -> Result<Self, JsonAcquisitionError<F, E>> {
        grouping.validate(self.shape).map_err(|error| {
            JsonAcquisitionError::Grouping(crate::JsonGroupingError::Plan(error))
        })?;
        self.grouping = Some(grouping);
        Ok(self)
    }
}

impl<'a, E: 'a> JsonSourceAcquisition<'a, Infallible, E> {
    /// Load an already-selected ordered request stream. Selection remains lazy:
    /// duplicates are separate requests, and a failure terminates the stream.
    /// One document limit covers every request rather than resetting per name.
    pub fn from_requests<I: Iterator<Item = Result<String, E>> + 'a>(
        requests: I,
        load: impl FnMut(&str) -> Result<JsonSourceDocument, E> + 'a,
        shape: &'a JsonSourceShape,
        limits: JsonProjectionLimits,
        maximum: usize,
    ) -> Result<Self, JsonAcquisitionError<Infallible, E>> {
        validate(shape, limits, maximum)?;
        Ok(Self {
            filename: None,
            select: None,
            requests: Some(Box::new(requests)),
            load: Box::new(load),
            shape,
            grouping: None,
            limits,
            maximum,
            count: 0,
            finished: false,
        })
    }
}

fn validate<F, E>(
    shape: &JsonSourceShape,
    limits: JsonProjectionLimits,
    maximum: usize,
) -> Result<(), JsonAcquisitionError<F, E>> {
    shape
        .validate()
        .map_err(|error| JsonAcquisitionError::Projection(JsonProjectionError::Plan(error)))?;
    if limits.values > crate::MAX_JSON_PROJECTED_VALUES
        || limits.name_bytes > crate::MAX_JSON_PROJECTED_NAME_BYTES
    {
        return Err(JsonAcquisitionError::Projection(
            JsonProjectionError::InvalidLimits,
        ));
    }
    if maximum > MAX_JSON_SOURCE_DOCUMENTS {
        return Err(JsonAcquisitionError::InvalidLimit);
    }
    Ok(())
}

impl<F, E> Iterator for JsonSourceAcquisition<'_, F, E> {
    type Item = Result<Instance, JsonAcquisitionError<F, E>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        self.finished = true;
        if let Some(filename) = self.filename.take() {
            let path = match filename() {
                Ok(path) => path,
                Err(error) => return Some(Err(JsonAcquisitionError::Filename(error))),
            };
            let select = self.select.take().expect("one source selector");
            self.requests = Some(match select(&path) {
                Ok(requests) => requests,
                Err(error) => return Some(Err(JsonAcquisitionError::Selection(error))),
            });
        }
        let path = match self
            .requests
            .as_mut()
            .expect("initialized requests")
            .next()?
        {
            Ok(path) => path,
            Err(error) => return Some(Err(JsonAcquisitionError::Request(error))),
        };
        if self.count == self.maximum {
            return Some(Err(JsonAcquisitionError::Documents));
        }
        let document = match (self.load)(&path) {
            Ok(document) => document,
            Err(source) => return Some(Err(JsonAcquisitionError::Load { path, source })),
        };
        let projected = match project_json_source_document(self.shape, &document, self.limits) {
            Ok(value) => value,
            Err(error) => return Some(Err(JsonAcquisitionError::Projection(error))),
        };
        let projected = if let Some(grouping) = self.grouping {
            match crate::prepare_json_grouping_document(
                grouping,
                self.shape,
                projected,
                self.limits,
            ) {
                Ok(value) => value,
                Err(error) => return Some(Err(JsonAcquisitionError::Grouping(error))),
            }
        } else {
            projected
        };
        self.count += 1;
        self.finished = false;
        Some(Ok(projected))
    }
}

impl<F, E> FusedIterator for JsonSourceAcquisition<'_, F, E> {}
