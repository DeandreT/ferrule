use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, TabularBoundaryKind, XlsxColumn, XlsxRow, XlsxWorksheetSetLayout};

use super::{SchemaComponent, Table, TableLayout, unique_field_name};
use crate::import::schema::ComponentFormat;

#[allow(clippy::too_many_arguments)]
pub(super) fn read(
    name: &str,
    excel: roxmltree::Node<'_, '_>,
    tables: &[Table],
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
    is_source: bool,
    is_default_output: bool,
    _warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let [table] = tables else {
        return None;
    };
    if !is_source
        || table.sheet.is_some()
        || table.worksheet_ports.is_empty() && table.worksheet_name_ports.is_empty()
    {
        return None;
    }
    let TableLayout::Flat {
        start_row,
        has_header,
        row_ports,
        row_number_ports,
        columns,
    } = &table.layout
    else {
        return None;
    };

    const WORKSHEETS: &str = "Worksheet";
    const WORKSHEET_NAME: &str = "Name";
    const ROWS: &str = "Row";

    let worksheet_path = vec![WORKSHEETS.to_string()];
    let rows_path = vec![WORKSHEETS.to_string(), ROWS.to_string()];
    let mut ports = BTreeMap::new();
    for key in &table.worksheet_ports {
        ports.insert(*key, worksheet_path.clone());
    }
    let mut name_path = worksheet_path.clone();
    name_path.push(WORKSHEET_NAME.to_string());
    for key in &table.worksheet_name_ports {
        ports.insert(*key, name_path.clone());
    }
    for key in row_ports {
        ports.insert(*key, rows_path.clone());
    }

    let mut row_fields = Vec::with_capacity(columns.len() + 1);
    let mut physical_columns = Vec::with_capacity(columns.len());
    for column in columns {
        let mut path = rows_path.clone();
        path.push(column.name.clone());
        for key in &column.ports {
            ports.insert(*key, path.clone());
        }
        row_fields.push(SchemaNode::scalar(&column.name, column.ty));
        physical_columns.push(XlsxColumn::new(column.index)?);
    }
    let row_number_path = if row_number_ports.is_empty() {
        None
    } else {
        let field_name = unique_field_name("r", &row_fields);
        let mut path = rows_path.clone();
        path.push(field_name.clone());
        for key in row_number_ports {
            ports.insert(*key, path.clone());
        }
        row_fields.push(SchemaNode::scalar(&field_name, ScalarType::Int));
        Some(vec![field_name])
    };

    Some(SchemaComponent {
        name: name.to_string(),
        format: ComponentFormat::Xlsx,
        schema: SchemaNode::group(
            name,
            vec![
                SchemaNode::group(
                    WORKSHEETS,
                    vec![
                        SchemaNode::scalar(WORKSHEET_NAME, ScalarType::String),
                        SchemaNode::group(ROWS, row_fields).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        input_instance: excel.attribute("inputinstance").map(str::to_string),
        output_instance: excel.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            xlsx_worksheet_set: Some(XlsxWorksheetSetLayout {
                worksheets_path: worksheet_path,
                worksheet_name_path: vec![WORKSHEET_NAME.to_string()],
                rows_path: vec![ROWS.to_string()],
                row_number_path,
                start_row: XlsxRow::new(start_row.unwrap_or(1))?,
                columns: physical_columns,
                has_header: *has_header,
            }),
            ..FormatOptions::default()
        },
        is_source,
        is_default_output,
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}
