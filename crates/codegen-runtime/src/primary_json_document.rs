//! Strict JSON primary-document boundary failures and aggregate input budget.
use crate::{JsonBoundaryError, PrimaryAcquisitionError, PrimaryAssemblyError, RuntimeError};
use std::{error::Error, fmt};

pub const MAX_PRIMARY_JSON_INPUT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct PrimaryJsonInputBudget {
    bytes: usize,
}

impl PrimaryJsonInputBudget {
    /// Charge before parsing. A failed charge does not admit a partial document.
    pub fn charge(&mut self, bytes: usize) -> Result<(), JsonBoundaryError> {
        if bytes > crate::MAX_JSON_DOCUMENT_BYTES {
            return Err(JsonBoundaryError::InputTooLarge {
                bytes,
                max: crate::MAX_JSON_DOCUMENT_BYTES,
            });
        }
        let total = self.bytes.saturating_add(bytes);
        if total > MAX_PRIMARY_JSON_INPUT_BYTES {
            return Err(JsonBoundaryError::InputTooLarge {
                bytes: total,
                max: MAX_PRIMARY_JSON_INPUT_BYTES,
            });
        }
        self.bytes = total;
        Ok(())
    }
}

#[derive(Debug)]
pub enum PrimaryJsonDocumentError<E> {
    Acquisition(PrimaryAcquisitionError<RuntimeError, E, JsonBoundaryError>),
    Assembly(PrimaryAssemblyError),
    Output(JsonBoundaryError),
}

impl<E: fmt::Display> fmt::Display for PrimaryJsonDocumentError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Acquisition(error) => error.fmt(f),
            Self::Assembly(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
        }
    }
}
impl<E: Error + 'static> Error for PrimaryJsonDocumentError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Acquisition(error) => error,
            Self::Assembly(error) => error,
            Self::Output(error) => error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_and_aggregate_limits_are_independent_and_inclusive() {
        let mut budget = PrimaryJsonInputBudget::default();
        let document = crate::MAX_JSON_DOCUMENT_BYTES;
        assert!(
            matches!(budget.charge(document + 1), Err(JsonBoundaryError::InputTooLarge { max, .. }) if max == document)
        );
        for _ in 0..4 {
            budget.charge(document).unwrap();
        }
        assert!(matches!(
            budget.charge(1),
            Err(JsonBoundaryError::InputTooLarge {
                max: MAX_PRIMARY_JSON_INPUT_BYTES,
                ..
            })
        ));
        budget.charge(0).unwrap();
    }

    #[test]
    fn complete_output_is_bounded_before_rendering() {
        let schema = ir::SchemaNode::scalar("Text", ir::ScalarType::String);
        let descriptor = serde_json::to_string(&schema).unwrap();
        let output = ir::Instance::Scalar(ir::Value::String(
            "a".repeat(crate::MAX_JSON_DOCUMENT_BYTES),
        ));
        assert!(
            matches!(crate::serialize_json_bytes(&descriptor, &output), Err(JsonBoundaryError::OutputTooLarge { max, .. }) if max == crate::MAX_JSON_DOCUMENT_BYTES)
        );
    }
}
