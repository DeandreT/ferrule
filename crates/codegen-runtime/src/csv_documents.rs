use crate::{CsvWriteOptions, RuntimeError};
use ir::{Instance, SchemaNode};
use std::fmt;

#[derive(Debug)]
pub enum CsvDocumentOutputError {
    Execution(RuntimeError),
    Output(format_csv::CsvDocumentsError),
}

impl fmt::Display for CsvDocumentOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execution(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CsvDocumentOutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Execution(error) => error,
            Self::Output(error) => error,
        })
    }
}

impl From<RuntimeError> for CsvDocumentOutputError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

pub fn serialize_csv_documents_bytes(
    schema: &SchemaNode,
    value: &Instance,
    options: &CsvWriteOptions,
) -> Result<Vec<format_csv::CsvBytesDocument>, CsvDocumentOutputError> {
    format_csv::to_document_bytes(schema, value, options).map_err(CsvDocumentOutputError::Output)
}

pub fn serialize_csv_documents(
    schema: &SchemaNode,
    value: &Instance,
    options: &CsvWriteOptions,
) -> Result<Vec<format_csv::CsvTextDocument>, CsvDocumentOutputError> {
    format_csv::to_document_text(schema, value, options).map_err(CsvDocumentOutputError::Output)
}
