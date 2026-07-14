//! XLSX worksheet instance I/O for row, transposed, composite, and grid mappings.
//!
//! Like CSV, an XLSX table uses a non-repeating group of scalar fields as
//! its row schema. Worksheet coordinates are one-based at this API boundary
//! so project options match normal spreadsheet notation.

use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Range, Reader, Xlsx};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use rust_xlsxwriter::{Workbook, Worksheet};
use thiserror::Error;

mod composite;
mod grid;

pub use composite::{from_bytes_composite, read_composite};
pub use grid::{from_bytes_grid, read_grid};

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
    #[error("expected {expected} row selector(s), got {got}")]
    RowCount { expected: usize, got: usize },
    #[error("row {row}: column `{field}` expected {expected:?}, got `{value}`")]
    Parse {
        row: u32,
        field: String,
        expected: ScalarType,
        value: String,
    },
    #[error("composite XLSX root schema must be a non-repeating group")]
    CompositeRootSchema,
    #[error("invalid composite XLSX schema path `{path}`: {reason}")]
    CompositePath { path: String, reason: &'static str },
    #[error("composite XLSX schema path `{0}` is mapped more than once")]
    DuplicateCompositePath(String),
    #[error("grid XLSX root schema must be a non-repeating group")]
    GridRootSchema,
    #[error("invalid grid XLSX layout: {0}")]
    GridLayout(&'static str),
    #[error("invalid grid XLSX field `{field}`: {reason}")]
    GridField { field: String, reason: &'static str },
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

#[derive(Debug)]
struct TransposedField<'a> {
    name: &'a str,
    ty: ScalarType,
    row: Option<u32>,
}

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
    let range = worksheet_range(bytes, sheet)?;
    rows_from_range(&range, &fields, start_row, &columns, has_header)
}

fn rows_from_range(
    range: &Range<Data>,
    fields: &[(&str, ScalarType)],
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<Vec<Instance>, XlsxFormatError> {
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

/// Reads selected worksheet rows as fields and aligned columns as records.
///
/// Each ordinary scalar field in `schema` consumes one one-based worksheet
/// row selector. An optional field named `n` must be an integer and receives
/// the one-based physical column number instead. The first selected row is
/// the driver: only its non-empty cells produce records, preserving gaps in
/// the synthetic `n` value.
pub fn read_transposed(
    path: &Path,
    schema: &SchemaNode,
    sheet: Option<&str>,
    rows: &[u32],
) -> Result<Vec<Instance>, XlsxFormatError> {
    let bytes = std::fs::read(path)?;
    from_bytes_transposed(&bytes, schema, sheet, rows)
}

/// Reads an XLSX byte buffer with selected rows interpreted as fields.
///
/// This is the in-memory equivalent of [`read_transposed`].
pub fn from_bytes_transposed(
    bytes: &[u8],
    schema: &SchemaNode,
    sheet: Option<&str>,
    rows: &[u32],
) -> Result<Vec<Instance>, XlsxFormatError> {
    let fields = transposed_fields(schema, rows)?;
    let driver_row = rows
        .first()
        .copied()
        .ok_or(XlsxFormatError::InvalidCoordinate)?
        - 1;
    let range = worksheet_range(bytes, sheet)?;
    let Some((last_row, last_column)) = range.end() else {
        return Ok(Vec::new());
    };
    if driver_row > last_row {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for column in 0..=last_column {
        if matches!(
            range.get_value((driver_row, column)),
            None | Some(Data::Empty)
        ) {
            continue;
        }
        let values = fields
            .iter()
            .map(|field| {
                let value = match field.row {
                    Some(row) => parse_cell(
                        range.get_value((row - 1, column)).unwrap_or(&Data::Empty),
                        field.ty,
                        row,
                        field.name,
                    )?,
                    None => Value::Int(i64::from(column + 1)),
                };
                Ok((field.name.to_string(), Instance::Scalar(value)))
            })
            .collect::<Result<Vec<_>, XlsxFormatError>>()?;
        records.push(Instance::Group(values));
    }
    Ok(records)
}

fn worksheet_range(bytes: &[u8], sheet: Option<&str>) -> Result<Range<Data>, XlsxFormatError> {
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
    Ok(workbook.worksheet_range(&sheet)?)
}

fn transposed_fields<'a>(
    schema: &'a SchemaNode,
    rows: &[u32],
) -> Result<Vec<TransposedField<'a>>, XlsxFormatError> {
    let fields = row_fields(schema)?;
    let synthetic_positions = fields
        .iter()
        .enumerate()
        .filter_map(|(index, (name, _))| (*name == "n").then_some(index))
        .collect::<Vec<_>>();
    if synthetic_positions.len() > 1
        || synthetic_positions
            .first()
            .is_some_and(|index| fields[*index].1 != ScalarType::Int)
    {
        return Err(XlsxFormatError::UnsupportedSchema);
    }
    let data_field_count = fields.len() - synthetic_positions.len();
    if rows.len() != data_field_count {
        return Err(XlsxFormatError::RowCount {
            expected: data_field_count,
            got: rows.len(),
        });
    }
    let mut unique = std::collections::BTreeSet::new();
    if rows
        .iter()
        .any(|row| *row == 0 || *row > MAX_WORKSHEET_ROW || !unique.insert(*row))
    {
        return Err(XlsxFormatError::InvalidCoordinate);
    }

    let mut selected_rows = rows.iter().copied();
    fields
        .into_iter()
        .map(|(name, ty)| {
            if name == "n" {
                Ok(TransposedField {
                    name,
                    ty,
                    row: None,
                })
            } else {
                selected_rows
                    .next()
                    .map(|row| TransposedField {
                        name,
                        ty,
                        row: Some(row),
                    })
                    .ok_or(XlsxFormatError::UnsupportedSchema)
            }
        })
        .collect()
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
    use super::composite::validate_composite_layout;
    use super::*;
    use mapping::{
        XlsxColumn, XlsxCompositeLayout, XlsxFixedCell, XlsxFixedRecord, XlsxRow, XlsxTableRegion,
    };

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

    #[test]
    fn transposed_reader_uses_driver_cells_and_physical_column_positions() {
        let schema = SchemaNode::group(
            "records",
            vec![
                SchemaNode::scalar("n", ScalarType::Int),
                SchemaNode::scalar("month", ScalarType::String),
                SchemaNode::scalar("amount", ScalarType::Float),
                SchemaNode::scalar("closed", ScalarType::Bool),
            ],
        );
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Columns").unwrap();
        worksheet.write_string(1, 1, "Jan").unwrap();
        worksheet.write_number(3, 1, 12.5).unwrap();
        worksheet.write_boolean(5, 1, true).unwrap();
        worksheet.write_number(3, 2, 99.0).unwrap();
        worksheet.write_string(1, 3, "Mar").unwrap();
        worksheet.write_number(3, 3, 8.0).unwrap();
        worksheet.write_boolean(5, 3, false).unwrap();
        // A value in a non-driver row must not create a record of its own.
        worksheet.write_number(3, 5, 17.0).unwrap();
        let bytes = workbook.save_to_buffer().unwrap();

        let actual = from_bytes_transposed(&bytes, &schema, Some("Columns"), &[2, 4, 6]).unwrap();

        assert_eq!(
            actual,
            vec![
                Instance::Group(vec![
                    ("n".into(), Instance::Scalar(Value::Int(2))),
                    (
                        "month".into(),
                        Instance::Scalar(Value::String("Jan".into())),
                    ),
                    ("amount".into(), Instance::Scalar(Value::Float(12.5))),
                    ("closed".into(), Instance::Scalar(Value::Bool(true))),
                ]),
                Instance::Group(vec![
                    ("n".into(), Instance::Scalar(Value::Int(4))),
                    (
                        "month".into(),
                        Instance::Scalar(Value::String("Mar".into())),
                    ),
                    ("amount".into(), Instance::Scalar(Value::Float(8.0))),
                    ("closed".into(), Instance::Scalar(Value::Bool(false))),
                ]),
            ]
        );
    }

    #[test]
    fn native_transposed_wrapper_reads_generated_workbook() {
        let schema = SchemaNode::group(
            "records",
            vec![
                SchemaNode::scalar("label", ScalarType::String),
                SchemaNode::scalar("value", ScalarType::Int),
            ],
        );
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.write_string(0, 0, "first").unwrap();
        worksheet.write_number(1, 0, 7.0).unwrap();
        let bytes = workbook.save_to_buffer().unwrap();
        let path = temp_file("transposed");
        std::fs::write(&path, bytes).unwrap();

        let actual = read_transposed(&path, &schema, None, &[1, 2]).unwrap();

        std::fs::remove_file(path).ok();
        assert_eq!(
            actual,
            vec![Instance::Group(vec![
                (
                    "label".into(),
                    Instance::Scalar(Value::String("first".into())),
                ),
                ("value".into(), Instance::Scalar(Value::Int(7))),
            ])]
        );
    }

    #[test]
    fn transposed_reader_validates_rows_and_synthetic_position() {
        let schema = SchemaNode::group(
            "records",
            vec![
                SchemaNode::scalar("label", ScalarType::String),
                SchemaNode::scalar("n", ScalarType::Int),
                SchemaNode::scalar("value", ScalarType::String),
            ],
        );

        let count = transposed_fields(&schema, &[1]).unwrap_err();
        assert!(matches!(
            count,
            XlsxFormatError::RowCount {
                expected: 2,
                got: 1
            }
        ));
        let duplicate = transposed_fields(&schema, &[1, 1]).unwrap_err();
        assert!(matches!(duplicate, XlsxFormatError::InvalidCoordinate));
        let out_of_range = transposed_fields(&schema, &[1, MAX_WORKSHEET_ROW + 1]).unwrap_err();
        assert!(matches!(out_of_range, XlsxFormatError::InvalidCoordinate));

        let invalid_n =
            SchemaNode::group("records", vec![SchemaNode::scalar("n", ScalarType::Float)]);
        let error = transposed_fields(&invalid_n, &[]).unwrap_err();
        assert!(matches!(error, XlsxFormatError::UnsupportedSchema));
    }

    #[test]
    fn transposed_reader_reports_the_selected_row_for_parse_errors() {
        let schema = SchemaNode::group(
            "records",
            vec![
                SchemaNode::scalar("label", ScalarType::String),
                SchemaNode::scalar("amount", ScalarType::Float),
            ],
        );
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.write_string(1, 0, "Jan").unwrap();
        worksheet.write_string(3, 0, "not-a-number").unwrap();
        let bytes = workbook.save_to_buffer().unwrap();

        let error = from_bytes_transposed(&bytes, &schema, None, &[2, 4]).unwrap_err();

        assert!(matches!(
            error,
            XlsxFormatError::Parse { row: 4, field, .. } if field == "amount"
        ));
    }

    fn xlsx_row(value: u32) -> XlsxRow {
        XlsxRow::new(value).unwrap()
    }

    fn xlsx_column(value: u32) -> XlsxColumn {
        XlsxColumn::new(value).unwrap()
    }

    fn composite_schema() -> SchemaNode {
        SchemaNode::group(
            "Workbook",
            vec![
                SchemaNode::scalar("Company", ScalarType::String),
                SchemaNode::group(
                    "Office",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::group(
                            "Address",
                            vec![
                                SchemaNode::scalar("Street", ScalarType::String),
                                SchemaNode::scalar("City", ScalarType::String),
                            ],
                        ),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "Staff",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("Extension", ScalarType::Int),
                        SchemaNode::scalar("Active", ScalarType::Bool),
                    ],
                )
                .repeating(),
                SchemaNode::scalar("Unmapped", ScalarType::String),
            ],
        )
    }

    fn fixed_cell(path: &[&str], row: u32, column: u32) -> XlsxFixedCell {
        XlsxFixedCell {
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
            row: xlsx_row(row),
            column: xlsx_column(column),
        }
    }

    fn composite_layout() -> XlsxCompositeLayout {
        XlsxCompositeLayout {
            table: XlsxTableRegion {
                path: vec!["Staff".into()],
                sheet: Some("Staff".into()),
                start_row: xlsx_row(1),
                columns: vec![xlsx_column(1), xlsx_column(3), xlsx_column(5)],
                has_header: true,
            },
            records: vec![
                XlsxFixedRecord {
                    path: Vec::new(),
                    sheet: Some("Office".into()),
                    cells: vec![fixed_cell(&["Company"], 1, 2)],
                },
                XlsxFixedRecord {
                    path: vec!["Office".into()],
                    sheet: Some("Office".into()),
                    cells: vec![
                        fixed_cell(&["Name"], 2, 2),
                        fixed_cell(&["Address", "Street"], 3, 2),
                        fixed_cell(&["Address", "City"], 4, 2),
                    ],
                },
            ],
        }
    }

    fn composite_workbook() -> Vec<u8> {
        let mut workbook = Workbook::new();
        let office = workbook.add_worksheet();
        office.set_name("Office").unwrap();
        office.write_string(0, 1, "Example Ltd").unwrap();
        office.write_string(1, 1, "West").unwrap();
        office.write_string(2, 1, "Main Street").unwrap();
        let staff = workbook.add_worksheet();
        staff.set_name("Staff").unwrap();
        staff.write_string(0, 0, "First").unwrap();
        staff.write_string(0, 2, "Extension").unwrap();
        staff.write_string(0, 4, "Active").unwrap();
        staff.write_string(1, 0, "Ada").unwrap();
        staff.write_number(1, 2, 41.0).unwrap();
        staff.write_boolean(1, 4, true).unwrap();
        staff.write_string(2, 1, "ignored").unwrap();
        staff.write_string(3, 0, "Lin").unwrap();
        staff.write_number(3, 2, 7.0).unwrap();
        staff.write_boolean(3, 4, false).unwrap();
        workbook.save_to_buffer().unwrap()
    }

    #[test]
    fn composite_reader_materializes_fixed_records_and_sparse_table() {
        let bytes = composite_workbook();
        let actual =
            from_bytes_composite(&bytes, &composite_schema(), &composite_layout()).unwrap();

        assert_eq!(
            actual,
            Instance::Group(vec![
                (
                    "Company".into(),
                    Instance::Scalar(Value::String("Example Ltd".into())),
                ),
                (
                    "Office".into(),
                    Instance::Repeated(vec![Instance::Group(vec![
                        (
                            "Name".into(),
                            Instance::Scalar(Value::String("West".into())),
                        ),
                        (
                            "Address".into(),
                            Instance::Group(vec![
                                (
                                    "Street".into(),
                                    Instance::Scalar(Value::String("Main Street".into())),
                                ),
                                ("City".into(), Instance::Scalar(Value::Null)),
                            ]),
                        ),
                    ])]),
                ),
                (
                    "Staff".into(),
                    Instance::Repeated(vec![
                        Instance::Group(vec![
                            (
                                "First".into(),
                                Instance::Scalar(Value::String("Ada".into())),
                            ),
                            ("Extension".into(), Instance::Scalar(Value::Int(41))),
                            ("Active".into(), Instance::Scalar(Value::Bool(true))),
                        ]),
                        Instance::Group(vec![
                            (
                                "First".into(),
                                Instance::Scalar(Value::String("Lin".into())),
                            ),
                            ("Extension".into(), Instance::Scalar(Value::Int(7))),
                            ("Active".into(), Instance::Scalar(Value::Bool(false))),
                        ]),
                    ]),
                ),
                ("Unmapped".into(), Instance::Scalar(Value::Null)),
            ])
        );

        let path = temp_file("composite");
        std::fs::write(&path, bytes).unwrap();
        let native = read_composite(&path, &composite_schema(), &composite_layout()).unwrap();
        std::fs::remove_file(path).ok();
        assert_eq!(native, actual);
    }

    #[test]
    fn composite_layout_rejects_invalid_paths_kinds_and_collisions() {
        let schema = composite_schema();
        let mut layout = composite_layout();
        layout.table.path = vec!["Company".into()];
        assert!(matches!(
            validate_composite_layout(&schema, &layout),
            Err(XlsxFormatError::CompositePath { .. })
        ));

        let mut layout = composite_layout();
        layout.records[1].cells[0].path = vec!["Address".into()];
        assert!(matches!(
            validate_composite_layout(&schema, &layout),
            Err(XlsxFormatError::CompositePath { .. })
        ));

        let mut layout = composite_layout();
        layout.records[0].cells.push(fixed_cell(&["Company"], 2, 2));
        assert!(matches!(
            validate_composite_layout(&schema, &layout),
            Err(XlsxFormatError::DuplicateCompositePath(path)) if path == "Company"
        ));

        let mut layout = composite_layout();
        layout.table.columns[1] = xlsx_column(1);
        assert!(matches!(
            validate_composite_layout(&schema, &layout),
            Err(XlsxFormatError::InvalidCoordinate)
        ));
    }

    #[test]
    fn composite_reader_reports_missing_named_sheet() {
        let mut layout = composite_layout();
        layout.records[0].sheet = Some("Missing".into());

        let error =
            from_bytes_composite(&composite_workbook(), &composite_schema(), &layout).unwrap_err();

        assert!(matches!(
            error,
            XlsxFormatError::MissingWorksheet(sheet) if sheet == "Missing"
        ));
    }
}
