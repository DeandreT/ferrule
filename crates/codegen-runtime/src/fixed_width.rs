//! Fixed-width document boundaries share the native layout and scalar codecs.
use crate::{PrimaryAcquisitionError, PrimaryAssemblyError, RuntimeError};
use ir::{Instance, SchemaNode, SourceDocumentOrigin};
use mapping::FixedWidthLayout;
use std::{error::Error, fmt};

pub const MAX_FIXED_WIDTH_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FIXED_WIDTH_TOTAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum FixedWidthBoundaryError {
    Descriptor(String),
    Encoding(std::str::Utf8Error),
    Format(format_csv::CsvFormatError),
    InputTooLarge { bytes: usize, maximum: usize },
    OutputTooLarge { maximum: usize },
    Path,
    RootShape,
    Execution(RuntimeError),
}
impl fmt::Display for FixedWidthBoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Descriptor(error) => write!(f, "fixed-width descriptor: {error}"),
            Self::Encoding(error) => error.fmt(f),
            Self::Format(error) => error.fmt(f),
            Self::InputTooLarge { bytes, maximum } => {
                write!(f, "fixed-width input has {bytes} bytes; maximum {maximum}")
            }
            Self::OutputTooLarge { maximum } => {
                write!(f, "fixed-width output exceeds {maximum} bytes")
            }
            Self::Path => f.write_str("invalid fixed-width source path"),
            Self::RootShape => f.write_str("fixed-width output requires repeated rows"),
            Self::Execution(error) => error.fmt(f),
        }
    }
}
impl Error for FixedWidthBoundaryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encoding(e) => Some(e),
            Self::Format(e) => Some(e),
            Self::Execution(e) => Some(e),
            _ => None,
        }
    }
}
#[derive(Default)]
pub struct FixedWidthInputBudget {
    bytes: usize,
}
impl FixedWidthInputBudget {
    pub fn charge(&mut self, bytes: usize) -> Result<(), FixedWidthBoundaryError> {
        if bytes > MAX_FIXED_WIDTH_DOCUMENT_BYTES {
            return Err(FixedWidthBoundaryError::InputTooLarge {
                bytes,
                maximum: MAX_FIXED_WIDTH_DOCUMENT_BYTES,
            });
        }
        let total = self.bytes.saturating_add(bytes);
        if total > MAX_FIXED_WIDTH_TOTAL_BYTES {
            return Err(FixedWidthBoundaryError::InputTooLarge {
                bytes: total,
                maximum: MAX_FIXED_WIDTH_TOTAL_BYTES,
            });
        }
        self.bytes = total;
        Ok(())
    }
}

fn descriptors(
    schema: &str,
    layout: &str,
) -> Result<(SchemaNode, FixedWidthLayout), FixedWidthBoundaryError> {
    if schema.len() > crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES
        || layout.len() > crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES
    {
        return Err(FixedWidthBoundaryError::Descriptor(
            "descriptor exceeds 1 MiB".into(),
        ));
    }
    Ok((
        serde_json::from_str(schema)
            .map_err(|e| FixedWidthBoundaryError::Descriptor(e.to_string()))?,
        serde_json::from_str(layout)
            .map_err(|e| FixedWidthBoundaryError::Descriptor(e.to_string()))?,
    ))
}

pub fn parse_fixed_width_bytes(
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
    path: &str,
    resolved: Option<&str>,
    bytes: &[u8],
) -> Result<Instance, FixedWidthBoundaryError> {
    if bytes.len() > MAX_FIXED_WIDTH_DOCUMENT_BYTES {
        return Err(FixedWidthBoundaryError::InputTooLarge {
            bytes: bytes.len(),
            maximum: MAX_FIXED_WIDTH_DOCUMENT_BYTES,
        });
    }
    let origin =
        SourceDocumentOrigin::new(path, resolved).map_err(|_| FixedWidthBoundaryError::Path)?;
    let text = std::str::from_utf8(bytes).map_err(FixedWidthBoundaryError::Encoding)?;
    let rows = format_csv::from_str_fixed_width(text, schema, layout)
        .map_err(FixedWidthBoundaryError::Format)?;
    Ok(Instance::Repeated(
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| match row {
                Instance::Group(group) => Instance::Group(
                    group.with_source_origin(origin.row(index + 1).expect("one-based row")),
                ),
                _ => unreachable!("native fixed-width rows are groups"),
            })
            .collect(),
    ))
}

pub fn parse_fixed_width_document(
    schema: &str,
    layout: &str,
    path: &str,
    resolved: Option<&str>,
    bytes: &[u8],
) -> Result<Instance, FixedWidthBoundaryError> {
    let (schema, layout) = descriptors(schema, layout)?;
    parse_fixed_width_bytes(&schema, &layout, path, resolved, bytes)
}

pub fn serialize_fixed_width_bytes(
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
    source: &Instance,
) -> Result<Vec<u8>, FixedWidthBoundaryError> {
    let rows = source
        .as_repeated()
        .ok_or(FixedWidthBoundaryError::RootShape)?;
    format_csv::to_bytes_fixed_width_bounded(schema, rows, layout, MAX_FIXED_WIDTH_DOCUMENT_BYTES)
        .map_err(|error| match error {
            format_csv::CsvBoundedError::Format(error) => FixedWidthBoundaryError::Format(error),
            format_csv::CsvBoundedError::OutputTooLarge { maximum } => {
                FixedWidthBoundaryError::OutputTooLarge { maximum }
            }
        })
}
pub fn serialize_fixed_width_document(
    schema: &str,
    layout: &str,
    source: &Instance,
) -> Result<Vec<u8>, FixedWidthBoundaryError> {
    let (schema, layout) = descriptors(schema, layout)?;
    serialize_fixed_width_bytes(&schema, &layout, source)
}

#[derive(Debug)]
pub enum PrimaryFixedWidthDocumentError<E> {
    Acquisition(PrimaryAcquisitionError<RuntimeError, E, FixedWidthBoundaryError>),
    Assembly(PrimaryAssemblyError),
    Output(FixedWidthBoundaryError),
}
impl<E: fmt::Display> fmt::Display for PrimaryFixedWidthDocumentError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Acquisition(e) => e.fmt(f),
            Self::Assembly(e) => e.fmt(f),
            Self::Output(e) => e.fmt(f),
        }
    }
}
impl<E: Error + 'static> Error for PrimaryFixedWidthDocumentError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Acquisition(e) => e,
            Self::Assembly(e) => e,
            Self::Output(e) => e,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};
    fn layout(empty: bool) -> FixedWidthLayout {
        FixedWidthLayout::new(
            vec![mapping::FixedFieldWidth::new(3).unwrap()],
            '_',
            true,
            empty,
        )
        .unwrap()
    }
    #[test]
    fn empty_policy_origins_and_typed_failures() {
        let schema = SchemaNode::group("row", vec![SchemaNode::scalar("Text", ScalarType::String)]);
        for empty in [false, true] {
            let parsed = parse_fixed_width_bytes(
                &schema,
                &layout(empty),
                "logical",
                Some("/actual"),
                b"___\r\nabc\n",
            )
            .unwrap();
            let rows = parsed.as_repeated().unwrap();
            let expected = if empty {
                Value::Null
            } else {
                Value::String(String::new())
            };
            assert_eq!(
                rows[0].field("Text").unwrap().as_scalar().unwrap(),
                &expected
            );
            let Instance::Group(group) = &rows[1] else {
                panic!("group")
            };
            assert_eq!(group.source_origin().unwrap().position(), 2);
            assert_eq!(
                group.source_origin().unwrap().document().source_path(),
                "/actual"
            );
            let expected = b"___\nabc\n";
            assert_eq!(
                format_csv::to_bytes_fixed_width_bounded(
                    &schema,
                    rows,
                    &layout(empty),
                    expected.len()
                )
                .unwrap(),
                expected
            );
            assert!(matches!(
                format_csv::to_bytes_fixed_width_bounded(
                    &schema,
                    rows,
                    &layout(empty),
                    expected.len() - 1
                ),
                Err(format_csv::CsvBoundedError::OutputTooLarge { .. })
            ));
        }
        let schema = SchemaNode::group("row", vec![SchemaNode::scalar("Text", ScalarType::Int)]);
        assert!(matches!(
            parse_fixed_width_bytes(&schema, &layout(false), "logical", None, b"___"),
            Err(FixedWidthBoundaryError::Format(
                format_csv::CsvFormatError::Parse { row: 0, .. }
            ))
        ));
        assert!(matches!(
            parse_fixed_width_bytes(&schema, &layout(true), "logical", None, b"12"),
            Err(FixedWidthBoundaryError::Format(
                format_csv::CsvFormatError::PartialFixedWidthRecord {
                    expected: 3,
                    got: 2,
                    ..
                }
            ))
        ));
    }
    #[test]
    fn budgets_do_not_accept_a_partial_charge() {
        let mut budget = FixedWidthInputBudget::default();
        assert!(budget.charge(MAX_FIXED_WIDTH_DOCUMENT_BYTES + 1).is_err());
        for _ in 0..4 {
            budget.charge(MAX_FIXED_WIDTH_DOCUMENT_BYTES).unwrap();
        }
        assert!(matches!(
            budget.charge(1),
            Err(FixedWidthBoundaryError::InputTooLarge {
                maximum: MAX_FIXED_WIDTH_TOTAL_BYTES,
                ..
            })
        ));
        budget.charge(0).unwrap();
    }
}
