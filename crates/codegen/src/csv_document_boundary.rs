//! Explicit single-document flat CSV input and output, sharing native policies.
use std::fmt;

use ir::SchemaNode;
use mapping::Project;

use crate::{CsvInputBoundaryError, CsvInputPolicy, CsvOutputError, CsvOutputPolicy, Program};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvDocumentBoundaryPolicy {
    pub source: CsvInputPolicy,
    pub target: CsvOutputPolicy,
}

#[derive(Debug)]
pub enum CsvDocumentBoundaryError {
    Source(CsvInputBoundaryError),
    TargetSchema(CsvInputBoundaryError),
    Target(CsvOutputError),
    ProgramField { field: &'static str },
}

impl fmt::Display for CsvDocumentBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "CSV document input: {error}"),
            Self::TargetSchema(error) => write!(f, "CSV document target schema: {error}"),
            Self::Target(error) => error.fmt(f),
            Self::ProgramField { field } => {
                write!(f, "CSV document adapters do not support {field}")
            }
        }
    }
}

impl std::error::Error for CsvDocumentBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) | Self::TargetSchema(error) => Some(error),
            Self::Target(error) => Some(error),
            Self::ProgramField { .. } => None,
        }
    }
}

impl CsvDocumentBoundaryPolicy {
    /// Verify endpoint options and borrowed schemas before lowering a project.
    pub fn from_project(project: &Project) -> Result<Self, CsvDocumentBoundaryError> {
        let policy = Self {
            source: CsvInputPolicy::from_input_format_options(&project.source_options)
                .map_err(CsvDocumentBoundaryError::Source)?,
            target: CsvOutputPolicy::from_format_options(&project.target_options)
                .map_err(CsvDocumentBoundaryError::Target)?,
        };
        endpoints(&project.source, &project.target)?;
        if !project.extra_sources.is_empty() || !project.extra_targets.is_empty() {
            return Err(CsvDocumentBoundaryError::ProgramField {
                field: "named endpoints",
            });
        }
        Ok(policy)
    }
}

fn endpoints(source: &SchemaNode, target: &SchemaNode) -> Result<(), CsvDocumentBoundaryError> {
    crate::csv_input_boundary::source_fields(source).map_err(CsvDocumentBoundaryError::Source)?;
    crate::csv_input_boundary::source_fields(target)
        .map_err(CsvDocumentBoundaryError::TargetSchema)?;
    Ok(())
}

/// Admit complete ordinary mapping semantics and both physical CSV endpoints.
/// The caller selects CSV explicitly; filenames and transport remain host-owned.
pub fn validate_csv_document_boundary(
    program: &Program,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<(), CsvDocumentBoundaryError> {
    policy
        .source
        .validate_dialect()
        .map_err(CsvDocumentBoundaryError::Source)?;
    endpoints(&program.source, &program.target)?;
    if !program.extra_sources.is_empty() {
        return Err(CsvDocumentBoundaryError::ProgramField {
            field: "named sources",
        });
    }
    crate::validate_csv_output(program, &policy.target).map_err(CsvDocumentBoundaryError::Target)
}

/// Admit a flat primary CSV input with explicit document-set output. This does
/// not acquire paths, select a decoder, or publish documents.
pub fn validate_csv_document_set_boundary(
    program: &Program,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<(), CsvDocumentBoundaryError> {
    crate::validate_primary_csv_acquisition(program, &policy.source)
        .map_err(CsvDocumentBoundaryError::Source)?;
    crate::validate_csv_document_set_output(program, &policy.target)
        .map_err(CsvDocumentBoundaryError::Target)
}
