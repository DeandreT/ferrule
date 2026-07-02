//! Delimited flat file schema and instance read/write, backed by the `csv`
//! crate for correct quoting/escaping.

use std::path::Path;

use ir::{FieldSchema, Record, RecordSchema, ScalarType, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CsvFormatError {
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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
}

/// Reads a CSV file (with a header row) into records shaped by `schema`,
/// parsing each column according to its declared scalar type.
pub fn read(path: &Path, schema: &RecordSchema) -> Result<Vec<Record>, CsvFormatError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)?;
    let mut out = Vec::new();
    for (row_idx, result) in reader.records().enumerate() {
        let raw = result?;
        if raw.len() != schema.fields.len() {
            return Err(CsvFormatError::ColumnCount {
                row: row_idx,
                expected: schema.fields.len(),
                got: raw.len(),
            });
        }
        let mut record = Record::new();
        for (field, cell) in schema.fields.iter().zip(raw.iter()) {
            record.set(field.name.clone(), parse_value(field, cell, row_idx)?);
        }
        out.push(record);
    }
    Ok(out)
}

fn parse_value(field: &FieldSchema, cell: &str, row: usize) -> Result<Value, CsvFormatError> {
    let bad = || CsvFormatError::Parse {
        row,
        field: field.name.clone(),
        expected: field.ty,
        value: cell.to_string(),
    };
    Ok(match field.ty {
        ScalarType::String => Value::String(cell.to_string()),
        ScalarType::Int => Value::Int(cell.parse().map_err(|_| bad())?),
        ScalarType::Float => Value::Float(cell.parse().map_err(|_| bad())?),
        ScalarType::Bool => Value::Bool(cell.parse().map_err(|_| bad())?),
    })
}

/// Writes `records` (shaped by `schema`) to a CSV file with a header row.
pub fn write(path: &Path, schema: &RecordSchema, records: &[Record]) -> Result<(), CsvFormatError> {
    let mut writer = csv::WriterBuilder::new().from_path(path)?;
    writer.write_record(schema.fields.iter().map(|f| f.name.as_str()))?;
    for record in records {
        let row = schema
            .fields
            .iter()
            .map(|f| format_value(record.get(&f.name)));
        writer.write_record(row)?;
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

    fn schema() -> RecordSchema {
        RecordSchema {
            fields: vec![
                FieldSchema {
                    name: "name".into(),
                    ty: ScalarType::String,
                },
                FieldSchema {
                    name: "age".into(),
                    ty: ScalarType::Int,
                },
            ],
        }
    }

    #[test]
    fn write_then_read_roundtrips() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_csv_test_{}.csv",
            std::process::id()
        ));

        let mut record = Record::new();
        record.set("name", Value::String("Jane".into()));
        record.set("age", Value::Int(29));

        write(&path, &schema(), std::slice::from_ref(&record)).unwrap();
        let read_back = read(&path, &schema()).unwrap();

        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_back, vec![record]);
    }

    #[test]
    fn column_count_mismatch_is_reported() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_csv_test_bad_{}.csv",
            std::process::id()
        ));
        std::fs::write(&path, "name,age,extra\nJane,29,x\n").unwrap();

        let err = read(&path, &schema()).unwrap_err();
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
}
