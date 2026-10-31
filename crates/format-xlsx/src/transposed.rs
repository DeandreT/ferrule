//! Transposed worksheets: selected physical rows are fields and columns are
//! records.
//!
//! Each ordinary scalar field consumes one one-based row selector, in schema
//! order. An optional integer field named `n` carries the record's one-based
//! physical column instead of occupying a row. The first selected row is the
//! driver: a reader creates one record for each non-empty driver cell.
//!
//! The writer is the reader's inverse. Record `k` goes to column `k + 1`, or
//! to column `n` when the schema has that field. There is no header row. A
//! record whose driver value is empty is refused because the reader would
//! skip its column. Without `n`, every record must fill every row: the
//! reference application writes each row as its own run of cells from column
//! A, so a missing value would move the rest of that row left and out of line
//! with the other rows. With `n`, a missing value leaves its cell empty.

use std::collections::BTreeMap;
use std::path::Path;

use calamine::Data;
use ir::{Instance, ScalarType, SchemaNode, Value};
use rust_xlsxwriter::Workbook;

use super::{
    CellValue, MAX_WORKSHEET_COLUMN, MAX_WORKSHEET_ROW, XlsxFormatError, cell_value, lexical_i64,
    parse_cell, row_fields, u16_column, validate_row, worksheet_range, write_cell,
};

/// The synthetic field that carries a record's physical column.
const POSITION_FIELD: &str = "n";

#[derive(Debug)]
pub(crate) struct TransposedField<'a> {
    name: &'a str,
    ty: ScalarType,
    row: Option<u32>,
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
        records.push(Instance::Group((values).into()));
    }
    Ok(records)
}

/// Writes a new workbook containing one transposed table.
///
/// The complete workbook is validated and built before the file is written,
/// so an invalid record leaves `path` untouched.
pub fn write_transposed(
    path: &Path,
    schema: &SchemaNode,
    records: &[Instance],
    sheet: Option<&str>,
    rows: &[u32],
) -> Result<(), XlsxFormatError> {
    let bytes = to_bytes_transposed(schema, records, sheet, rows)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Writes a new workbook containing one transposed table into a byte buffer.
///
/// See the module documentation for the column and empty-value rules.
pub fn to_bytes_transposed(
    schema: &SchemaNode,
    records: &[Instance],
    sheet: Option<&str>,
    rows: &[u32],
) -> Result<Vec<u8>, XlsxFormatError> {
    let fields = transposed_fields(schema, rows)?;
    let row_schema = row_fields(schema)?;
    let positioned = fields.iter().any(|field| field.row.is_none());
    if !positioned && records.len() > MAX_WORKSHEET_COLUMN as usize {
        return Err(XlsxFormatError::TransposedRecordCount {
            count: records.len(),
        });
    }

    let mut columns = BTreeMap::new();
    let mut cells = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        let values = validate_row(index, record, &row_schema)?;
        let mut column =
            u32::try_from(index + 1).map_err(|_| XlsxFormatError::InvalidCoordinate)?;
        let mut record_cells = Vec::with_capacity(fields.len());
        let mut driver = true;
        for (field, value) in fields.iter().zip(values) {
            let Some(row) = field.row else {
                column = record_column(index, value)?;
                continue;
            };
            let cell = cell_value(field.name, field.ty, value, index)?;
            let empty = matches!(&cell, CellValue::Empty);
            if driver && (empty || matches!(&cell, CellValue::String(text) if text.is_empty())) {
                return Err(XlsxFormatError::EmptyTransposedDriver {
                    record: index,
                    field: field.name.to_string(),
                });
            }
            if empty && !positioned {
                return Err(XlsxFormatError::TransposedGap {
                    record: index,
                    field: field.name.to_string(),
                });
            }
            driver = false;
            record_cells.push((field, row, value));
        }
        if let Some(first) = columns.insert(column, index) {
            return Err(XlsxFormatError::DuplicateTransposedColumn {
                record: index,
                first,
                column,
            });
        }
        cells.push((column, record_cells));
    }

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    if let Some(sheet) = sheet {
        worksheet.set_name(sheet)?;
    }
    for (index, (column, record_cells)) in cells.into_iter().enumerate() {
        let column = u16_column(column - 1)?;
        for (field, row, value) in record_cells {
            write_cell(
                worksheet,
                row - 1,
                column,
                field.name,
                field.ty,
                value,
                index,
            )?;
        }
    }
    Ok(workbook.save_to_buffer()?)
}

fn record_column(record: usize, value: &Value) -> Result<u32, XlsxFormatError> {
    let column = match value {
        Value::Int(value) => Some(*value),
        Value::String(value) => lexical_i64(value),
        Value::Null | Value::JsonNull(_) => {
            return Err(XlsxFormatError::TransposedColumn {
                record,
                reason: "is missing",
            });
        }
        other => {
            return Err(XlsxFormatError::ValueType {
                row: record,
                field: POSITION_FIELD.to_string(),
                expected: ScalarType::Int,
                got: other.type_name(),
            });
        }
    };
    column
        .and_then(|column| u32::try_from(column).ok())
        .filter(|column| (1..=MAX_WORKSHEET_COLUMN).contains(column))
        .ok_or(XlsxFormatError::TransposedColumn {
            record,
            reason: "must be a column number from 1 to 16384",
        })
}

pub(crate) fn transposed_fields<'a>(
    schema: &'a SchemaNode,
    rows: &[u32],
) -> Result<Vec<TransposedField<'a>>, XlsxFormatError> {
    let fields = row_fields(schema)?;
    let synthetic_positions = fields
        .iter()
        .enumerate()
        .filter_map(|(index, (name, _))| (*name == POSITION_FIELD).then_some(index))
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
            if name == POSITION_FIELD {
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

#[cfg(test)]
#[path = "transposed_tests.rs"]
mod tests;
