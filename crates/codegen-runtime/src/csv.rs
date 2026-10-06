use std::fmt;

use ir::{Instance, SchemaNode};

use crate::{CsvFormatError, CsvWriteOptions, RuntimeError};

/// Maximum serialized UTF-8 bytes for one generated CSV document.
pub const MAX_CSV_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Typed CSV boundary failure. Native row errors retain their exact payload.
#[derive(Debug)]
pub enum CsvBoundaryError {
    Execution(RuntimeError),
    RootShape { got: &'static str },
    Format(CsvFormatError),
    OutputTooLarge { maximum: usize },
}

impl fmt::Display for CsvBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execution(error) => error.fmt(formatter),
            Self::RootShape { got } => write!(
                formatter,
                "CSV primary output requires repeated rows, got {got}"
            ),
            Self::Format(error) => error.fmt(formatter),
            Self::OutputTooLarge { maximum } => {
                write!(formatter, "CSV output exceeds the {maximum}-byte maximum")
            }
        }
    }
}

impl std::error::Error for CsvBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Execution(error) => Some(error),
            Self::Format(error) => Some(error),
            Self::RootShape { .. } | Self::OutputTooLarge { .. } => None,
        }
    }
}

impl From<RuntimeError> for CsvBoundaryError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

impl From<format_csv::CsvBoundedError> for CsvBoundaryError {
    fn from(error: format_csv::CsvBoundedError) -> Self {
        match error {
            format_csv::CsvBoundedError::Format(error) => Self::Format(error),
            format_csv::CsvBoundedError::OutputTooLarge { maximum } => {
                Self::OutputTooLarge { maximum }
            }
        }
    }
}

fn rows(primary: &Instance) -> Result<&[Instance], CsvBoundaryError> {
    let got = match primary {
        Instance::Repeated(rows) => return Ok(rows),
        Instance::Scalar(value) => value.type_name(),
        Instance::Group(_) => "group",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    };
    Err(CsvBoundaryError::RootShape { got })
}

/// Serialize a mapped repeated primary using a borrowed flat native row schema.
/// Mapping must complete before calling this boundary. No output is returned
/// unless every row and the complete serialized byte budget are valid.
pub fn serialize_csv_bytes(
    schema: &SchemaNode,
    primary: &Instance,
    options: &CsvWriteOptions,
) -> Result<Vec<u8>, CsvBoundaryError> {
    format_csv::to_bytes_with_options_bounded(
        schema,
        rows(primary)?,
        options,
        MAX_CSV_DOCUMENT_BYTES,
    )
    .map_err(CsvBoundaryError::from)
}

/// Serialize the same bounded native CSV bytes as owned UTF-8 text.
pub fn serialize_csv(
    schema: &SchemaNode,
    primary: &Instance,
    options: &CsvWriteOptions,
) -> Result<String, CsvBoundaryError> {
    format_csv::to_string_with_options_bounded(
        schema,
        rows(primary)?,
        options,
        MAX_CSV_DOCUMENT_BYTES,
    )
    .map_err(CsvBoundaryError::from)
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use ir::{ScalarType, Value};

    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::group("row", vec![SchemaNode::scalar("name", ScalarType::String)])
    }

    #[test]
    fn borrowed_row_schema_output_matches_native_with_complete_policy() {
        let rows = vec![Instance::Group(
            vec![(
                "name".into(),
                Instance::Scalar(Value::String("é;'quoted'\r\nnext".into())),
            )]
            .into(),
        )];
        let options = CsvWriteOptions {
            delimiter: Some(';'),
            quote: Some('\''),
            utf8_bom: true,
            ..CsvWriteOptions::default()
        };
        let expected = format_csv::to_string_with_options(&schema(), &rows, &options).unwrap();
        let primary = Instance::Repeated(rows);
        assert_eq!(
            serialize_csv(&schema(), &primary, &options).unwrap(),
            expected
        );
        assert_eq!(
            serialize_csv_bytes(&schema(), &primary, &options).unwrap(),
            expected.as_bytes()
        );
    }

    #[test]
    fn wrong_root_is_typed_and_native_late_row_error_retains_its_payload() {
        let options = CsvWriteOptions::default();
        let error = serialize_csv_bytes(&schema(), &Instance::Group(Vec::new().into()), &options)
            .unwrap_err();
        assert!(matches!(
            error,
            CsvBoundaryError::RootShape { got: "group" }
        ));
        let primary = Instance::Repeated(vec![
            Instance::Group(
                vec![("name".into(), Instance::Scalar(Value::String("ok".into())))].into(),
            ),
            Instance::Group(Vec::new().into()),
        ]);
        let error = serialize_csv_bytes(&schema(), &primary, &options).unwrap_err();
        assert!(
            matches!(&error, CsvBoundaryError::Format(CsvFormatError::MissingField { row: 1, field }) if field == "name")
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn execution_cause_remains_typed_and_unmodified() {
        let original = RuntimeError::NotABool {
            node: 37,
            found: "int",
        };
        let error = CsvBoundaryError::from(original);
        assert!(matches!(
            &error,
            CsvBoundaryError::Execution(RuntimeError::NotABool {
                node: 37,
                found: "int"
            })
        ));
        assert!(
            error
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>()
                .is_some()
        );
    }
}
