//! Delimited flat file schema and instance read/write, backed by the `csv`
//! crate for correct quoting/escaping.
//!
//! A CSV file's row-schema is a non-repeating [`SchemaNode::Group`] of
//! scalar fields; the file's row-repetition itself is a format convention,
//! not something declared in the schema (unlike XML, where `repeating` is a
//! per-element schema property).

use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use thiserror::Error;

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
    #[error("`{0}` is not a valid CSV delimiter (must be a single-byte character)")]
    BadDelimiter(char),
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
        SchemaKind::Group { children } => children
            .iter()
            .map(|c| match &c.kind {
                SchemaKind::Scalar { ty } if !c.repeating => Ok((c.name.as_str(), *ty)),
                _ => Err(CsvFormatError::UnsupportedSchema),
            })
            .collect(),
        SchemaKind::Scalar { .. } => Err(CsvFormatError::UnsupportedSchema),
    }
}

/// Reads a CSV file (with a header row) into one [`Instance::Group`] per
/// row, parsing each column according to its declared scalar type.
/// `delimiter` defaults to `,`.
pub fn read(
    path: &Path,
    schema: &SchemaNode,
    delimiter: Option<char>,
) -> Result<Vec<Instance>, CsvFormatError> {
    let fields = row_fields(schema)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .delimiter(delimiter_byte(delimiter)?)
        .from_path(path)?;
    let mut out = Vec::new();
    for (row_idx, result) in reader.records().enumerate() {
        let raw = result?;
        if raw.len() != fields.len() {
            return Err(CsvFormatError::ColumnCount {
                row: row_idx,
                expected: fields.len(),
                got: raw.len(),
            });
        }
        let mut row = Vec::with_capacity(fields.len());
        for ((name, ty), cell) in fields.iter().zip(raw.iter()) {
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
    let bad = || CsvFormatError::Parse {
        row,
        field: name.to_string(),
        expected: ty,
        value: cell.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(cell.to_string()),
        ScalarType::Int => Value::Int(cell.parse().map_err(|_| bad())?),
        ScalarType::Float => Value::Float(cell.parse().map_err(|_| bad())?),
        ScalarType::Bool => Value::Bool(cell.parse().map_err(|_| bad())?),
    })
}

/// Writes one row per [`Instance::Group`] in `rows` to a CSV file with a
/// header row. `delimiter` defaults to `,`.
pub fn write(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    delimiter: Option<char>,
) -> Result<(), CsvFormatError> {
    let fields = row_fields(schema)?;
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter_byte(delimiter)?)
        .from_path(path)?;
    writer.write_record(fields.iter().map(|(n, _)| *n))?;
    for row in rows {
        let cells = fields
            .iter()
            .map(|(n, _)| format_value(row.field(n).and_then(Instance::as_scalar)));
        writer.write_record(cells)?;
    }
    writer.flush()?;
    Ok(())
}

fn format_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::Float(f)) => f.to_string(),
        Some(Value::String(s)) => s.clone(),
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
    fn write_then_read_roundtrips() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_csv_test_{}.csv",
            std::process::id()
        ));

        let row = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(29))),
        ]);

        write(&path, &schema(), std::slice::from_ref(&row), None).unwrap();
        let read_back = read(&path, &schema(), None).unwrap();

        std::fs::remove_file(&path).unwrap();
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

        let err = read(&path, &schema(), None).unwrap_err();
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
    fn custom_delimiter_roundtrips() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_csv_test_semi_{}.csv",
            std::process::id()
        ));

        let row = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("Jane;Doe".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(29))),
        ]);

        write(&path, &schema(), std::slice::from_ref(&row), Some(';')).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let read_back = read(&path, &schema(), Some(';')).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(text.starts_with("name;age"));
        // The value containing the delimiter must be quoted, not split.
        assert_eq!(read_back, vec![row]);
    }
}
