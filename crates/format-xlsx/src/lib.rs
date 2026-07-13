//! XLSX worksheet instance I/O for flat row mappings.
//!
//! Like CSV, an XLSX table uses a non-repeating group of scalar fields as
//! its row schema. Worksheet coordinates are one-based at this API boundary
//! so project options match normal spreadsheet notation.

use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Reader, Xlsx};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use rust_xlsxwriter::{Workbook, Worksheet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XlsxFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xlsx read error: {0}")]
    Read(#[from] calamine::XlsxError),
    #[error("xlsx write error: {0}")]
    Write(#[from] rust_xlsxwriter::XlsxError),
    #[error("workbook contains no worksheets")]
    NoWorksheets,
    #[error("worksheet `{0}` does not exist")]
    MissingWorksheet(String),
    #[error("row schema must be a non-repeating group of non-repeating scalar fields")]
    UnsupportedSchema,
    #[error("worksheet coordinates are outside Excel's row or column limits")]
    InvalidCoordinate,
    #[error("expected {expected} column selector(s), got {got}")]
    ColumnCount { expected: usize, got: usize },
    #[error("row {row}: column `{field}` expected {expected:?}, got `{value}`")]
    Parse {
        row: u32,
        field: String,
        expected: ScalarType,
        value: String,
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
}

const MAX_EXACT_F64_INTEGER: i64 = 1_i64 << f64::MANTISSA_DIGITS;
const MAX_WORKSHEET_ROW: u32 = 1_048_576;
const MAX_WORKSHEET_COLUMN: u32 = 16_384;

fn row_fields(schema: &SchemaNode) -> Result<Vec<(&str, ScalarType)>, XlsxFormatError> {
    if schema.repeating {
        return Err(XlsxFormatError::UnsupportedSchema);
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .map(|child| match child.kind {
                SchemaKind::Scalar { ty } if !child.repeating && !child.attribute => {
                    Ok((child.name.as_str(), ty))
                }
                _ => Err(XlsxFormatError::UnsupportedSchema),
            })
            .collect(),
        SchemaKind::Scalar { .. } => Err(XlsxFormatError::UnsupportedSchema),
    }
}

fn column_indexes(field_count: usize, columns: &[u32]) -> Result<Vec<u32>, XlsxFormatError> {
    let columns = if columns.is_empty() {
        (1..=u32::try_from(field_count).map_err(|_| XlsxFormatError::ColumnCount {
            expected: field_count,
            got: columns.len(),
        })?)
            .collect()
    } else {
        if columns.len() != field_count {
            return Err(XlsxFormatError::ColumnCount {
                expected: field_count,
                got: columns.len(),
            });
        }
        columns.to_vec()
    };
    let mut unique = std::collections::BTreeSet::new();
    if columns
        .iter()
        .any(|column| *column == 0 || !unique.insert(*column))
    {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    if columns.iter().any(|column| *column > MAX_WORKSHEET_COLUMN) {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    Ok(columns.into_iter().map(|column| column - 1).collect())
}

/// Reads the selected worksheet table into one group per data row.
pub fn read(
    path: &Path,
    schema: &SchemaNode,
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<Vec<Instance>, XlsxFormatError> {
    let bytes = std::fs::read(path)?;
    from_bytes(&bytes, schema, sheet, start_row, columns, has_header)
}

/// Reads an XLSX byte buffer into one group per selected worksheet row.
///
/// This is the in-memory equivalent of [`read`], suitable for hosts without
/// filesystem access such as WebAssembly applications.
pub fn from_bytes(
    bytes: &[u8],
    schema: &SchemaNode,
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<Vec<Instance>, XlsxFormatError> {
    if start_row == 0 || start_row > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    let fields = row_fields(schema)?;
    let columns = column_indexes(fields.len(), columns)?;
    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let sheet = match sheet {
        Some(sheet) => sheet.to_string(),
        None => workbook
            .sheet_names()
            .first()
            .cloned()
            .ok_or(XlsxFormatError::NoWorksheets)?,
    };
    if !workbook.sheet_names().iter().any(|name| name == &sheet) {
        return Err(XlsxFormatError::MissingWorksheet(sheet));
    }
    let range = workbook.worksheet_range(&sheet)?;
    let Some((last_row, _)) = range.end() else {
        return Ok(Vec::new());
    };
    let first_data_row = start_row - 1 + u32::from(has_header);
    if first_data_row > last_row {
        return Ok(Vec::new());
    }

    let mut rows = Vec::new();
    for row in first_data_row..=last_row {
        let cells = columns
            .iter()
            .map(|column| range.get_value((row, *column)).unwrap_or(&Data::Empty))
            .collect::<Vec<_>>();
        if cells.iter().all(|cell| matches!(cell, Data::Empty)) {
            continue;
        }
        let values = fields
            .iter()
            .zip(cells)
            .map(|((name, ty), cell)| {
                parse_cell(cell, *ty, row + 1, name)
                    .map(|value| ((*name).to_string(), Instance::Scalar(value)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(Instance::Group(values));
    }
    Ok(rows)
}

fn parse_cell(
    cell: &Data,
    ty: ScalarType,
    row: u32,
    field: &str,
) -> Result<Value, XlsxFormatError> {
    if matches!(cell, Data::Empty) {
        return Ok(Value::Null);
    }
    let bad = || XlsxFormatError::Parse {
        row,
        field: field.to_string(),
        expected: ty,
        value: cell.to_string(),
    };
    match ty {
        ScalarType::String => match cell {
            Data::Error(_) => Err(bad()),
            _ => Ok(Value::String(cell.to_string())),
        },
        ScalarType::Int => match cell {
            Data::Int(value) => Ok(Value::Int(*value)),
            Data::Float(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && *value >= i64::MIN as f64
                    && *value < i64::MAX as f64 =>
            {
                Ok(Value::Int(*value as i64))
            }
            Data::String(value) => value.parse().map(Value::Int).map_err(|_| bad()),
            _ => Err(bad()),
        },
        ScalarType::Float => match cell {
            Data::Int(value) => exact_f64(*value).map(Value::Float).ok_or_else(bad),
            Data::Float(value) if value.is_finite() => Ok(Value::Float(*value)),
            Data::String(value) => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(Value::Float)
                .ok_or_else(bad),
            _ => Err(bad()),
        },
        ScalarType::Bool => match cell {
            Data::Bool(value) => Ok(Value::Bool(*value)),
            Data::String(value) => value.parse().map(Value::Bool).map_err(|_| bad()),
            _ => Err(bad()),
        },
    }
}

/// Writes a new workbook containing the selected flat worksheet table.
pub fn write(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<(), XlsxFormatError> {
    let bytes = to_bytes(schema, rows, sheet, start_row, columns, has_header)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Writes a new workbook containing the selected table into an XLSX byte
/// buffer.
///
/// This validates and materializes the entire workbook before returning, so
/// callers can avoid replacing an existing file when a row is invalid.
pub fn to_bytes(
    schema: &SchemaNode,
    rows: &[Instance],
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<Vec<u8>, XlsxFormatError> {
    if start_row == 0 || start_row > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    let fields = row_fields(schema)?;
    let columns = column_indexes(fields.len(), columns)?;
    let records = rows
        .iter()
        .enumerate()
        .map(|(row, instance)| validate_row(row, instance, &fields))
        .collect::<Result<Vec<_>, _>>()?;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    if let Some(sheet) = sheet {
        worksheet.set_name(sheet)?;
    }
    let table_row = start_row - 1;
    if has_header {
        for ((name, _), column) in fields.iter().zip(&columns) {
            worksheet.write_string(table_row, u16_column(*column)?, *name)?;
        }
    }
    let data_row = table_row
        .checked_add(u32::from(has_header))
        .ok_or(XlsxFormatError::InvalidCoordinate)?;
    for (offset, record) in records.iter().enumerate() {
        let row = data_row
            .checked_add(u32::try_from(offset).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        if row >= MAX_WORKSHEET_ROW {
            return Err(XlsxFormatError::InvalidCoordinate);
        }
        for (((name, ty), value), column) in fields.iter().zip(record).zip(&columns) {
            write_cell(
                worksheet,
                row,
                u16_column(*column)?,
                name,
                *ty,
                value,
                offset,
            )?;
        }
    }
    Ok(workbook.save_to_buffer()?)
}

fn u16_column(column: u32) -> Result<u16, XlsxFormatError> {
    u16::try_from(column).map_err(|_| XlsxFormatError::InvalidCoordinate)
}

fn validate_row<'a>(
    row: usize,
    instance: &'a Instance,
    fields: &[(&str, ScalarType)],
) -> Result<Vec<&'a Value>, XlsxFormatError> {
    let Instance::Group(values) = instance else {
        return Err(XlsxFormatError::RowShape {
            row,
            got: instance_type_name(instance),
        });
    };
    for (index, (name, _)) in values.iter().enumerate() {
        if !fields.iter().any(|(field, _)| field == name) {
            return Err(XlsxFormatError::UnexpectedField {
                row,
                field: name.clone(),
            });
        }
        if values[..index].iter().any(|(previous, _)| previous == name) {
            return Err(XlsxFormatError::DuplicateField {
                row,
                field: name.clone(),
            });
        }
    }
    fields
        .iter()
        .map(|(name, ty)| {
            values
                .iter()
                .find(|(candidate, _)| candidate == name)
                .ok_or_else(|| XlsxFormatError::MissingField {
                    row,
                    field: (*name).to_string(),
                })
                .and_then(|(_, instance)| match instance {
                    Instance::Scalar(value) => Ok(value),
                    other => Err(XlsxFormatError::ValueType {
                        row,
                        field: (*name).to_string(),
                        expected: *ty,
                        got: instance_type_name(other),
                    }),
                })
        })
        .collect()
}

fn write_cell(
    worksheet: &mut Worksheet,
    row: u32,
    column: u16,
    field: &str,
    ty: ScalarType,
    value: &Value,
    row_index: usize,
) -> Result<(), XlsxFormatError> {
    let bad = |got| XlsxFormatError::ValueType {
        row: row_index,
        field: field.to_string(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (_, Value::Null) => {}
        (ScalarType::String, Value::String(value)) => {
            worksheet.write_string(row, column, value)?;
        }
        (ScalarType::String, Value::Bool(value)) => {
            worksheet.write_boolean(row, column, *value)?;
        }
        (ScalarType::String, Value::Int(value)) => {
            worksheet.write_string(row, column, value.to_string())?;
        }
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            worksheet.write_string(row, column, value.to_string())?;
        }
        (ScalarType::Int, Value::Int(value)) => {
            let number = exact_f64(*value).ok_or_else(|| bad("int outside the exact f64 range"))?;
            worksheet.write_number(row, column, number)?;
        }
        (ScalarType::Float, Value::Int(value)) => {
            let number = exact_f64(*value).ok_or_else(|| bad("int outside the exact f64 range"))?;
            worksheet.write_number(row, column, number)?;
        }
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => {
            worksheet.write_number(row, column, *value)?;
        }
        (ScalarType::Bool, Value::Bool(value)) => {
            worksheet.write_boolean(row, column, *value)?;
        }
        (_, value) => return Err(bad(value.type_name())),
    }
    Ok(())
}

fn exact_f64(value: i64) -> Option<f64> {
    (-MAX_EXACT_F64_INTEGER..=MAX_EXACT_F64_INTEGER)
        .contains(&value)
        .then_some(value as f64)
}

fn instance_type_name(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(value) => value.type_name(),
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated",
        Instance::MappedSequence(_) => "mapped sequence",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "sales",
            vec![
                SchemaNode::scalar("month", ScalarType::String),
                SchemaNode::scalar("amount", ScalarType::Float),
                SchemaNode::scalar("closed", ScalarType::Bool),
            ],
        )
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ferrule-xlsx-{name}-{}-{}.xlsx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    fn rows() -> Vec<Instance> {
        vec![
            Instance::Group(vec![
                (
                    "month".into(),
                    Instance::Scalar(Value::String("Jan".into())),
                ),
                ("amount".into(), Instance::Scalar(Value::Float(12.5))),
                ("closed".into(), Instance::Scalar(Value::Bool(true))),
            ]),
            Instance::Group(vec![
                (
                    "month".into(),
                    Instance::Scalar(Value::String("Feb".into())),
                ),
                ("amount".into(), Instance::Scalar(Value::Null)),
                ("closed".into(), Instance::Scalar(Value::Bool(false))),
            ]),
        ]
    }

    #[test]
    fn native_wrappers_write_and_read_selected_sheet_columns() {
        let path = temp_file("roundtrip");
        let rows = rows();

        write(
            &path,
            &schema(),
            &rows,
            Some("Revenue"),
            3,
            &[2, 4, 6],
            true,
        )
        .unwrap();
        let actual = read(&path, &schema(), Some("Revenue"), 3, &[2, 4, 6], true).unwrap();
        std::fs::remove_file(path).ok();
        assert_eq!(actual, rows);
    }

    #[test]
    fn byte_reader_selects_header_and_non_contiguous_columns() {
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Revenue").unwrap();
        worksheet.write_string(2, 1, "month").unwrap();
        worksheet.write_string(2, 3, "amount").unwrap();
        worksheet.write_string(2, 5, "closed").unwrap();
        worksheet.write_string(3, 0, "ignored").unwrap();
        worksheet.write_string(3, 1, "Jan").unwrap();
        worksheet.write_number(3, 2, 999.0).unwrap();
        worksheet.write_number(3, 3, 12.5).unwrap();
        worksheet.write_boolean(3, 5, true).unwrap();
        let bytes = workbook.save_to_buffer().unwrap();

        let actual = from_bytes(&bytes, &schema(), Some("Revenue"), 3, &[2, 4, 6], true).unwrap();

        assert_eq!(actual, vec![rows().remove(0)]);
    }

    #[test]
    fn byte_reader_reports_a_missing_worksheet() {
        let bytes = to_bytes(&schema(), &[], Some("Revenue"), 1, &[], true).unwrap();

        let error = from_bytes(&bytes, &schema(), Some("Expenses"), 1, &[], true).unwrap_err();

        assert!(matches!(
            error,
            XlsxFormatError::MissingWorksheet(sheet) if sheet == "Expenses"
        ));
    }

    #[test]
    fn byte_writer_rejects_lossy_and_non_finite_numbers() {
        let int_schema = SchemaNode::group("rows", vec![SchemaNode::scalar("id", ScalarType::Int)]);
        let lossy = vec![Instance::Group(vec![(
            "id".into(),
            Instance::Scalar(Value::Int(MAX_EXACT_F64_INTEGER + 1)),
        )])];
        let error = to_bytes(&int_schema, &lossy, None, 1, &[], false).unwrap_err();
        assert!(matches!(error, XlsxFormatError::ValueType { field, .. } if field == "id"));

        let float_schema = SchemaNode::group(
            "rows",
            vec![SchemaNode::scalar("amount", ScalarType::Float)],
        );
        let non_finite = vec![Instance::Group(vec![(
            "amount".into(),
            Instance::Scalar(Value::Float(f64::INFINITY)),
        )])];
        let error = to_bytes(&float_schema, &non_finite, None, 1, &[], false).unwrap_err();
        assert!(matches!(error, XlsxFormatError::ValueType { field, .. } if field == "amount"));
    }

    #[test]
    fn byte_writer_rejects_coordinates_outside_excel_limits() {
        let invalid_row = to_bytes(&schema(), &[], None, 0, &[], false).unwrap_err();
        assert!(matches!(invalid_row, XlsxFormatError::InvalidCoordinate));

        let invalid_column = to_bytes(
            &schema(),
            &[],
            None,
            1,
            &[1, 2, MAX_WORKSHEET_COLUMN + 1],
            false,
        )
        .unwrap_err();
        assert!(matches!(invalid_column, XlsxFormatError::InvalidCoordinate));

        let duplicate_column = to_bytes(&schema(), &[], None, 1, &[1, 2, 2], false).unwrap_err();
        assert!(matches!(
            duplicate_column,
            XlsxFormatError::InvalidCoordinate
        ));
    }

    #[test]
    fn rejects_misaligned_column_selectors() {
        let error = column_indexes(3, &[1, 2]).unwrap_err();
        assert!(matches!(
            error,
            XlsxFormatError::ColumnCount {
                expected: 3,
                got: 2
            }
        ));
    }
}
