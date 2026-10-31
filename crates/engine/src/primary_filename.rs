//! Source-independent evaluation; selection, loading and parsing stay host-owned.

use ir::{ScalarType, Value};
use mapping::{DocumentProject, GlobalFilename, NodeId, PrimaryFilenameError};
use thiserror::Error;

use crate::{EngineError, ExecutionContext};

#[derive(Debug, Error, PartialEq)]
pub enum PrimaryFilenameEvaluationError {
    #[error(transparent)]
    Plan(#[from] PrimaryFilenameError),
    #[error(transparent)]
    Parameter(#[from] EngineError),
    #[error("primary filename node {node} must produce a string, got {found}")]
    NonStringResult { node: NodeId, found: &'static str },
}

/// Evaluate a literal or global parameter before acquiring any source document.
///
/// Retained preview values are never used, even when the execution context is
/// configured for preview. This function preserves the selected text exactly;
/// it does not interpret paths, enumerate files, parse content or run the mapper.
/// Success is not validation or admission of the embedded mapping body.
pub fn evaluate_primary_filename(
    document: &DocumentProject,
    execution: &ExecutionContext<'_>,
) -> Result<String, PrimaryFilenameEvaluationError> {
    evaluate_filename(
        document.global_filename()?,
        document.primary_source().filename,
        execution,
    )
}

/// Resolve the explicit output transport without opening or publishing a file.
pub fn evaluate_output_filename(
    document: &DocumentProject,
    execution: &ExecutionContext<'_>,
) -> Result<Option<String>, PrimaryFilenameEvaluationError> {
    document
        .global_output_filename()?
        .map(|filename| {
            evaluate_filename(
                filename,
                document.output_filename().expect("output filename"),
                execution,
            )
        })
        .transpose()
}

fn evaluate_filename(
    filename: GlobalFilename<'_>,
    node: NodeId,
    execution: &ExecutionContext<'_>,
) -> Result<String, PrimaryFilenameEvaluationError> {
    match filename {
        GlobalFilename::Literal(text) => Ok(text.to_string()),
        GlobalFilename::Parameter(name) => {
            let value = execution
                .parameters
                .and_then(|parameters| parameters.values.get(name))
                .ok_or_else(|| EngineError::MissingRuntimeParameter {
                    node,
                    name: name.to_string(),
                })?;
            let coerced = crate::eval_expr::coerce_value_map_input(value, ScalarType::String)
                .ok_or_else(|| EngineError::RuntimeParameterType {
                    node,
                    name: name.to_string(),
                    expected: ScalarType::String,
                    found: value.type_name(),
                })?;
            match coerced {
                Value::String(text) => Ok(text),
                _ => Err(PrimaryFilenameEvaluationError::NonStringResult {
                    node,
                    found: value.type_name(),
                }),
            }
        }
    }
}
