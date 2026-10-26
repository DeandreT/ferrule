use std::io::{self, Write};

use ir::{Instance, SchemaNode, Value};
use rust_xlsxwriter::Workbook;

use super::{XlsxLimit, XlsxLimits, add, check, fields};
use crate::{
    CellValue, FlatTableWriteOptions, XlsxFormatError, cell_value, column_indexes, u16_column,
    validate_row, write_cell,
};

/// Creates a new flat workbook with finite preflight and output budgets.
/// Equivalent to [`crate::to_bytes`] for accepted values; never updates a file.
pub fn to_bytes_bounded(
    schema: &SchemaNode,
    rows: &[Instance],
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
    limits: &XlsxLimits,
) -> Result<Vec<u8>, XlsxFormatError> {
    to_bytes_with_options_bounded(
        schema,
        rows,
        FlatTableWriteOptions {
            sheet,
            start_row,
            columns,
            headers: &[],
            has_header,
        },
        limits,
    )
}

pub(super) struct PreparedFlat<'a> {
    fields: Vec<(&'a str, ir::ScalarType)>,
    columns: Vec<u32>,
}

pub(super) fn preflight<'a>(
    schema: &'a SchemaNode,
    rows: &[Instance],
    options: FlatTableWriteOptions<'_>,
    limits: &XlsxLimits,
) -> Result<PreparedFlat<'a>, XlsxFormatError> {
    let limits = limits.options();
    let fields = fields(schema, limits)?;
    let columns = column_indexes(fields.len(), options.columns)?;
    if options.start_row == 0 || options.start_row > crate::MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    if !options.headers.is_empty() && options.headers.len() != fields.len() {
        return Err(XlsxFormatError::HeaderCount {
            expected: fields.len(),
            got: options.headers.len(),
        });
    }
    let total_rows = rows.len().saturating_add(usize::from(options.has_header));
    check(XlsxLimit::Rows, rows.len(), limits.max_rows)?;
    check(
        XlsxLimit::Rows,
        options.start_row as usize + total_rows.saturating_sub(1),
        limits.max_rows,
    )?;
    for column in &columns {
        check(XlsxLimit::Columns, *column as usize + 1, limits.max_columns)?;
    }
    let cells = total_rows.saturating_mul(fields.len());
    check(XlsxLimit::Cells, cells, limits.max_cells)?;
    check(
        XlsxLimit::MaterializedCells,
        cells,
        limits.max_materialized_cells,
    )?;
    let width = columns
        .iter()
        .max()
        .zip(columns.iter().min())
        .map_or(0, |(max, min)| (max - min + 1) as usize);
    check(
        XlsxLimit::DenseCells,
        total_rows.saturating_mul(width),
        limits.max_dense_cells,
    )?;
    let mut text = 0;
    let mut growth = 64 * 1024; // Conservative fixed package XML allowance.
    add(
        &mut growth,
        cells.saturating_mul(256),
        XlsxLimit::ExpandedBytes,
        limits.max_expanded_bytes,
    )?;
    let mut strings = 0;
    let mut charge =
        |value: &str, is_string: bool, expansion: usize| -> Result<(), XlsxFormatError> {
            check(XlsxLimit::StringBytes, value.len(), limits.max_string_bytes)?;
            add(
                &mut text,
                value.len(),
                XlsxLimit::XmlTextBytes,
                limits.max_xml_text_bytes,
            )?;
            check(
                XlsxLimit::MaterializedTextBytes,
                text,
                limits.max_materialized_text_bytes,
            )?;
            // XML escaping and Excel's _xHHHH_ encoding expand each source byte
            // by at most seven bytes. Count duplicates too, not just unique strings.
            add(
                &mut growth,
                value.len().saturating_mul(expansion),
                XlsxLimit::ExpandedBytes,
                limits.max_expanded_bytes,
            )?;
            if is_string {
                add(
                    &mut strings,
                    1,
                    XlsxLimit::SharedStrings,
                    limits.max_shared_strings,
                )?;
            }
            Ok(())
        };
    if let Some(sheet) = options.sheet {
        charge(sheet, false, 7)?;
    }
    if options.has_header {
        for (index, (name, _)) in fields.iter().enumerate() {
            charge(
                options.headers.get(index).map_or(*name, String::as_str),
                true,
                7,
            )?;
        }
    }
    for (index, row) in rows.iter().enumerate() {
        // Bound malformed groups before validate_row's duplicate scan.
        if let Instance::Group(values) = row {
            check(XlsxLimit::Columns, values.len(), limits.max_columns)?;
            for (name, value) in values.iter() {
                charge(name, false, 7)?;
                if let Instance::Scalar(Value::String(value)) = value {
                    charge(value, false, 7)?;
                }
            }
        }
        let values = validate_row(index, row, &fields)?;
        for ((name, ty), value) in fields.iter().zip(values) {
            match cell_value(name, *ty, value, index)? {
                CellValue::String(value) => charge(&value, true, 7)?,
                // Pinned rust_xlsxwriter's default write_number_cell uses
                // Display, not a fixed-size scientific representation. Charge
                // its actual decimal text in addition to structural overhead.
                CellValue::Number(value) => charge(&value.to_string(), false, 1)?,
                CellValue::Empty | CellValue::Bool(_) => {}
            }
        }
    }
    // Worksheet XML is the largest generated individual part in this profile;
    // charging all package growth is conservative and avoids a false guarantee.
    check(XlsxLimit::EntryBytes, growth, limits.max_entry_bytes)?;
    check(
        XlsxLimit::XmlNodes,
        cells.saturating_mul(16).saturating_add(2048),
        limits.max_xml_nodes,
    )?;
    check(XlsxLimit::Entries, 10, limits.max_entries)?;
    check(XlsxLimit::XmlDepth, 32, limits.max_xml_depth)?;
    Ok(PreparedFlat { fields, columns })
}

/// Creates a new flat workbook, retaining physical headers and sparse columns.
/// Rows, cell values, string storage and a conservative XML growth charge are
/// checked before constructing the workbook. The ZIP writer cannot grow the
/// returned buffer beyond `max_output_bytes`.
pub fn to_bytes_with_options_bounded(
    schema: &SchemaNode,
    rows: &[Instance],
    options: FlatTableWriteOptions<'_>,
    limits: &XlsxLimits,
) -> Result<Vec<u8>, XlsxFormatError> {
    let PreparedFlat { fields, columns } = preflight(schema, rows, options, limits)?;
    let limits = limits.options();
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    if let Some(sheet) = options.sheet {
        worksheet.set_name(sheet)?;
    }
    let start = options.start_row - 1;
    if options.has_header {
        for (index, ((name, _), column)) in fields.iter().zip(&columns).enumerate() {
            worksheet.write_string(
                start,
                u16_column(*column)?,
                options.headers.get(index).map_or(*name, String::as_str),
            )?;
        }
    }
    for (offset, row) in rows.iter().enumerate() {
        let values = validate_row(offset, row, &fields)?;
        for (((name, ty), value), column) in fields.iter().zip(values).zip(&columns) {
            write_cell(
                worksheet,
                start + u32::from(options.has_header) + offset as u32,
                u16_column(*column)?,
                name,
                *ty,
                value,
                offset,
            )?;
        }
    }
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        maximum: limits.max_output_bytes,
        exceeded: None,
    };
    let result = workbook.save_to_writer(&mut output);
    if let Some(actual) = output.exceeded {
        return Err(XlsxFormatError::LimitExceeded {
            limit: XlsxLimit::OutputBytes,
            maximum: limits.max_output_bytes,
            actual,
        });
    }
    result?;
    // Recheck actual generated parts too: library changes must not silently
    // weaken the conservative template-growth estimates above.
    super::package::validate(&output.bytes, limits)?;
    Ok(output.bytes)
}

struct BoundedOutput {
    bytes: Vec<u8>,
    maximum: usize,
    exceeded: Option<usize>,
}

impl Write for BoundedOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let actual = self.bytes.len().saturating_add(buf.len());
        if actual > self.maximum {
            self.exceeded = Some(actual);
            return Err(io::Error::other("XLSX output budget exceeded"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
