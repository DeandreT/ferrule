use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Range, Reader, Xlsx};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    XlsxCellKind, XlsxHierarchicalLayout, XlsxOutputColumn, XlsxOutputRange, XlsxRangeStart,
};
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet};

use super::{MAX_EXACT_F64_INTEGER, MAX_WORKSHEET_ROW, XlsxFormatError};

/// Reads a workbook written with a retained hierarchical layout.
pub fn read_hierarchical(
    path: &Path,
    schema: &SchemaNode,
    layout: &XlsxHierarchicalLayout,
) -> Result<Instance, XlsxFormatError> {
    let bytes = std::fs::read(path)?;
    from_bytes_hierarchical(&bytes, schema, layout)
}

/// Reads a hierarchical workbook from memory.
pub fn from_bytes_hierarchical(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &XlsxHierarchicalLayout,
) -> Result<Instance, XlsxFormatError> {
    validate_layout(schema, layout)?;
    let worksheet_schema = schema_at(schema, &layout.worksheets_path)?;
    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(XlsxFormatError::NoWorksheets);
    }
    let mut worksheets = Vec::with_capacity(sheet_names.len());
    for name in sheet_names {
        let range = workbook.worksheet_range(&name)?;
        worksheets.push(read_worksheet(&range, worksheet_schema, &name, layout)?);
    }
    let mut assignments = BTreeMap::new();
    assignments.insert(
        layout.worksheets_path.clone(),
        Instance::Repeated(worksheets),
    );
    Ok(materialize_schema(schema, &mut Vec::new(), &assignments))
}

fn read_worksheet(
    cells: &Range<Data>,
    schema: &SchemaNode,
    name: &str,
    layout: &XlsxHierarchicalLayout,
) -> Result<Instance, XlsxFormatError> {
    let mut assignments = BTreeMap::new();
    assignments.insert(
        layout.worksheet_name_path.clone(),
        Instance::Scalar(Value::String(name.to_string())),
    );
    let mut previous_end = 0_u32;
    for range in &layout.ranges {
        let start = range_start(range, previous_end)?;
        let data_start = start
            .checked_add(u32::from(range.has_header))
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        let row_schema = schema_at(schema, &range.path)?;
        let mut rows = Vec::new();
        match range.count {
            Some(count) => {
                for offset in 0..count.get() {
                    let row = data_start
                        .checked_add(offset)
                        .ok_or(XlsxFormatError::InvalidCoordinate)?;
                    rows.push(read_range_row(cells, row_schema, row, &range.columns)?);
                }
            }
            None => {
                let mut row = data_start;
                while row <= MAX_WORKSHEET_ROW && !range_row_is_empty(cells, row, &range.columns) {
                    rows.push(read_range_row(cells, row_schema, row, &range.columns)?);
                    row = row
                        .checked_add(1)
                        .ok_or(XlsxFormatError::InvalidCoordinate)?;
                }
            }
        }
        let next_row = data_start
            .checked_add(u32::try_from(rows.len()).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        let actual_end = next_row.saturating_sub(1);
        previous_end = if let Some(count) = range.count {
            start
                .checked_add(u32::from(range.has_header))
                .and_then(|value| value.checked_add(count.get()))
                .and_then(|value| value.checked_sub(1))
                .ok_or(XlsxFormatError::InvalidCoordinate)?
                .max(actual_end)
        } else {
            actual_end
        };
        if previous_end > MAX_WORKSHEET_ROW {
            return Err(XlsxFormatError::InvalidCoordinate);
        }
        let value = if row_schema.repeating {
            Instance::Repeated(rows)
        } else {
            rows.into_iter()
                .next()
                .ok_or_else(|| XlsxFormatError::HierarchicalValue {
                    path: range.path.join("/"),
                    reason: "fixed singleton row is unavailable",
                })?
        };
        assignments.insert(range.path.clone(), value);
    }
    Ok(materialize_schema(schema, &mut Vec::new(), &assignments))
}

fn range_start(range: &XlsxOutputRange, previous_end: u32) -> Result<u32, XlsxFormatError> {
    let start = match range.start {
        XlsxRangeStart::Absolute { row } => row.get(),
        XlsxRangeStart::AfterPrevious { offset } => previous_end
            .checked_add(offset.get())
            .ok_or(XlsxFormatError::InvalidCoordinate)?,
    };
    if start == 0 || start > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    Ok(start)
}

fn range_row_is_empty(cells: &Range<Data>, row: u32, columns: &[XlsxOutputColumn]) -> bool {
    columns.iter().all(|column| {
        matches!(
            cells.get_value((row - 1, column.column.get() - 1)),
            None | Some(Data::Empty)
        )
    })
}

fn read_range_row(
    cells: &Range<Data>,
    schema: &SchemaNode,
    row: u32,
    columns: &[XlsxOutputColumn],
) -> Result<Instance, XlsxFormatError> {
    if row == 0 || row > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    let mut assignments = BTreeMap::new();
    for column in columns {
        let field = schema_at(schema, &column.path)?;
        let SchemaKind::Scalar { ty } = field.kind else {
            return Err(XlsxFormatError::HierarchicalPath {
                path: column.path.join("/"),
                reason: "column path must end at a scalar",
            });
        };
        let cell = cells
            .get_value((row - 1, column.column.get() - 1))
            .unwrap_or(&Data::Empty);
        assignments.insert(
            column.path.clone(),
            Instance::Scalar(read_cell(cell, ty, column.kind, row, &field.name)?),
        );
    }
    Ok(materialize_schema(schema, &mut Vec::new(), &assignments))
}

fn read_cell(
    cell: &Data,
    ty: ScalarType,
    kind: XlsxCellKind,
    row: u32,
    field: &str,
) -> Result<Value, XlsxFormatError> {
    if matches!(cell, Data::Empty) {
        return Ok(Value::Null);
    }
    match kind {
        XlsxCellKind::String => match cell {
            Data::Error(_) => Err(cell_parse_error(cell, ty, row, field)),
            _ => Ok(Value::String(cell.to_string())),
        },
        XlsxCellKind::Number | XlsxCellKind::Boolean => super::parse_cell(cell, ty, row, field),
        XlsxCellKind::Date | XlsxCellKind::DateTime | XlsxCellKind::Time => {
            read_datetime_cell(cell, kind, ty, row, field)
        }
    }
}

fn read_datetime_cell(
    cell: &Data,
    kind: XlsxCellKind,
    ty: ScalarType,
    row: u32,
    field: &str,
) -> Result<Value, XlsxFormatError> {
    let lexical = match cell {
        Data::DateTime(value) => {
            let (year, month, day, hour, minute, second, millis) = value.to_ymd_hms_milli();
            format_datetime_parts(kind, year, month, day, hour, minute, second, millis)
        }
        Data::DateTimeIso(value) | Data::String(value) => normalize_datetime_lexical(kind, value),
        _ => return Err(cell_parse_error(cell, ty, row, field)),
    };
    Ok(Value::String(lexical))
}

#[allow(clippy::too_many_arguments)]
fn format_datetime_parts(
    kind: XlsxCellKind,
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    millis: u16,
) -> String {
    let date = format!("{year:04}-{month:02}-{day:02}");
    let mut time = format!("{hour:02}:{minute:02}:{second:02}");
    if millis != 0 {
        time.push_str(&format!(".{millis:03}"));
    }
    match kind {
        XlsxCellKind::Date => date,
        XlsxCellKind::DateTime => format!("{date}T{time}"),
        XlsxCellKind::Time => time,
        _ => String::new(),
    }
}

fn normalize_datetime_lexical(kind: XlsxCellKind, value: &str) -> String {
    match kind {
        XlsxCellKind::Date => value
            .split_once('T')
            .or_else(|| value.split_once(' '))
            .map_or(value, |(date, _)| date)
            .to_string(),
        XlsxCellKind::DateTime => value.replacen(' ', "T", 1),
        XlsxCellKind::Time => value
            .split_once('T')
            .or_else(|| value.split_once(' '))
            .map_or(value, |(_, time)| time)
            .to_string(),
        _ => value.to_string(),
    }
}

fn cell_parse_error(cell: &Data, ty: ScalarType, row: u32, field: &str) -> XlsxFormatError {
    XlsxFormatError::Parse {
        row,
        field: field.to_string(),
        expected: ty,
        value: cell.to_string(),
    }
}

fn materialize_schema(
    schema: &SchemaNode,
    path: &mut Vec<String>,
    assignments: &BTreeMap<Vec<String>, Instance>,
) -> Instance {
    if let Some(value) = assignments.get(path) {
        return value.clone();
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Instance::Scalar(Value::Null);
    };
    let fields = children
        .iter()
        .map(|child| {
            path.push(child.name.clone());
            let value = if assignments
                .keys()
                .any(|assigned| assigned.starts_with(path))
            {
                materialize_schema(child, path, assignments)
            } else if child.repeating {
                Instance::Repeated(Vec::new())
            } else {
                match child.kind {
                    SchemaKind::Scalar { .. } => Instance::Scalar(Value::Null),
                    SchemaKind::Group { .. } => materialize_schema(child, path, assignments),
                }
            };
            path.pop();
            (child.name.clone(), value)
        })
        .collect();
    Instance::Group(fields)
}

/// Writes a hierarchical workbook and returns its worksheet count.
pub fn write_hierarchical(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
) -> Result<usize, XlsxFormatError> {
    let (bytes, worksheet_count) = to_bytes_hierarchical(schema, instance, layout)?;
    std::fs::write(path, bytes)?;
    Ok(worksheet_count)
}

/// Materializes a hierarchical workbook in memory and returns its worksheet
/// count. Validation completes before any caller-owned file is replaced.
pub fn to_bytes_hierarchical(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
) -> Result<(Vec<u8>, usize), XlsxFormatError> {
    validate_layout(schema, layout)?;
    let worksheets = repeated_at(instance, &layout.worksheets_path)?;
    if worksheets.is_empty() {
        return Err(XlsxFormatError::NoWorksheets);
    }

    let mut workbook = Workbook::new();
    let mut names = BTreeSet::new();
    for worksheet_instance in worksheets {
        let name = scalar_at(worksheet_instance, &layout.worksheet_name_path)?;
        let Value::String(name) = name else {
            return Err(XlsxFormatError::HierarchicalValue {
                path: layout.worksheet_name_path.join("/"),
                reason: "worksheet name must be a non-null string",
            });
        };
        if !names.insert(name.as_str()) {
            return Err(XlsxFormatError::DuplicateWorksheet(name.clone()));
        }
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(name)?;
        write_worksheet(worksheet, worksheet_instance, layout)?;
    }
    let worksheet_count = worksheets.len();
    Ok((workbook.save_to_buffer()?, worksheet_count))
}

fn validate_layout(
    schema: &SchemaNode,
    layout: &XlsxHierarchicalLayout,
) -> Result<(), XlsxFormatError> {
    if schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err(XlsxFormatError::HierarchicalLayout(
            "target schema must be a non-repeating group",
        ));
    }
    let worksheet = schema_at(schema, &layout.worksheets_path)?;
    if !worksheet.repeating || !matches!(worksheet.kind, SchemaKind::Group { .. }) {
        return Err(XlsxFormatError::HierarchicalPath {
            path: layout.worksheets_path.join("/"),
            reason: "worksheet path must end at a repeating group",
        });
    }
    let name = schema_at(worksheet, &layout.worksheet_name_path)?;
    if name.repeating
        || !matches!(
            name.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )
    {
        return Err(XlsxFormatError::HierarchicalPath {
            path: layout.worksheet_name_path.join("/"),
            reason: "worksheet name path must end at a non-repeating string scalar",
        });
    }
    if layout.ranges.is_empty() {
        return Err(XlsxFormatError::HierarchicalLayout(
            "at least one row range is required",
        ));
    }

    let mut range_paths = BTreeSet::new();
    for (index, range) in layout.ranges.iter().enumerate() {
        if index == 0 && matches!(range.start, XlsxRangeStart::AfterPrevious { .. }) {
            return Err(XlsxFormatError::HierarchicalLayout(
                "the first row range must use an absolute start",
            ));
        }
        if !range_paths.insert(&range.path) {
            return Err(XlsxFormatError::HierarchicalPath {
                path: range.path.join("/"),
                reason: "row range path is mapped more than once",
            });
        }
        validate_range(worksheet, range)?;
    }
    Ok(())
}

fn validate_range(worksheet: &SchemaNode, range: &XlsxOutputRange) -> Result<(), XlsxFormatError> {
    let row = schema_at(worksheet, &range.path)?;
    if !matches!(row.kind, SchemaKind::Group { .. }) {
        return Err(XlsxFormatError::HierarchicalPath {
            path: range.path.join("/"),
            reason: "row range path must end at a group",
        });
    }
    if !row.repeating && range.count.map(mapping::XlsxRow::get) != Some(1) {
        return Err(XlsxFormatError::HierarchicalPath {
            path: range.path.join("/"),
            reason: "a non-repeating row range must have a fixed count of one",
        });
    }
    if range.columns.is_empty() {
        return Err(XlsxFormatError::HierarchicalPath {
            path: range.path.join("/"),
            reason: "row range must contain at least one column",
        });
    }

    let mut paths = BTreeSet::new();
    let mut columns = BTreeSet::new();
    for column in &range.columns {
        if !paths.insert(&column.path) || !columns.insert(column.column) {
            return Err(XlsxFormatError::HierarchicalPath {
                path: range.path.join("/"),
                reason: "a field path or physical column is mapped more than once",
            });
        }
        let field = schema_at(row, &column.path)?;
        let SchemaKind::Scalar { ty } = field.kind else {
            return Err(XlsxFormatError::HierarchicalPath {
                path: column.path.join("/"),
                reason: "column path must end at a scalar",
            });
        };
        if field.repeating || !compatible_type(ty, column.kind) {
            return Err(XlsxFormatError::HierarchicalPath {
                path: column.path.join("/"),
                reason: "column scalar type is incompatible with its XLSX cell kind",
            });
        }
    }
    Ok(())
}

fn compatible_type(ty: ScalarType, kind: XlsxCellKind) -> bool {
    match kind {
        XlsxCellKind::String | XlsxCellKind::Date | XlsxCellKind::DateTime | XlsxCellKind::Time => {
            ty == ScalarType::String
        }
        XlsxCellKind::Number => matches!(ty, ScalarType::Int | ScalarType::Float),
        XlsxCellKind::Boolean => ty == ScalarType::Bool,
    }
}

fn write_worksheet(
    worksheet: &mut Worksheet,
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
) -> Result<(), XlsxFormatError> {
    let date_format = Format::new().set_num_format("yyyy-mm-dd");
    let datetime_format = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
    let time_format = Format::new().set_num_format("hh:mm:ss");
    let mut previous_end = 0_u32;

    for range in &layout.ranges {
        let start = match range.start {
            XlsxRangeStart::Absolute { row } => row.get(),
            XlsxRangeStart::AfterPrevious { offset } => previous_end
                .checked_add(offset.get())
                .ok_or(XlsxFormatError::InvalidCoordinate)?,
        };
        if start == 0 || start > MAX_WORKSHEET_ROW {
            return Err(XlsxFormatError::InvalidCoordinate);
        }
        let mut next_row = start;
        if range.has_header {
            write_header(worksheet, next_row, &range.columns)?;
            next_row = next_row
                .checked_add(1)
                .ok_or(XlsxFormatError::InvalidCoordinate)?;
        }

        let rows = rows_at(instance, &range.path)?;
        let limit = range.count.map(mapping::XlsxRow::get).unwrap_or(u32::MAX);
        for row in rows.iter().take(limit as usize) {
            if next_row == 0 || next_row > MAX_WORKSHEET_ROW {
                return Err(XlsxFormatError::InvalidCoordinate);
            }
            write_row(
                worksheet,
                next_row,
                row,
                &range.columns,
                &date_format,
                &datetime_format,
                &time_format,
            )?;
            next_row = next_row
                .checked_add(1)
                .ok_or(XlsxFormatError::InvalidCoordinate)?;
        }

        let actual_end = next_row.saturating_sub(1);
        previous_end = if let Some(count) = range.count {
            start
                .checked_add(u32::from(range.has_header))
                .and_then(|value| value.checked_add(count.get()))
                .and_then(|value| value.checked_sub(1))
                .ok_or(XlsxFormatError::InvalidCoordinate)?
                .max(actual_end)
        } else {
            actual_end
        };
        if previous_end > MAX_WORKSHEET_ROW {
            return Err(XlsxFormatError::InvalidCoordinate);
        }
    }
    Ok(())
}

fn write_header(
    worksheet: &mut Worksheet,
    one_based_row: u32,
    columns: &[XlsxOutputColumn],
) -> Result<(), XlsxFormatError> {
    for column in columns {
        if let Some(header) = &column.header {
            let physical_column = u16::try_from(column.column.get() - 1)
                .map_err(|_| XlsxFormatError::InvalidCoordinate)?;
            worksheet.write_string(one_based_row - 1, physical_column, header)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_row(
    worksheet: &mut Worksheet,
    one_based_row: u32,
    row: &Instance,
    columns: &[XlsxOutputColumn],
    date_format: &Format,
    datetime_format: &Format,
    time_format: &Format,
) -> Result<(), XlsxFormatError> {
    for column in columns {
        let value = scalar_at(row, &column.path)?;
        write_value(
            worksheet,
            one_based_row - 1,
            column.column.get() - 1,
            value,
            column,
            date_format,
            datetime_format,
            time_format,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_value(
    worksheet: &mut Worksheet,
    row: u32,
    column: u32,
    value: &Value,
    layout: &XlsxOutputColumn,
    date_format: &Format,
    datetime_format: &Format,
    time_format: &Format,
) -> Result<(), XlsxFormatError> {
    if matches!(value, Value::Null) {
        return Ok(());
    }
    let column = u16::try_from(column).map_err(|_| XlsxFormatError::InvalidCoordinate)?;
    match (layout.kind, value) {
        (XlsxCellKind::String, Value::String(value)) => {
            worksheet.write_string(row, column, value)?;
        }
        (XlsxCellKind::String, Value::Bool(value)) => {
            worksheet.write_boolean(row, column, *value)?;
        }
        (XlsxCellKind::String, Value::Int(value)) => {
            worksheet.write_string(row, column, value.to_string())?;
        }
        (XlsxCellKind::String, Value::Float(value)) if value.is_finite() => {
            worksheet.write_string(row, column, value.to_string())?;
        }
        (XlsxCellKind::Number, Value::Int(value)) => {
            let number = exact_f64(*value).ok_or_else(|| {
                value_error(layout, "integer exceeds Excel's exact numeric range")
            })?;
            worksheet.write_number(row, column, number)?;
        }
        (XlsxCellKind::Number, Value::Float(value)) if value.is_finite() => {
            worksheet.write_number(row, column, *value)?;
        }
        (XlsxCellKind::Number, Value::String(value)) => {
            let number = value
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .ok_or_else(|| value_error(layout, "value is not a finite number"))?;
            worksheet.write_number(row, column, number)?;
        }
        (XlsxCellKind::Boolean, Value::Bool(value)) => {
            worksheet.write_boolean(row, column, *value)?;
        }
        (XlsxCellKind::Boolean, Value::String(value)) => {
            let boolean = value
                .parse::<bool>()
                .map_err(|_| value_error(layout, "value is not a boolean"))?;
            worksheet.write_boolean(row, column, boolean)?;
        }
        (
            kind @ (XlsxCellKind::Date | XlsxCellKind::DateTime | XlsxCellKind::Time),
            Value::String(value),
        ) => {
            let datetime = ExcelDateTime::parse_from_str(value)
                .map_err(|_| value_error(layout, "value is not an Excel-compatible date/time"))?;
            let format = if kind == XlsxCellKind::Date {
                date_format
            } else if kind == XlsxCellKind::DateTime {
                datetime_format
            } else {
                time_format
            };
            worksheet.write_datetime_with_format(row, column, &datetime, format)?;
        }
        _ => {
            return Err(value_error(
                layout,
                "value type does not match the XLSX cell kind",
            ));
        }
    }
    Ok(())
}

fn value_error(column: &XlsxOutputColumn, reason: &'static str) -> XlsxFormatError {
    XlsxFormatError::HierarchicalValue {
        path: column.path.join("/"),
        reason,
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    (-MAX_EXACT_F64_INTEGER..=MAX_EXACT_F64_INTEGER)
        .contains(&value)
        .then_some(value as f64)
}

fn schema_at<'a>(root: &'a SchemaNode, path: &[String]) -> Result<&'a SchemaNode, XlsxFormatError> {
    let mut node = root;
    for segment in path {
        node = node
            .child(segment)
            .ok_or_else(|| XlsxFormatError::HierarchicalPath {
                path: path.join("/"),
                reason: "field does not exist in the schema",
            })?;
    }
    Ok(node)
}

fn instance_at<'a>(root: &'a Instance, path: &[String]) -> Result<&'a Instance, XlsxFormatError> {
    let mut instance = root;
    for segment in path {
        let Instance::Group(fields) = instance else {
            return Err(XlsxFormatError::HierarchicalValue {
                path: path.join("/"),
                reason: "path crosses a non-group value",
            });
        };
        instance = fields
            .iter()
            .find_map(|(name, value)| (name == segment).then_some(value))
            .ok_or_else(|| XlsxFormatError::HierarchicalValue {
                path: path.join("/"),
                reason: "field is missing from the target instance",
            })?;
    }
    Ok(instance)
}

fn repeated_at<'a>(root: &'a Instance, path: &[String]) -> Result<&'a [Instance], XlsxFormatError> {
    match instance_at(root, path)? {
        Instance::Repeated(items) => Ok(items),
        _ => Err(XlsxFormatError::HierarchicalValue {
            path: path.join("/"),
            reason: "field must be a repeated group",
        }),
    }
}

fn scalar_at<'a>(root: &'a Instance, path: &[String]) -> Result<&'a Value, XlsxFormatError> {
    match instance_at(root, path)? {
        Instance::Scalar(value) => Ok(value),
        _ => Err(XlsxFormatError::HierarchicalValue {
            path: path.join("/"),
            reason: "field must be a scalar",
        }),
    }
}

enum RangeRows<'a> {
    One(&'a Instance),
    Many(&'a [Instance]),
}

impl<'a> RangeRows<'a> {
    fn iter(&self) -> Box<dyn Iterator<Item = &'a Instance> + '_> {
        match self {
            Self::One(row) => Box::new(std::iter::once(*row)),
            Self::Many(rows) => Box::new(rows.iter()),
        }
    }
}

fn rows_at<'a>(root: &'a Instance, path: &[String]) -> Result<RangeRows<'a>, XlsxFormatError> {
    match instance_at(root, path)? {
        row @ Instance::Group(_) => Ok(RangeRows::One(row)),
        Instance::Repeated(rows) if rows.iter().all(|row| matches!(row, Instance::Group(_))) => {
            Ok(RangeRows::Many(rows))
        }
        _ => Err(XlsxFormatError::HierarchicalValue {
            path: path.join("/"),
            reason: "row range must be a group or repeated groups",
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use calamine::{Data, Reader, Xlsx};
    use ir::{Instance, ScalarType, SchemaNode, Value};
    use mapping::{
        XlsxCellKind, XlsxColumn, XlsxHierarchicalLayout, XlsxOutputColumn, XlsxOutputRange,
        XlsxRangeStart, XlsxRow,
    };

    use super::{XlsxFormatError, from_bytes_hierarchical, to_bytes_hierarchical};

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Workbook",
            vec![
                SchemaNode::group(
                    "Sheets",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::group(
                            "Summary",
                            vec![
                                SchemaNode::scalar("Label", ScalarType::String),
                                SchemaNode::scalar("Started", ScalarType::String),
                            ],
                        ),
                        SchemaNode::group(
                            "People",
                            vec![
                                SchemaNode::scalar("Display", ScalarType::String),
                                SchemaNode::scalar("Score", ScalarType::Float),
                            ],
                        )
                        .repeating(),
                        SchemaNode::group(
                            "Footer",
                            vec![SchemaNode::scalar("Label", ScalarType::String)],
                        ),
                    ],
                )
                .repeating(),
            ],
        )
    }

    fn layout() -> XlsxHierarchicalLayout {
        XlsxHierarchicalLayout {
            worksheets_path: vec!["Sheets".into()],
            worksheet_name_path: vec!["Name".into()],
            ranges: vec![
                XlsxOutputRange {
                    path: vec!["Summary".into()],
                    start: XlsxRangeStart::Absolute {
                        row: XlsxRow::new(2).unwrap(),
                    },
                    count: XlsxRow::new(1),
                    has_header: true,
                    columns: vec![
                        column("Label", 1, Some("Project"), XlsxCellKind::String),
                        column("Started", 3, Some("Started"), XlsxCellKind::Date),
                    ],
                },
                XlsxOutputRange {
                    path: vec!["People".into()],
                    start: XlsxRangeStart::AfterPrevious {
                        offset: XlsxRow::new(2).unwrap(),
                    },
                    count: None,
                    has_header: true,
                    columns: vec![
                        column("Display", 2, Some("Person"), XlsxCellKind::String),
                        column("Score", 4, Some("Score"), XlsxCellKind::Number),
                    ],
                },
                XlsxOutputRange {
                    path: vec!["Footer".into()],
                    start: XlsxRangeStart::AfterPrevious {
                        offset: XlsxRow::new(3).unwrap(),
                    },
                    count: XlsxRow::new(1),
                    has_header: false,
                    columns: vec![column("Label", 1, None, XlsxCellKind::String)],
                },
            ],
        }
    }

    fn column(
        field: &str,
        physical: u32,
        header: Option<&str>,
        kind: XlsxCellKind,
    ) -> XlsxOutputColumn {
        XlsxOutputColumn {
            path: vec![field.into()],
            column: XlsxColumn::new(physical).unwrap(),
            header: header.map(str::to_string),
            kind,
        }
    }

    fn workbook_instance(names: &[&str]) -> Instance {
        Instance::Group(vec![(
            "Sheets".into(),
            Instance::Repeated(
                names
                    .iter()
                    .map(|name| {
                        Instance::Group(vec![
                            (
                                "Name".into(),
                                Instance::Scalar(Value::String((*name).into())),
                            ),
                            (
                                "Summary".into(),
                                Instance::Group(vec![
                                    (
                                        "Label".into(),
                                        Instance::Scalar(Value::String("Release".into())),
                                    ),
                                    (
                                        "Started".into(),
                                        Instance::Scalar(Value::String("2024-02-03".into())),
                                    ),
                                ]),
                            ),
                            (
                                "People".into(),
                                Instance::Repeated(vec![person("Ada", 8.5), person("Lin", 9.25)]),
                            ),
                            (
                                "Footer".into(),
                                Instance::Group(vec![(
                                    "Label".into(),
                                    Instance::Scalar(Value::String("End".into())),
                                )]),
                            ),
                        ])
                    })
                    .collect(),
            ),
        )])
    }

    fn person(name: &str, score: f64) -> Instance {
        Instance::Group(vec![
            (
                "Display".into(),
                Instance::Scalar(Value::String(name.into())),
            ),
            ("Score".into(), Instance::Scalar(Value::Float(score))),
        ])
    }

    #[test]
    fn writes_runtime_named_worksheets_and_relative_row_ranges() {
        let (bytes, count) = to_bytes_hierarchical(
            &schema(),
            &workbook_instance(&["North", "South"]),
            &layout(),
        )
        .unwrap();
        let mut workbook = Xlsx::new(Cursor::new(bytes)).unwrap();

        assert_eq!(count, 2);
        assert_eq!(workbook.sheet_names(), ["North", "South"]);
        let north = workbook.worksheet_range("North").unwrap();
        assert_eq!(
            north.get_value((1, 0)).map(ToString::to_string).as_deref(),
            Some("Project")
        );
        assert_eq!(
            north.get_value((2, 0)).map(ToString::to_string).as_deref(),
            Some("Release")
        );
        assert!(matches!(north.get_value((2, 2)), Some(Data::DateTime(_))));
        assert_eq!(
            north.get_value((4, 1)).map(ToString::to_string).as_deref(),
            Some("Person")
        );
        assert_eq!(
            north.get_value((5, 1)).map(ToString::to_string).as_deref(),
            Some("Ada")
        );
        assert_eq!(
            north.get_value((6, 1)).map(ToString::to_string).as_deref(),
            Some("Lin")
        );
        assert_eq!(
            north.get_value((9, 0)).map(ToString::to_string).as_deref(),
            Some("End")
        );
    }

    #[test]
    fn writer_and_reader_roundtrip_runtime_worksheets_and_ranges() {
        let expected = workbook_instance(&["North", "South"]);
        let (bytes, _) = to_bytes_hierarchical(&schema(), &expected, &layout()).unwrap();

        assert_eq!(
            from_bytes_hierarchical(&bytes, &schema(), &layout()).unwrap(),
            expected
        );
    }

    #[test]
    fn rejects_duplicate_runtime_worksheet_names() {
        let error =
            to_bytes_hierarchical(&schema(), &workbook_instance(&["Same", "Same"]), &layout())
                .unwrap_err();

        assert!(matches!(error, XlsxFormatError::DuplicateWorksheet(name) if name == "Same"));
    }
}
