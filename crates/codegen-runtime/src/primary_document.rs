//! Failures of generated complete primary document entry points for document
//! formats. Acquisition failures happen before the mapping runs; output
//! failures include the single mapping execution.

use std::error::Error;
use std::fmt;

use crate::{
    PrimaryAcquisitionError, PrimaryAssemblyError, RuntimeError, XmlBoundaryError,
    XmlOutputSetError,
};

#[derive(Debug)]
pub enum PrimaryXmlDocumentError<E> {
    Acquisition(PrimaryAcquisitionError<RuntimeError, E, XmlBoundaryError>),
    Assembly(PrimaryAssemblyError),
    Output(XmlOutputSetError),
}

impl<E: fmt::Display> fmt::Display for PrimaryXmlDocumentError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Acquisition(error) => error.fmt(f),
            Self::Assembly(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static> Error for PrimaryXmlDocumentError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Acquisition(error) => error,
            Self::Assembly(error) => error,
            Self::Output(error) => error,
        })
    }
}
