//! Admission of complete primary document entry points for document formats.

use std::fmt;

use mapping::{
    PrimaryDocumentFormat, PrimaryDocumentRoute, PrimaryDocumentShape, PrimaryRouteError,
};

use crate::{
    CompanionSelection, DocumentProgram, ProgramValidationError, XmlDocumentInputs, XmlOutputMode,
};

#[derive(Debug)]
pub enum PrimaryDocumentBoundaryError {
    /// The saved formats have no primary document route.
    Route(PrimaryRouteError),
    /// The route uses another format than the requested entry point.
    Format {
        expected: PrimaryDocumentFormat,
        found: PrimaryDocumentFormat,
    },
    /// Named sources and targets are not part of a primary document run.
    NamedEndpoints,
    /// The ordinary program is not admitted.
    Program(ProgramValidationError),
    /// The program has no single-document XML adapter.
    XmlAdapter,
    /// Strict JSON entry points do not use an XML adapter.
    JsonAdapter,
    FixedWidthAdapter,
    EdiAdapter,
    EdiDescriptor {
        bytes: usize,
        maximum: usize,
    },
    /// An observed XML root view reads one document, not a document set.
    RootViewDocumentSet,
}

impl fmt::Display for PrimaryDocumentBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EdiDescriptor { bytes, maximum } => {
                write!(f, "EDI descriptor is {bytes} bytes; maximum is {maximum}")
            }
            Self::EdiAdapter => {
                f.write_str("EDI documents require validated source and target policies")
            }
            Self::FixedWidthAdapter => {
                f.write_str("fixed-width documents require validated source and target layouts")
            }
            Self::Route(error) => error.fmt(f),
            Self::Format { expected, found } => write!(
                f,
                "a primary {} document entry point cannot read {} documents",
                expected.name(),
                found.name()
            ),
            Self::NamedEndpoints => {
                f.write_str("primary document entry points have no named sources or targets")
            }
            Self::Program(error) => error.fmt(f),
            Self::XmlAdapter => {
                f.write_str("primary XML documents require a single-document XML source and target")
            }
            Self::JsonAdapter => {
                f.write_str("primary JSON documents require a strict JSON source and target")
            }
            Self::RootViewDocumentSet => f.write_str(
                "an observed XML root view cannot read a filename wildcard's document set",
            ),
        }
    }
}

impl std::error::Error for PrimaryDocumentBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Route(error) => Some(error),
            Self::Program(error) => Some(error),
            _ => None,
        }
    }
}

fn route(
    document: &DocumentProgram,
    expected: PrimaryDocumentFormat,
) -> Result<PrimaryDocumentRoute, PrimaryDocumentBoundaryError> {
    let route = document
        .primary_route()
        .map_err(PrimaryDocumentBoundaryError::Route)?;
    if route.format != expected {
        return Err(PrimaryDocumentBoundaryError::Format {
            expected,
            found: route.format,
        });
    }
    let program = document.program();
    if !program.extra_sources.is_empty() || !program.extra_targets.is_empty() {
        return Err(PrimaryDocumentBoundaryError::NamedEndpoints);
    }
    Ok(route)
}

pub fn validate_primary_edi_documents(
    document: &DocumentProgram,
) -> Result<&crate::document_program::EdiDocumentPolicy, PrimaryDocumentBoundaryError> {
    let policy = document
        .edi_policy()
        .ok_or(PrimaryDocumentBoundaryError::EdiAdapter)?;
    route(document, PrimaryDocumentFormat::Edi(policy.kind))?;
    for descriptor in [
        serde_json::to_vec(&document.program().source),
        serde_json::to_vec(&document.program().target),
        serde_json::to_vec(&policy.source),
        serde_json::to_vec(&policy.target),
    ] {
        let bytes = descriptor
            .map_err(|_| PrimaryDocumentBoundaryError::EdiAdapter)?
            .len();
        let maximum = 1024 * 1024;
        if bytes > maximum {
            return Err(PrimaryDocumentBoundaryError::EdiDescriptor { bytes, maximum });
        }
    }
    crate::prepare_boundary_emission(document.program(), CompanionSelection::Complete)
        .map_err(PrimaryDocumentBoundaryError::Program)?;
    Ok(policy)
}

pub fn validate_primary_fixed_width_documents(
    document: &DocumentProgram,
) -> Result<&crate::document_program::FixedWidthDocumentPolicy, PrimaryDocumentBoundaryError> {
    route(document, PrimaryDocumentFormat::FixedWidth)?;
    crate::prepare_boundary_emission(document.program(), CompanionSelection::Complete)
        .map_err(PrimaryDocumentBoundaryError::Program)?;
    document
        .fixed_width_policy()
        .ok_or(PrimaryDocumentBoundaryError::FixedWidthAdapter)
}

/// Admit strict JSON source/target document plans before any artifact exists.
pub fn validate_primary_json_documents(
    document: &DocumentProgram,
) -> Result<(), PrimaryDocumentBoundaryError> {
    route(document, PrimaryDocumentFormat::Json)?;
    let plan = crate::prepare_boundary_emission(document.program(), CompanionSelection::Complete)
        .map_err(PrimaryDocumentBoundaryError::Program)?;
    if plan.xml().is_some() {
        return Err(PrimaryDocumentBoundaryError::JsonAdapter);
    }
    Ok(())
}

/// Admits a complete primary XML document entry point: an XML route without
/// named endpoints whose program has the single-document XML adapter. Returns
/// how that adapter reads each document.
pub fn validate_primary_xml_documents(
    document: &DocumentProgram,
) -> Result<XmlDocumentInputs, PrimaryDocumentBoundaryError> {
    let route = route(document, PrimaryDocumentFormat::Xml)?;
    let plan = crate::prepare_boundary_emission(document.program(), CompanionSelection::Complete)
        .map_err(PrimaryDocumentBoundaryError::Program)?;
    let xml = plan
        .xml()
        .filter(|xml| xml.mode() == XmlOutputMode::SingleDocument)
        .ok_or(PrimaryDocumentBoundaryError::XmlAdapter)?;
    match (xml.inputs(), route.shape) {
        (XmlDocumentInputs::RootViewWithNamedInputs, _) => {
            Err(PrimaryDocumentBoundaryError::NamedEndpoints)
        }
        (XmlDocumentInputs::RootViewPrimary, PrimaryDocumentShape::DocumentSet) => {
            Err(PrimaryDocumentBoundaryError::RootViewDocumentSet)
        }
        (inputs, _) => Ok(inputs),
    }
}
