//! Explicit primary CSV acquisition composed with strict mapped-document output.

use std::fmt;

use crate::{
    CsvInputBoundaryError, CsvInputPolicy, CsvMappedDocumentOutputError,
    CsvMappedDocumentOutputPolicy, DocumentProgram,
};

/// Independently selected input and constructed-document output policies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryMappedCsvDocumentPolicy {
    pub input: CsvInputPolicy,
    pub output: CsvMappedDocumentOutputPolicy,
}

#[derive(Debug)]
pub enum PrimaryMappedCsvDocumentBoundaryError {
    OutputFilename,
    Selection,
    Input(CsvInputBoundaryError),
    Output(CsvMappedDocumentOutputError),
}

impl fmt::Display for PrimaryMappedCsvDocumentBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputFilename => f.write_str(
                "primary mapped CSV documents use computed document paths, not a global output filename",
            ),
            Self::Selection => {
                f.write_str("primary mapped CSV documents require direct filename selection")
            }
            Self::Input(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PrimaryMappedCsvDocumentBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

impl PrimaryMappedCsvDocumentPolicy {
    /// Capture understood endpoint settings. Complete document admission still
    /// requires [`validate_primary_mapped_csv_documents`] after lowering.
    pub fn from_project(
        project: &mapping::Project,
        rows_field: impl Into<String>,
    ) -> Result<Self, PrimaryMappedCsvDocumentBoundaryError> {
        let input = CsvInputPolicy::from_input_format_options(&project.source_options)
            .map_err(PrimaryMappedCsvDocumentBoundaryError::Input)?;
        crate::csv_input_boundary::source_fields(&project.source)
            .map_err(PrimaryMappedCsvDocumentBoundaryError::Input)?;
        let output = CsvMappedDocumentOutputPolicy::from_project(project, rows_field)
            .map_err(PrimaryMappedCsvDocumentBoundaryError::Output)?;
        Ok(Self { input, output })
    }
}

/// Validate the entire explicit composition before rendering any artifacts.
/// Lowering owns filename-expression classification; this profile does not
/// infer a saved-format route or accept a global filename for computed outputs.
pub fn validate_primary_mapped_csv_documents(
    document: &DocumentProgram,
    policy: &PrimaryMappedCsvDocumentPolicy,
) -> Result<(), PrimaryMappedCsvDocumentBoundaryError> {
    if document.output_filename().is_some() {
        return Err(PrimaryMappedCsvDocumentBoundaryError::OutputFilename);
    }
    if !document.primary_selection().is_direct() {
        return Err(PrimaryMappedCsvDocumentBoundaryError::Selection);
    }
    crate::validate_primary_csv_acquisition(document.program(), &policy.input)
        .map_err(PrimaryMappedCsvDocumentBoundaryError::Input)?;
    crate::validate_csv_mapped_document_output(document.program(), &policy.output)
        .map_err(PrimaryMappedCsvDocumentBoundaryError::Output)
}
