use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use umya_spreadsheet::structs::{Style, Worksheet};

use super::{
    MAX_WORKSHEET_ROW, XlsxFormatError, column_indexes, exact_f64, lexical_f64, lexical_i64,
    row_fields, validate_row,
};

/// Replaces one selected table while preserving the rest of an existing
/// workbook. The workbook is fully serialized before the original file is
/// replaced, so validation or serialization failures leave it untouched.
pub fn update(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<(), XlsxFormatError> {
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
    let data_start = start_row
        .checked_add(u32::from(has_header))
        .ok_or(XlsxFormatError::InvalidCoordinate)?;
    let last_data_row = data_start
        .checked_add(u32::try_from(records.len()).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
        .and_then(|row| row.checked_sub(1))
        .unwrap_or(data_start);
    if !records.is_empty() && last_data_row > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }

    let mut workbook = umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|error| XlsxFormatError::Update(error.to_string()))?;
    let worksheet = match sheet {
        Some(name) => workbook
            .sheet_by_name_mut(name)
            .map_err(|_| XlsxFormatError::MissingWorksheet(name.to_string()))?,
        None => workbook
            .sheet_mut(0)
            .map_err(|_| XlsxFormatError::NoWorksheets)?,
    };
    replace_table(
        worksheet, &fields, &columns, &records, start_row, data_start, has_header,
    )?;

    let mut bytes = Vec::new();
    umya_spreadsheet::writer::xlsx::write_writer(&workbook, &mut bytes)
        .map_err(|error| XlsxFormatError::Update(error.to_string()))?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn replace_table(
    worksheet: &mut Worksheet,
    fields: &[(&str, ScalarType)],
    columns: &[u32],
    records: &[Vec<&Value>],
    header_row: u32,
    data_start: u32,
    has_header: bool,
) -> Result<(), XlsxFormatError> {
    let columns = columns.iter().map(|column| column + 1).collect::<Vec<_>>();
    let data_styles = columns
        .iter()
        .map(|column| {
            worksheet
                .cell((*column, data_start))
                .map(|cell| cell.style().clone())
        })
        .collect::<Vec<_>>();
    let highest_row = worksheet.highest_row();
    for row in data_start..=highest_row {
        for column in &columns {
            clear_cell(worksheet, *column, row);
        }
    }
    if has_header {
        for ((name, _), column) in fields.iter().zip(&columns) {
            replace_value(
                worksheet,
                *column,
                header_row,
                None,
                ScalarType::String,
                &Value::String((*name).to_string()),
                0,
                name,
            )?;
        }
    }
    for (offset, record) in records.iter().enumerate() {
        let row = data_start
            .checked_add(u32::try_from(offset).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        for ((((name, ty), value), column), style) in
            fields.iter().zip(record).zip(&columns).zip(&data_styles)
        {
            replace_value(
                worksheet,
                *column,
                row,
                style.as_ref(),
                *ty,
                value,
                offset,
                name,
            )?;
        }
    }
    Ok(())
}

fn clear_cell(worksheet: &mut Worksheet, column: u32, row: u32) {
    let style = worksheet
        .cell((column, row))
        .map(|cell| cell.style().clone());
    worksheet.remove_cell((column, row));
    if let Some(style) = style {
        worksheet.cell_mut((column, row)).set_style(style);
    }
}

#[allow(clippy::too_many_arguments)]
fn replace_value(
    worksheet: &mut Worksheet,
    column: u32,
    row: u32,
    fallback_style: Option<&Style>,
    ty: ScalarType,
    value: &Value,
    row_index: usize,
    field: &str,
) -> Result<(), XlsxFormatError> {
    let existing_style = worksheet
        .cell((column, row))
        .map(|cell| cell.style().clone())
        .or_else(|| fallback_style.cloned());
    worksheet.remove_cell((column, row));
    if *value == Value::Null {
        if let Some(style) = existing_style {
            worksheet.cell_mut((column, row)).set_style(style);
        }
        return Ok(());
    }
    let cell = worksheet.cell_mut((column, row));
    if let Some(style) = existing_style {
        cell.set_style(style);
    }
    let bad = |got| XlsxFormatError::ValueType {
        row: row_index,
        field: field.to_string(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (ScalarType::String, Value::String(value)) => {
            cell.set_value_string(value);
        }
        (ScalarType::String, Value::Bool(value)) => {
            cell.set_value_bool(*value);
        }
        (ScalarType::String, Value::Int(value)) => {
            cell.set_value_string(value.to_string());
        }
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            cell.set_value_string(value.to_string());
        }
        (ScalarType::Int | ScalarType::Float, Value::Int(value)) => {
            cell.set_value_number(
                exact_f64(*value).ok_or_else(|| bad("int outside the exact f64 range"))?,
            );
        }
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => {
            cell.set_value_number(*value);
        }
        (ScalarType::Int, Value::String(value)) => {
            cell.set_value_number(
                lexical_i64(value)
                    .and_then(exact_f64)
                    .ok_or_else(|| bad("string"))?,
            );
        }
        (ScalarType::Float, Value::String(value)) => {
            cell.set_value_number(lexical_f64(value).ok_or_else(|| bad("string"))?);
        }
        (ScalarType::Bool, Value::Bool(value)) => {
            cell.set_value_bool(*value);
        }
        (ScalarType::Bool, Value::String(value)) => {
            cell.set_value_bool(value.trim().parse().map_err(|_| bad("string"))?);
        }
        (_, Value::Float(_)) => return Err(bad("non-finite float")),
        (_, other) => return Err(bad(other.type_name())),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use calamine::{Data, Reader};
    use rust_xlsxwriter::Workbook;

    use super::*;

    fn row(month: &str, west: f64) -> Instance {
        Instance::Group(vec![
            (
                "Month".into(),
                Instance::Scalar(Value::String(month.into())),
            ),
            ("West".into(), Instance::Scalar(Value::Float(west))),
        ])
    }

    #[test]
    fn replaces_only_the_selected_table_and_removes_stale_rows() {
        let path =
            std::env::temp_dir().join(format!("ferrule_xlsx_update_{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut workbook = Workbook::new();
        let sales = workbook.add_worksheet();
        sales.set_name("Sales").unwrap();
        sales.write_string(0, 0, "Report title").unwrap();
        sales.write_string(4, 0, "Old month").unwrap();
        sales.write_string(4, 1, "Old west").unwrap();
        sales.write_string(5, 0, "Old row").unwrap();
        sales.write_number(5, 1, 1.0).unwrap();
        sales.write_string(6, 0, "Stale row").unwrap();
        sales.write_number(6, 1, 2.0).unwrap();
        sales.write_string(5, 3, "Preserve beside table").unwrap();
        let keep = workbook.add_worksheet();
        keep.set_name("Keep").unwrap();
        keep.write_string(0, 0, "Preserve other sheet").unwrap();
        workbook.save(&path).unwrap();
        let schema = SchemaNode::group(
            "Sales",
            vec![
                SchemaNode::scalar("Month", ScalarType::String),
                SchemaNode::scalar("West", ScalarType::Float),
            ],
        );

        update(
            &path,
            &schema,
            &[row("January", 4.5)],
            Some("Sales"),
            5,
            &[1, 2],
            true,
        )
        .unwrap();

        let mut result: calamine::Xlsx<_> = calamine::open_workbook(&path).unwrap();
        let sales = result.worksheet_range("Sales").unwrap();
        let keep = result.worksheet_range("Keep").unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            sales.get_value((0, 0)),
            Some(&Data::String("Report title".into()))
        );
        assert_eq!(sales.get_value((4, 0)), Some(&Data::String("Month".into())));
        assert_eq!(
            sales.get_value((5, 0)),
            Some(&Data::String("January".into()))
        );
        assert_eq!(sales.get_value((5, 1)), Some(&Data::Float(4.5)));
        assert_eq!(sales.get_value((6, 0)), None);
        assert_eq!(
            sales.get_value((5, 3)),
            Some(&Data::String("Preserve beside table".into()))
        );
        assert_eq!(
            keep.get_value((0, 0)),
            Some(&Data::String("Preserve other sheet".into()))
        );
    }
}
