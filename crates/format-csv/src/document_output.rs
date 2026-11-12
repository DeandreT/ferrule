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
    #[error("CSV mapped document row field must be nonempty and NUL-free")]
    InvalidRowsField,
    #[error("unsupported CSV mapped row schema: {reason}")]
    MappedRowSchema { reason: &'static str },
    #[error("invalid CSV mapped document configuration: {source}")]
    MappedConfiguration { source: CsvBoundedError },
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

/// Project the sole named repeated field of each constructed document. The
/// selected rows are borrowed; no source rows or wrapper fields are copied.
/// Logical paths are not filesystem destinations and are preserved unchanged.
/// `schema` is the nonrepeating physical row group, not the wrapper. This
/// profile refuses noncanonical metadata/constraints rather than enforcing it.
pub fn to_mapped_document_bytes(
    schema: &SchemaNode,
    value: &Instance,
    rows_field: &str,
    options: &CsvWriteOptions,
) -> Result<Vec<CsvBytesDocument>, CsvDocumentsError> {
    encode_projected(schema, value, options, Limits::default(), Some(rows_field))
}

pub fn to_mapped_document_text(
    schema: &SchemaNode,
    value: &Instance,
    rows_field: &str,
    options: &CsvWriteOptions,
) -> Result<Vec<CsvTextDocument>, CsvDocumentsError> {
    to_mapped_document_bytes(schema, value, rows_field, options)?
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
    encode_projected(schema, value, options, limits, None)
}

fn encode_projected(
    schema: &SchemaNode,
    value: &Instance,
    options: &CsvWriteOptions,
    limits: Limits,
    rows_field: Option<&str>,
) -> Result<Vec<CsvBytesDocument>, CsvDocumentsError> {
    if rows_field.is_some_and(|field| field.is_empty() || field.contains('\0')) {
        return Err(CsvDocumentsError::InvalidRowsField);
    }
    let Instance::DocumentSet(documents) = value else {
        return Err(CsvDocumentsError::RootShape);
    };
    preflight(documents, &limits)?;
    if rows_field.is_some() {
        validate_mapped_row_schema(schema)?;
        // Validate static settings even with zero documents. Do not validate
        // headers here: existing row/type errors precede no-quote boundaries.
        crate::require_executable_dependency(options.repair_dependency)
            .and_then(|()| {
                crate::dialect_bytes(options.delimiter, options.quote, options.quote_disabled)
                    .map(|_| ())
            })
            .and({
                if options.raw_delimited_crlf {
                    Err(crate::CsvFormatError::UnsupportedSchema)
                } else {
                    Ok(())
                }
            })
            .map_err(|source| CsvDocumentsError::MappedConfiguration {
                source: source.into(),
            })?;
    }
    let mut remaining = limits.total;
    let mut output = Vec::with_capacity(documents.len());
    for (document, member) in documents.iter().enumerate() {
        let selected = if let Some(rows_field) = rows_field {
            let bad = || CsvDocumentsError::DocumentShape {
                document,
                path: member.path().into(),
            };
            let Instance::Group(fields) = member.value() else {
                return Err(bad());
            };
            if fields.len() != 1 {
                return Err(bad());
            }
            let (name, rows) = &fields[0];
            if name != rows_field || !matches!(rows, Instance::Repeated(_)) {
                return Err(bad());
            }
            rows
        } else {
            member.value()
        };
        let Instance::Repeated(rows) = selected else {
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

// The general row_fields helper deliberately has a wider contract. This new
// profile refuses constraints/physical metadata it cannot enforce, including
// fixed/default values, rather than dropping them in generated descriptors.
fn validate_mapped_row_schema(schema: &SchemaNode) -> Result<(), CsvDocumentsError> {
    let bad = || CsvDocumentsError::MappedRowSchema {
        reason: "a canonical nonrepeating closed group of 1..=256 unique scalar columns is required",
    };
    let ir::SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(bad());
    };
    let bytes = children.iter().fold(schema.name.len(), |sum, field| {
        sum.saturating_add(field.name.len())
    });
    if children.is_empty() || children.len() > 256 || bytes > 1024 * 1024 {
        return Err(bad());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut canonical = Vec::with_capacity(children.len());
    for field in children {
        let ir::SchemaKind::Scalar { ty } = field.kind else {
            return Err(bad());
        };
        if field.name.is_empty() || !names.insert(field.name.as_str()) {
            return Err(bad());
        }
        let column = SchemaNode::scalar(&field.name, ty);
        if *field != column {
            return Err(bad());
        }
        canonical.push(column);
    }
    if *schema != SchemaNode::group(&schema.name, canonical) {
        return Err(bad());
    }
    Ok(())
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

    #[test]
    fn mapped_projection_keeps_one_budget_for_literal_107_byte_oracle() {
        let schema = SchemaNode::group(
            "DetailLines",
            vec![
                SchemaNode::scalar("Client", ScalarType::String),
                SchemaNode::scalar("SKU", ScalarType::String),
                SchemaNode::scalar("Total", ScalarType::Int),
                SchemaNode::scalar("Label", ScalarType::String),
            ],
        );
        let row = |client: &str, sku: &str, total, label: &str| {
            Instance::Group(
                vec![
                    (
                        "Client".into(),
                        Instance::Scalar(Value::String(client.into())),
                    ),
                    ("SKU".into(), Instance::Scalar(Value::String(sku.into()))),
                    ("Total".into(), Instance::Scalar(Value::Int(total))),
                    (
                        "Label".into(),
                        Instance::Scalar(Value::String(label.into())),
                    ),
                ]
                .into(),
            )
        };
        let document = |path: &str, rows| {
            ir::DocumentMember::new(
                path,
                Instance::Group(vec![("DetailLines".into(), Instance::Repeated(rows))].into()),
            )
            .unwrap()
        };
        let value = Instance::DocumentSet(vec![
            document(
                "out/B-2026-11-12T03:04:05Z.csv",
                vec![row("B", "x", 5, "é;\"Q\"\r\nZ"), row("B", "y", 6, "bee")],
            ),
            document(
                "out/A-2026-11-12T03:04:05Z.csv",
                vec![row("A", "x", 12, "A\"x"), row("A", "z", -2, "尾")],
            ),
        ]);
        let options = CsvWriteOptions {
            delimiter: Some(';'),
            utf8_bom: true,
            ..Default::default()
        };
        let output = encode_projected(
            &schema,
            &value,
            &options,
            Limits {
                total: 107,
                document: 56,
                ..Default::default()
            },
            Some("DetailLines"),
        )
        .unwrap();
        assert_eq!(
            output[0].bytes,
            b"\xef\xbb\xbfClient;SKU;Total;Label\nB;x;5;\"\xc3\xa9;\"\"Q\"\"\r\nZ\"\nB;y;6;bee\n"
        );
        assert_eq!(
            output[1].bytes,
            b"\xef\xbb\xbfClient;SKU;Total;Label\nA;x;12;\"A\"\"x\"\nA;z;-2;\xe5\xb0\xbe\n"
        );
        assert!(
            matches!(encode_projected(&schema, &value, &options, Limits { total: 106, ..Default::default() }, Some("DetailLines")),
            Err(CsvDocumentsError::Document { document: 1, path, source: CsvBoundedError::OutputTooLarge { maximum: 50 } }) if path == "out/A-2026-11-12T03:04:05Z.csv")
        );
        assert!(
            matches!(encode_projected(&schema, &value, &options, Limits { document: 55, ..Default::default() }, Some("DetailLines")),
            Err(CsvDocumentsError::Document { document: 0, path, source: CsvBoundedError::OutputTooLarge { maximum: 55 } }) if path == "out/B-2026-11-12T03:04:05Z.csv")
        );
        // The test seam only lowers the same budgets; production keeps the
        // existing literal limits and does not expose another limit API.
        let limits = Limits::default();
        assert_eq!(
            (limits.documents, limits.path, limits.document, limits.total),
            (4096, 4096, 67_108_864, 268_435_456)
        );
    }
}
