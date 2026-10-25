use crate::{PrimaryAcquisitionError, PrimaryAssemblyError, RuntimeError};
use ir::{Instance, SchemaNode};
use mapping::{EdiBoundaryKind, FormatOptions};
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum EdiBoundaryError {
    Descriptor(String),
    Format(format_edi::EdiFormatError),
    InputTooLarge { bytes: usize, maximum: usize },
    Execution(RuntimeError),
}
impl fmt::Display for EdiBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Descriptor(e) => f.write_str(e),
            Self::Format(e) => e.fmt(f),
            Self::InputTooLarge { bytes, maximum } => {
                write!(f, "EDI input is {bytes} bytes; maximum is {maximum}")
            }
            Self::Execution(e) => e.fmt(f),
        }
    }
}
impl Error for EdiBoundaryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::Descriptor(_) | Self::InputTooLarge { .. } => None,
        }
    }
}

/// Charges each document before parsing, without retaining failed charges.
#[derive(Default)]
pub struct EdiInputBudget {
    total: usize,
}
impl EdiInputBudget {
    pub fn charge(&mut self, bytes: usize) -> Result<(), EdiBoundaryError> {
        const DOCUMENT: usize = 64 * 1024 * 1024;
        const TOTAL: usize = 256 * 1024 * 1024;
        if bytes > DOCUMENT {
            return Err(EdiBoundaryError::InputTooLarge {
                bytes,
                maximum: DOCUMENT,
            });
        }
        let next = self.total.saturating_add(bytes);
        if next > TOTAL {
            return Err(EdiBoundaryError::InputTooLarge {
                bytes: next,
                maximum: TOTAL,
            });
        }
        self.total = next;
        Ok(())
    }
}

pub struct EdiDocumentBoundary {
    schema: SchemaNode,
    options: FormatOptions,
    kind: EdiBoundaryKind,
}
impl EdiDocumentBoundary {
    pub fn input(
        schema: &str,
        options: &str,
        kind: EdiBoundaryKind,
    ) -> Result<Self, EdiBoundaryError> {
        let boundary = Self::new(schema, options, kind)?;
        format_edi::document::validate_input(&boundary.schema, &boundary.options, kind)
            .map_err(EdiBoundaryError::Format)?;
        Ok(boundary)
    }
    pub fn new(
        schema: &str,
        options: &str,
        kind: EdiBoundaryKind,
    ) -> Result<Self, EdiBoundaryError> {
        if schema.len() > 1024 * 1024 || options.len() > 1024 * 1024 {
            return Err(EdiBoundaryError::Descriptor(
                "EDI descriptor exceeds 1 MiB".into(),
            ));
        }
        let schema = serde_json::from_str(schema)
            .map_err(|e| EdiBoundaryError::Descriptor(e.to_string()))?;
        let options = serde_json::from_str(options)
            .map_err(|e| EdiBoundaryError::Descriptor(e.to_string()))?;
        format_edi::document::validate(&schema, &options, kind)
            .map_err(EdiBoundaryError::Format)?;
        Ok(Self {
            schema,
            options,
            kind,
        })
    }
    pub fn parse(&self, bytes: &[u8]) -> Result<Instance, EdiBoundaryError> {
        format_edi::document::parse(&self.schema, &self.options, self.kind, bytes)
            .map_err(EdiBoundaryError::Format)
    }
    pub fn serialize(
        &self,
        instance: &Instance,
        datetime: &str,
    ) -> Result<Vec<u8>, EdiBoundaryError> {
        format_edi::document::serialize(&self.schema, &self.options, self.kind, instance, datetime)
            .map_err(EdiBoundaryError::Format)
    }
}

#[derive(Debug)]
pub enum PrimaryEdiDocumentError<E> {
    Boundary(EdiBoundaryError),
    Acquisition(PrimaryAcquisitionError<RuntimeError, E, EdiBoundaryError>),
    Assembly(PrimaryAssemblyError),
}
impl<E: fmt::Display> fmt::Display for PrimaryEdiDocumentError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(e) => e.fmt(f),
            Self::Acquisition(e) => e.fmt(f),
            Self::Assembly(e) => e.fmt(f),
        }
    }
}
impl<E: Error + 'static> Error for PrimaryEdiDocumentError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Boundary(error) => Some(error),
            Self::Acquisition(error) => Some(error),
            Self::Assembly(error) => Some(error),
        }
    }
}
pub use mapping::EdiBoundaryKind as PrimaryEdiKind;

/// Opt-in wire envelope validation for adapters that require complete EDIFACT.
pub fn validate_edifact_envelope(bytes: &[u8]) -> Result<(), EdiBoundaryError> {
    EdiInputBudget::default().charge(bytes.len())?;
    let text = std::str::from_utf8(bytes).map_err(|_| {
        EdiBoundaryError::Format(format_edi::EdiFormatError::NotEdifact("input is not UTF-8"))
    })?;
    format_edi::edifact::validate_envelope(text).map_err(EdiBoundaryError::Format)
}
