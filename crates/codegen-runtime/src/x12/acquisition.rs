//! Lazy, descriptor-bound X12 acquisition. Hosts own I/O and input cleanup.
use super::*;
use std::iter::FusedIterator;
#[cfg(test)]
#[path = "acquisition_tests.rs"]
mod tests;

pub const MAX_X12_SOURCE_DOCUMENTS: usize = 100_000;

pub struct ParsedX12Source {
    descriptor: String,
    document: ir::DocumentMember,
}

/// Call inside the host loader before closing its input. The returned value is
/// immutable and bound to the exact descriptor that performed the parse.
pub fn parse_mapforce_x12_source_document(
    descriptor_text: &str,
    identity: &str,
    bytes: &[u8],
) -> Result<ParsedX12Source, X12BoundaryError> {
    let value = super::parse_mapforce_x12_source(descriptor_text, bytes)?;
    let document = ir::DocumentMember::new(identity, value)
        .ok_or_else(|| descriptor("invalid source document identity"))?;
    Ok(ParsedX12Source {
        descriptor: descriptor_text.into(),
        document,
    })
}

#[derive(Debug)]
pub enum X12AcquisitionError<F, E> {
    Boundary(X12BoundaryError),
    InvalidLimit,
    Filename(F),
    Selection(E),
    Request(E),
    Documents,
    Load { path: String, source: E },
    DescriptorMismatch { path: String },
}
impl<F: fmt::Display, E: fmt::Display> fmt::Display for X12AcquisitionError<F, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::InvalidLimit => f.write_str("invalid X12 source document limit"),
            Self::Filename(error) => write!(f, "X12 source filename: {error}"),
            Self::Selection(error) => write!(f, "X12 source selection: {error}"),
            Self::Request(error) => write!(f, "X12 source request: {error}"),
            Self::Documents => f.write_str("X12 source document limit reached"),
            Self::Load { path, source } => write!(f, "X12 source load {path:?}: {source}"),
            Self::DescriptorMismatch { path } => {
                write!(f, "X12 source {path:?} used a different descriptor")
            }
        }
    }
}
impl<F: Error + 'static, E: Error + 'static> Error for X12AcquisitionError<F, E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Boundary(e) => Some(e),
            Self::Filename(e) => Some(e),
            Self::Selection(e) | Self::Request(e) | Self::Load { source: e, .. } => Some(e),
            _ => None,
        }
    }
}
type Requests<'a, E> = Box<dyn Iterator<Item = Result<String, E>> + 'a>;
type Selector<'a, E> = Box<dyn FnOnce(&str) -> Result<Requests<'a, E>, E> + 'a>;
type Loader<'a, E> = Box<dyn FnMut(&str) -> Result<ParsedX12Source, E> + 'a>;

/// One terminal-on-error source attempt. No filename, selection or load callback
/// runs at construction. A loader returns only after parsing and closing input.
pub struct X12SourceAcquisition<'a, F, E> {
    filename: Option<Box<dyn FnOnce() -> Result<String, F> + 'a>>,
    select: Option<Selector<'a, E>>,
    requests: Option<Requests<'a, E>>,
    load: Loader<'a, E>,
    descriptor: &'a str,
    maximum: usize,
    count: usize,
    finished: bool,
}
impl<'a, F: 'a, E: 'a> X12SourceAcquisition<'a, F, E> {
    pub fn new<I: Iterator<Item = Result<String, E>> + 'a>(
        descriptor_text: &'a str,
        filename: impl FnOnce() -> Result<String, F> + 'a,
        select: impl FnOnce(&str) -> Result<I, E> + 'a,
        load: impl FnMut(&str) -> Result<ParsedX12Source, E> + 'a,
        maximum: usize,
    ) -> Result<Self, X12AcquisitionError<F, E>> {
        source_mapforce::source_profile(descriptor_text).map_err(X12AcquisitionError::Boundary)?;
        if maximum > MAX_X12_SOURCE_DOCUMENTS {
            return Err(X12AcquisitionError::InvalidLimit);
        }
        Ok(Self {
            filename: Some(Box::new(filename)),
            select: Some(Box::new(move |path| {
                select(path).map(|items| Box::new(items) as Requests<'a, E>)
            })),
            requests: None,
            load: Box::new(load),
            descriptor: descriptor_text,
            maximum,
            count: 0,
            finished: false,
        })
    }
}
impl<F, E> Iterator for X12SourceAcquisition<'_, F, E> {
    type Item = Result<ir::DocumentMember, X12AcquisitionError<F, E>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        self.finished = true;
        if let Some(filename) = self.filename.take() {
            let path = match filename() {
                Ok(path) => path,
                Err(error) => return Some(Err(X12AcquisitionError::Filename(error))),
            };
            let select = self.select.take().expect("one source selector");
            self.requests = Some(match select(&path) {
                Ok(requests) => requests,
                Err(error) => return Some(Err(X12AcquisitionError::Selection(error))),
            });
        }
        let path = match self
            .requests
            .as_mut()
            .expect("initialized source requests")
            .next()?
        {
            Ok(path) => path,
            Err(error) => return Some(Err(X12AcquisitionError::Request(error))),
        };
        if self.count == self.maximum {
            return Some(Err(X12AcquisitionError::Documents));
        }
        let source = match (self.load)(&path) {
            Ok(value) => value,
            Err(source) => return Some(Err(X12AcquisitionError::Load { path, source })),
        };
        if source.descriptor != self.descriptor {
            return Some(Err(X12AcquisitionError::DescriptorMismatch { path }));
        }
        self.count += 1;
        self.finished = false;
        Some(Ok(source.document))
    }
}
impl<F, E> FusedIterator for X12SourceAcquisition<'_, F, E> {}
