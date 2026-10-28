//! Atomic, bounded serialization of path-bearing CSV row documents.

use ir::{Instance, SchemaNode};
use thiserror::Error;

use crate::{CsvBoundedError, CsvWriteOptions, to_bytes_with_options_bounded};

pub const MAX_CSV_OUTPUT_DOCUMENTS: usize = 4096;
pub const MAX_CSV_OUTPUT_PATH_BYTES: usize = 4096;
pub const MAX_CSV_OUTPUT_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CSV_OUTPUT_TOTAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvBytesDocument {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvTextDocument {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Error)]
pub enum CsvDocumentsError {
    #[error("CSV document output requires a document set")]
    RootShape,
    #[error("CSV document output exceeds the {maximum}-document maximum")]
    TooManyDocuments { maximum: usize },
    #[error("CSV document {document} path exceeds {maximum} UTF-8 bytes")]
    PathTooLong { document: usize, maximum: usize },
    #[error("CSV document {document} at {path:?} requires repeated rows")]
    DocumentShape { document: usize, path: String },
    #[error("CSV document {document} at {path:?}: {source}")]
    Document {
        document: usize,
        path: String,
        source: CsvBoundedError,
    },
    #[error("CSV document encoder returned invalid UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Serialize every document before returning any output. Logical paths remain
/// unchanged, including duplicates; filesystem hosts validate destinations.
pub fn to_document_bytes(
    schema: &SchemaNode,
    value: &Instance,
    options: &CsvWriteOptions,
) -> Result<Vec<CsvBytesDocument>, CsvDocumentsError> {
    encode(schema, value, options, Limits::default())
}

pub fn to_document_text(
    schema: &SchemaNode,
    value: &Instance,
    options: &CsvWriteOptions,
) -> Result<Vec<CsvTextDocument>, CsvDocumentsError> {
    to_document_bytes(schema, value, options)?
        .into_iter()
        .map(|document| {
            Ok(CsvTextDocument {
                path: document.path,
                text: String::from_utf8(document.bytes)?,
            })
        })
        .collect()
}

/// Check logical metadata before a host allocates or normalizes native paths.
pub fn validate_document_output_metadata(
    documents: &[ir::DocumentMember],
) -> Result<(), CsvDocumentsError> {
    preflight(documents, &Limits::default())
}

struct Limits {
    documents: usize,
    path: usize,
    document: usize,
    total: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            documents: MAX_CSV_OUTPUT_DOCUMENTS,
            path: MAX_CSV_OUTPUT_PATH_BYTES,
            document: MAX_CSV_OUTPUT_DOCUMENT_BYTES,
            total: MAX_CSV_OUTPUT_TOTAL_BYTES,
        }
    }
}

fn encode(
    schema: &SchemaNode,
    value: &Instance,
    options: &CsvWriteOptions,
    limits: Limits,
) -> Result<Vec<CsvBytesDocument>, CsvDocumentsError> {
    let Instance::DocumentSet(documents) = value else {
        return Err(CsvDocumentsError::RootShape);
    };
    preflight(documents, &limits)?;
    let mut remaining = limits.total;
    let mut output = Vec::with_capacity(documents.len());
    for (document, member) in documents.iter().enumerate() {
        let Instance::Repeated(rows) = member.value() else {
            return Err(CsvDocumentsError::DocumentShape {
                document,
                path: member.path().into(),
            });
        };
        let bytes =
            to_bytes_with_options_bounded(schema, rows, options, remaining.min(limits.document))
                .map_err(|source| CsvDocumentsError::Document {
                    document,
                    path: member.path().into(),
                    source,
                })?;
        remaining -= bytes.len();
        output.push(CsvBytesDocument {
            path: member.path().into(),
            bytes,
        });
    }
    Ok(output)
}

fn preflight(documents: &[ir::DocumentMember], limits: &Limits) -> Result<(), CsvDocumentsError> {
    if documents.len() > limits.documents {
        return Err(CsvDocumentsError::TooManyDocuments {
            maximum: limits.documents,
        });
    }
    // Preflight paths before formatting any document. Count and path limits
    // bound metadata separately from the shared serialized-byte budget.
    for (document, member) in documents.iter().enumerate() {
        if member.path().len() > limits.path {
            return Err(CsvDocumentsError::PathTooLong {
                document,
                maximum: limits.path,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{DocumentMember, ScalarType, Value};

    fn schema() -> SchemaNode {
        SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
    }

    fn document(path: &str, text: &str) -> DocumentMember {
        DocumentMember::new(
            path,
            Instance::Repeated(vec![Instance::Group(
                vec![("Value".into(), Instance::Scalar(Value::String(text.into())))].into(),
            )]),
        )
        .unwrap()
    }

    #[test]
    fn documents_retain_paths_bom_headers_quoted_crlf_and_duplicates() {
        let value =
            Instance::DocumentSet(vec![document("é.csv", "A;\r\nB"), document("é.csv", "C")]);
        let options = CsvWriteOptions {
            delimiter: Some(';'),
            utf8_bom: true,
            ..Default::default()
        };
        let text = to_document_text(&schema(), &value, &options).unwrap();
        assert_eq!(
            text,
            vec![
                CsvTextDocument {
                    path: "é.csv".into(),
                    text: "\u{feff}Value\n\"A;\r\nB\"\n".into()
                },
                CsvTextDocument {
                    path: "é.csv".into(),
                    text: "\u{feff}Value\nC\n".into()
                },
            ]
        );
        let bytes = to_document_bytes(&schema(), &value, &options).unwrap();
        for (text, bytes) in text.iter().zip(bytes) {
            assert_eq!(text.path, bytes.path);
            assert_eq!(text.text.as_bytes(), bytes.bytes);
        }
    }

    #[test]
    fn one_global_budget_covers_every_header_and_document() {
        let value = Instance::DocumentSet(vec![document("a.csv", "A"), document("b.csv", "B")]);
        // Each file is eight bytes, but the total allowance is only fifteen.
        let error = encode(
            &schema(),
            &value,
            &CsvWriteOptions::default(),
            Limits {
                total: 15,
                document: 8,
                ..Limits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            CsvDocumentsError::Document {
                document: 1,
                source: CsvBoundedError::OutputTooLarge { maximum: 7 },
                ..
            }
        ));
        assert_eq!(
            encode(
                &schema(),
                &value,
                &CsvWriteOptions::default(),
                Limits {
                    total: 16,
                    document: 8,
                    ..Limits::default()
                }
            )
            .unwrap()
            .len(),
            2
        );
    }

    #[test]
    fn count_path_shape_and_late_row_failures_are_atomic() {
        let value = Instance::DocumentSet(vec![document("a.csv", "A"), document("é.csv", "B")]);
        assert!(matches!(
            encode(
                &schema(),
                &value,
                &Default::default(),
                Limits {
                    documents: 1,
                    ..Limits::default()
                }
            ),
            Err(CsvDocumentsError::TooManyDocuments { maximum: 1 })
        ));
        assert!(matches!(
            encode(
                &schema(),
                &value,
                &Default::default(),
                Limits {
                    path: 5,
                    ..Limits::default()
                }
            ),
            Err(CsvDocumentsError::PathTooLong {
                document: 1,
                maximum: 5
            })
        ));
        let wrong = Instance::DocumentSet(vec![
            document("a.csv", "A"),
            DocumentMember::new("b.csv", Instance::Group(vec![].into())).unwrap(),
        ]);
        assert!(matches!(
            to_document_bytes(&schema(), &wrong, &Default::default()),
            Err(CsvDocumentsError::DocumentShape { document: 1, .. })
        ));
        let wrong_row = Instance::DocumentSet(vec![
            document("a.csv", "A"),
            DocumentMember::new(
                "b.csv",
                Instance::Repeated(vec![Instance::Scalar(Value::Null)]),
            )
            .unwrap(),
        ]);
        assert!(matches!(
            to_document_bytes(&schema(), &wrong_row, &Default::default()),
            Err(CsvDocumentsError::Document {
                document: 1,
                source: CsvBoundedError::Format(_),
                ..
            })
        ));
        assert!(
            to_document_bytes(
                &schema(),
                &Instance::DocumentSet(vec![]),
                &Default::default()
            )
            .unwrap()
            .is_empty()
        );
    }
}
