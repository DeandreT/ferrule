//! Delimited flat file schema and instance read/write, backed by the `csv`
//! crate for correct quoting/escaping.
//!
//! A CSV file's row-schema is a non-repeating [`SchemaNode::Group`] of
//! scalar fields; the file's row-repetition itself is a format convention,
//! not something declared in the schema (unlike XML, where `repeating` is a
//! per-element schema property).
//! Empty cells represent [`Value::Null`] for every scalar type; CSV therefore
//! cannot distinguish a null string from an intentionally empty string.

use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use thiserror::Error;

mod fixed_width;

pub use fixed_width::{
    from_str_fixed_width, read_fixed_width, to_string_fixed_width, write_fixed_width,
};

#[derive(Debug, Error)]
pub enum CsvFormatError {
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("row schema must be a non-repeating group of non-repeating scalar fields")]
    UnsupportedSchema,
    #[error("row {row}: column `{field}` expected {expected:?}, got `{value}`")]
    Parse {
        row: usize,
        field: String,
        expected: ScalarType,
        value: String,
    },
    #[error("row {row}: expected {expected} column(s), got {got}")]
    ColumnCount {
        row: usize,
        expected: usize,
        got: usize,
    },
    #[error("row {row}: expected a group, got {got}")]
    RowShape { row: usize, got: &'static str },
    #[error("row {row}: missing column `{field}`")]
    MissingField { row: usize, field: String },
    #[error("row {row}: unexpected column `{field}`")]
    UnexpectedField { row: usize, field: String },
    #[error("row {row}: duplicate column `{field}`")]
    DuplicateField { row: usize, field: String },
    #[error("row {row}: column `{field}` expected {expected:?}, got {got}")]
    ValueType {
        row: usize,
        field: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error("`{0}` is not a valid CSV delimiter (must be a single-byte character)")]
    BadDelimiter(char),
    #[error("fixed-width layout declares {got} field width(s), but the schema has {expected}")]
    FixedWidthFieldCount { expected: usize, got: usize },
    #[error("fixed-width record {record}: expected {expected} character(s), got only {got}")]
    PartialFixedWidthRecord {
        record: usize,
        expected: usize,
        got: usize,
    },
    #[error("fixed-width record {record}: expected {expected} character(s), got {got}")]
    FixedWidthRecordOverflow {
        record: usize,
        expected: usize,
        got: usize,
    },
    #[error(
        "row {row}: column `{field}` exceeds its fixed width of {width} character(s), got {got}"
    )]
    FixedWidthFieldOverflow {
        row: usize,
        field: String,
        width: usize,
        got: usize,
    },
}

fn delimiter_byte(delimiter: Option<char>) -> Result<u8, CsvFormatError> {
    match delimiter {
        None => Ok(b','),
        Some(c) if c.is_ascii() => Ok(c as u8),
        Some(c) => Err(CsvFormatError::BadDelimiter(c)),
    }
}

fn row_fields(schema: &SchemaNode) -> Result<Vec<(&str, ScalarType)>, CsvFormatError> {
    if schema.repeating {
        return Err(CsvFormatError::UnsupportedSchema);
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .map(|c| match &c.kind {
                SchemaKind::Scalar { ty } if !c.repeating => Ok((c.name.as_str(), *ty)),
                _ => Err(CsvFormatError::UnsupportedSchema),
            })
            .collect(),
        SchemaKind::Scalar { .. } => Err(CsvFormatError::UnsupportedSchema),
    }
}

/// Reads a CSV file into one [`Instance::Group`] per row, parsing each
/// column according to its declared scalar type (columns are positional;
/// when `has_headers` the first row is skipped). Missing trailing columns
/// are represented as [`Value::Null`]. `delimiter` defaults to `,`.
pub fn read(
    path: &Path,
    schema: &SchemaNode,
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    let fields = row_fields(schema)?;
    let reader = csv::ReaderBuilder::new()
        .has_headers(has_headers)
        .flexible(true)
        .delimiter(delimiter_byte(delimiter)?)
        .from_path(path)?;
    read_records(reader, &fields)
}

/// Reads CSV text into one [`Instance::Group`] per row.
///
/// This is the in-memory equivalent of [`read`], suitable for hosts without
/// filesystem access such as WebAssembly applications.
pub fn from_str(
    text: &str,
    schema: &SchemaNode,
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    let fields = row_fields(schema)?;
    let reader = csv::ReaderBuilder::new()
        .has_headers(has_headers)
        .flexible(true)
        .delimiter(delimiter_byte(delimiter)?)
        .from_reader(text.as_bytes());
    read_records(reader, &fields)
}

fn read_records<R: std::io::Read>(
    mut reader: csv::Reader<R>,
    fields: &[(&str, ScalarType)],
) -> Result<Vec<Instance>, CsvFormatError> {
    let mut out = Vec::new();
    for (row_idx, result) in reader.records().enumerate() {
        let raw = result?;
        if raw.len() > fields.len() {
            return Err(CsvFormatError::ColumnCount {
                row: row_idx,
                expected: fields.len(),
                got: raw.len(),
            });
        }
        let mut row = Vec::with_capacity(fields.len());
        for (column, (name, ty)) in fields.iter().enumerate() {
            let cell = raw.get(column).unwrap_or_default();
            row.push((
                name.to_string(),
                Instance::Scalar(parse_value(name, *ty, cell, row_idx)?),
            ));
        }
        out.push(Instance::Group(row));
    }
    Ok(out)
}

fn parse_value(
    name: &str,
    ty: ScalarType,
    cell: &str,
    row: usize,
) -> Result<Value, CsvFormatError> {
    if cell.is_empty() {
        return Ok(Value::Null);
    }
    parse_present_value(name, ty, cell, row)
}

fn parse_present_value(
    name: &str,
    ty: ScalarType,
    cell: &str,
    row: usize,
) -> Result<Value, CsvFormatError> {
    let bad = || CsvFormatError::Parse {
        row,
        field: name.to_string(),
        expected: ty,
        value: cell.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(cell.to_string()),
        ScalarType::Int => Value::Int(cell.parse().map_err(|_| bad())?),
        ScalarType::Float => {
            let value = cell.parse::<f64>().map_err(|_| bad())?;
            if !value.is_finite() {
                return Err(bad());
            }
            Value::Float(value)
        }
        ScalarType::Bool => Value::Bool(boolean_lexical(cell).ok_or_else(bad)?),
    })
}

/// Writes one row per [`Instance::Group`] in `rows` to a CSV file, with a
/// header row when `has_headers`. `delimiter` defaults to `,`.
pub fn write(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<(), CsvFormatError> {
    std::fs::write(path, to_string(schema, rows, delimiter, has_headers)?)?;
    Ok(())
}

/// Writes one row per [`Instance::Group`] in `rows` as CSV text.
///
/// This is the in-memory equivalent of [`write`], including its delimiter,
/// header, schema validation, and flat-row conventions.
pub fn to_string(
    schema: &SchemaNode,
    rows: &[Instance],
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<String, CsvFormatError> {
    let fields = row_fields(schema)?;
    let delimiter = delimiter_byte(delimiter)?;
    // Validate and materialize every record before producing output. A
    // shape/type error must not truncate a previously valid output file.
    let records = rows
        .iter()
        .enumerate()
        .map(|(row, instance)| format_row(row, instance, &fields))
        .collect::<Result<Vec<_>, _>>()?;
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    if has_headers {
        writer.write_record(fields.iter().map(|(n, _)| *n))?;
    }
    for record in records {
        writer.write_record(record)?;
    }
    writer.flush()?;
    let bytes = writer.into_inner().map_err(|error| error.into_error())?;
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error).into())
}

fn format_row(
    row: usize,
    instance: &Instance,
    schema_fields: &[(&str, ScalarType)],
) -> Result<Vec<String>, CsvFormatError> {
    let Instance::Group(instance_fields) = instance else {
        return Err(CsvFormatError::RowShape {
            row,
            got: instance_type_name(instance),
        });
    };

    for (index, (name, _)) in instance_fields.iter().enumerate() {
        if !schema_fields
            .iter()
            .any(|(schema_name, _)| schema_name == name)
        {
            return Err(CsvFormatError::UnexpectedField {
                row,
                field: name.clone(),
            });
        }
        if instance_fields[..index]
            .iter()
            .any(|(previous, _)| previous == name)
        {
            return Err(CsvFormatError::DuplicateField {
                row,
                field: name.clone(),
            });
        }
    }

    schema_fields
        .iter()
        .map(|(name, ty)| {
            let value = instance_fields
                .iter()
                .find(|(instance_name, _)| instance_name == name)
                .ok_or_else(|| CsvFormatError::MissingField {
                    row,
                    field: (*name).to_string(),
                })?;
            let Instance::Scalar(value) = &value.1 else {
                return Err(value_type_error(
                    row,
                    name,
                    *ty,
                    instance_type_name(&value.1),
                ));
            };
            format_value(row, name, *ty, value)
        })
        .collect()
}

fn format_value(
    row: usize,
    field: &str,
    ty: ScalarType,
    value: &Value,
) -> Result<String, CsvFormatError> {
    let incompatible = || value_type_error(row, field, ty, value.type_name());
    match (ty, value) {
        (_, Value::Null | Value::JsonNull(_)) => Ok(String::new()),
        (ScalarType::String, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::String, Value::Float(_)) => {
            Err(value_type_error(row, field, ty, "non-finite float"))
        }
        (ScalarType::String, Value::String(value)) => Ok(value.clone()),
        (ScalarType::Int, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::Int, Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible()),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::Float, Value::Float(_)) => {
            Err(value_type_error(row, field, ty, "non-finite float"))
        }
        (ScalarType::Float, Value::Int(value)) if exact_f64(*value).is_some() => {
            Ok(value.to_string())
        }
        (ScalarType::Float, Value::Int(_)) => Err(value_type_error(
            row,
            field,
            ty,
            "int outside the exact f64 range",
        )),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .ok_or_else(incompatible),
        (ScalarType::Bool, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::Bool, Value::String(value)) => boolean_lexical(value)
            .map(|value| value.to_string())
            .ok_or_else(incompatible),
        _ => Err(incompatible()),
    }
}

fn boolean_lexical(value: &str) -> Option<bool> {
    match value.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}

fn value_type_error(
    row: usize,
    field: &str,
    expected: ScalarType,
    got: &'static str,
) -> CsvFormatError {
    CsvFormatError::ValueType {
        row,
        field: field.to_string(),
        expected,
        got,
    }
}

fn instance_type_name(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(value) => value.type_name(),
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        )
    }

    #[test]
    fn text_io_roundtrips_with_headers() {
        let row = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(29))),
        ]);

        let text = to_string(&schema(), std::slice::from_ref(&row), None, true).unwrap();
        let read_back = from_str(&text, &schema(), None, true).unwrap();

        assert_eq!(text, "name,age\nJane,29\n");
        assert_eq!(read_back, vec![row]);
    }

    #[test]
    fn column_count_mismatch_is_reported() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_csv_test_bad_{}.csv",
            std::process::id()
        ));
        std::fs::write(&path, "name,age,extra\nJane,29,x\n").unwrap();

        let err = read(&path, &schema(), None, true).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            err,
            CsvFormatError::ColumnCount {
                expected: 2,
                got: 3,
                ..
            }
        ));
    }

    #[test]
    fn missing_trailing_columns_are_null() {
        let rows = from_str("Jane,29\nJohn\n", &schema(), None, false).unwrap();

        assert_eq!(
            rows,
            vec![
                Instance::Group(vec![
                    (
                        "name".into(),
                        Instance::Scalar(Value::String("Jane".into())),
                    ),
                    ("age".into(), Instance::Scalar(Value::Int(29))),
                ]),
                Instance::Group(vec![
                    (
                        "name".into(),
                        Instance::Scalar(Value::String("John".into())),
                    ),
                    ("age".into(), Instance::Scalar(Value::Null)),
                ]),
            ]
        );
    }

    #[test]
    fn text_io_roundtrips_without_headers() {
        let row = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(29))),
        ]);

        let text = to_string(&schema(), std::slice::from_ref(&row), None, false).unwrap();
        let read_back = from_str(&text, &schema(), None, false).unwrap();

        assert_eq!(text, "Jane,29\n");
        assert_eq!(read_back, vec![row]);
    }

    #[test]
    fn text_io_roundtrips_a_custom_delimiter_and_quoted_value() {
        let row = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane;Doe".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(29))),
        ]);

        let text = to_string(&schema(), std::slice::from_ref(&row), Some(';'), true).unwrap();
        let read_back = from_str(&text, &schema(), Some(';'), true).unwrap();

        assert_eq!(text, "name;age\n\"Jane;Doe\";29\n");
        // The value containing the delimiter must be quoted, not split.
        assert_eq!(read_back, vec![row]);
    }

    #[test]
    fn write_rejects_incompatible_typed_strings_without_truncating_output() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_test_invalid_value_{}.csv",
            std::process::id()
        ));
        std::fs::write(&path, "existing output\n").unwrap();
        let row = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            (
                "age".into(),
                Instance::Scalar(Value::String("not a number".into())),
            ),
        ]);

        let error = write(&path, &schema(), &[row], None, true).unwrap_err();
        let unchanged = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(matches!(
            error,
            CsvFormatError::ValueType {
                row: 0,
                field,
                expected: ScalarType::Int,
                got: "string",
            } if field == "age"
        ));
        assert_eq!(unchanged, "existing output\n");
    }

    #[test]
    fn write_rejects_non_group_rows() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_test_scalar_row_{}.csv",
            std::process::id()
        ));

        let error = write(
            &path,
            &schema(),
            &[Instance::Scalar(Value::String("Jane,29".into()))],
            None,
            true,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            CsvFormatError::RowShape {
                row: 0,
                got: "string"
            }
        ));
        assert!(!path.exists());

        let error = write(
            &path,
            &schema(),
            &[Instance::MappedSequence(Vec::new())],
            None,
            true,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            CsvFormatError::RowShape {
                row: 0,
                got: "mapped sequence"
            }
        ));
        assert!(!path.exists());
    }

    #[test]
    fn write_rejects_missing_and_non_scalar_fields() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_test_bad_fields_{}.csv",
            std::process::id()
        ));
        let missing = Instance::Group(vec![(
            "name".into(),
            Instance::Scalar(Value::String("Jane".into())),
        )]);
        let error = write(&path, &schema(), &[missing], None, false).unwrap_err();
        assert!(matches!(
            error,
            CsvFormatError::MissingField { row: 0, field } if field == "age"
        ));

        let nested = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("age".into(), Instance::Group(Vec::new())),
        ]);
        let error = write(&path, &schema(), &[nested], None, false).unwrap_err();
        assert!(matches!(
            error,
            CsvFormatError::ValueType {
                row: 0,
                field,
                expected: ScalarType::Int,
                got: "group",
            } if field == "age"
        ));

        let mapped = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("age".into(), Instance::MappedSequence(Vec::new())),
        ]);
        let error = write(&path, &schema(), &[mapped], None, false).unwrap_err();
        assert!(matches!(
            error,
            CsvFormatError::ValueType {
                row: 0,
                field,
                expected: ScalarType::Int,
                got: "mapped sequence",
            } if field == "age"
        ));
        assert!(!path.exists());
    }

    #[test]
    fn null_cells_roundtrip_for_all_scalar_types() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_test_nulls_{}.csv",
            std::process::id()
        ));
        let schema = SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("text", ScalarType::String),
                SchemaNode::scalar("integer", ScalarType::Int),
                SchemaNode::scalar("number", ScalarType::Float),
                SchemaNode::scalar("boolean", ScalarType::Bool),
            ],
        );
        let row = Instance::Group(
            ["text", "integer", "number", "boolean"]
                .into_iter()
                .map(|name| (name.to_string(), Instance::Scalar(Value::Null)))
                .collect(),
        );

        write(&path, &schema, std::slice::from_ref(&row), None, true).unwrap();
        let read_back = read(&path, &schema, None, true).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(read_back, vec![row]);
    }

    #[test]
    fn boolean_columns_accept_word_and_numeric_lexicals() {
        let schema = SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("left", ScalarType::Bool),
                SchemaNode::scalar("right", ScalarType::Bool),
            ],
        );
        assert_eq!(
            from_str("1,false\n0,true\n", &schema, None, false).unwrap(),
            vec![
                Instance::Group(vec![
                    ("left".into(), Instance::Scalar(Value::Bool(true))),
                    ("right".into(), Instance::Scalar(Value::Bool(false))),
                ]),
                Instance::Group(vec![
                    ("left".into(), Instance::Scalar(Value::Bool(false))),
                    ("right".into(), Instance::Scalar(Value::Bool(true))),
                ]),
            ]
        );

        let row = Instance::Group(vec![
            ("left".into(), Instance::Scalar(Value::String(" 1 ".into()))),
            ("right".into(), Instance::Scalar(Value::String("0".into()))),
        ]);
        assert_eq!(
            to_string(&schema, &[row], None, false).unwrap(),
            "true,false\n"
        );
        assert!(matches!(
            from_str("yes,no\n", &schema, None, false),
            Err(CsvFormatError::Parse {
                row: 0,
                expected: ScalarType::Bool,
                ..
            })
        ));
    }

    #[test]
    fn read_rejects_non_finite_float_cells() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_csv_test_non_finite_{}.csv",
            std::process::id()
        ));
        let schema =
            SchemaNode::group("row", vec![SchemaNode::scalar("number", ScalarType::Float)]);

        for value in ["NaN", "inf", "1e999"] {
            std::fs::write(&path, format!("{value}\n")).unwrap();
            assert!(matches!(
                read(&path, &schema, None, false),
                Err(CsvFormatError::Parse {
                    row: 0,
                    ref field,
                    expected: ScalarType::Float,
                    ..
                }) if field == "number"
            ));
        }
        std::fs::remove_file(path).unwrap();
    }
}
