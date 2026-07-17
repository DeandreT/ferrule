use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::{
    FormatOptions, XlsxColumn, XlsxCompositeLayout, XlsxFixedCell, XlsxFixedRecord, XlsxRow,
    XlsxTableRegion,
};

use super::{ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32};

mod grid;
mod hierarchical;

const MAX_WORKSHEET_ROW: u32 = 1_048_576;
const MAX_WORKSHEET_COLUMN: u32 = 16_384;

#[derive(Clone)]
struct Column {
    name: String,
    index: u32,
    ty: ScalarType,
    ports: Vec<u32>,
}

#[derive(Clone)]
struct Table {
    sheet: Option<String>,
    layout: TableLayout,
}

#[derive(Clone)]
enum TableLayout {
    Flat {
        start_row: Option<u32>,
        has_header: bool,
        row_ports: Vec<u32>,
        columns: Vec<Column>,
    },
    Transposed {
        rows: Vec<TransposedRow>,
        index_ports: Vec<u32>,
    },
}

#[derive(Clone)]
struct TransposedRow {
    name: String,
    row: u32,
    ty: ScalarType,
    ports: Vec<u32>,
}

struct FixedRecord {
    name: String,
    sheet: Option<String>,
    ports: Vec<u32>,
    cells: Vec<FixedCell>,
}

struct FixedCell {
    name: String,
    row: XlsxRow,
    column: XlsxColumn,
    ty: ScalarType,
    ports: Vec<u32>,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))?;
    let root = data.children().find(|node| node.has_tag_name("root"))?;
    let excel = data.children().find(|node| node.has_tag_name("excel"))?;
    let workbook = root
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Workbook"))?;
    let (input_keys, output_keys) = entry_key_sets(&root);
    let is_source = output_keys.len() >= input_keys.len();

    if let Some(hierarchical) = hierarchical::read(
        *component,
        excel,
        workbook,
        &name,
        input_keys.clone(),
        output_keys.clone(),
        is_source,
        warnings,
    ) {
        return Some(hierarchical);
    }

    if let Some(grid) = grid::read(
        *component,
        excel,
        workbook,
        &name,
        input_keys.clone(),
        output_keys.clone(),
        is_source,
        warnings,
    ) {
        return Some(grid);
    }

    let mut tables = Vec::new();
    let mut records = Vec::new();
    for worksheet in workbook
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Worksheet"))
    {
        inspect_worksheet(
            worksheet,
            &name,
            is_source,
            warnings,
            &mut tables,
            &mut records,
        );
    }

    if !records.is_empty()
        && let Some(composite) = read_composite(
            name.clone(),
            excel,
            tables.clone(),
            records,
            input_keys.clone(),
            output_keys.clone(),
            is_source,
            is_default_output(component),
            warnings,
        )
    {
        return Some(composite);
    }
    let table = match tables.len() {
        0 => {
            warnings.push(format!(
                "xlsx component `{name}` has no supported table: select one worksheet with either one open row range and fixed-index columns or multiple fixed rows with open Cell sequences"
            ));
            return None;
        }
        1 => tables.pop()?,
        count => {
            warnings.push(format!(
                "xlsx component `{name}` contains {count} supported tables; ferrule currently imports one worksheet/table per component"
            ));
            return None;
        }
    };

    let mut ports = BTreeMap::new();
    let (fields, options) = match table.layout {
        TableLayout::Flat {
            start_row,
            has_header,
            row_ports,
            columns,
        } => {
            for key in row_ports {
                ports.insert(key, Vec::new());
            }
            let mut fields = Vec::with_capacity(columns.len());
            let mut xlsx_columns = Vec::with_capacity(columns.len());
            for column in columns {
                for key in column.ports {
                    ports.insert(key, vec![column.name.clone()]);
                }
                fields.push(SchemaNode::scalar(&column.name, column.ty));
                xlsx_columns.push(column.index);
            }
            (
                fields,
                FormatOptions {
                    has_header_row: Some(has_header),
                    xlsx_sheet: table.sheet,
                    xlsx_start_row: start_row,
                    xlsx_columns,
                    xlsx_update_existing: !is_source
                        && excel.attribute("updateexistingfile") == Some("1"),
                    ..FormatOptions::default()
                },
            )
        }
        TableLayout::Transposed { rows, index_ports } => {
            if !is_source && excel.attribute("updateexistingfile") == Some("1") {
                warnings.push(format!(
                    "xlsx component `{name}` updates a transposed table in an existing workbook; that target layout is unsupported"
                ));
            }
            let mut fields = Vec::with_capacity(rows.len() + usize::from(!index_ports.is_empty()));
            let mut xlsx_rows = Vec::with_capacity(rows.len());
            for row in rows {
                for key in row.ports {
                    ports.insert(key, vec![row.name.clone()]);
                }
                fields.push(SchemaNode::scalar(&row.name, row.ty));
                xlsx_rows.push(row.row);
            }
            if !index_ports.is_empty() {
                for key in index_ports {
                    ports.insert(key, vec!["n".to_string()]);
                }
                fields.push(SchemaNode::scalar("n", ScalarType::Int));
            }
            (
                fields,
                FormatOptions {
                    has_header_row: Some(false),
                    xlsx_sheet: table.sheet,
                    xlsx_rows,
                    ..FormatOptions::default()
                },
            )
        }
    };

    Some(SchemaComponent {
        name: name.clone(),
        format: ComponentFormat::Xlsx,
        schema: SchemaNode::group(&name, fields),
        input_instance: excel.attribute("inputinstance").map(str::to_string),
        output_instance: excel.attribute("outputinstance").map(str::to_string),
        options,
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn read_composite(
    name: String,
    excel: roxmltree::Node<'_, '_>,
    mut tables: Vec<Table>,
    records: Vec<FixedRecord>,
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
    is_source: bool,
    is_default_output: bool,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let table = match tables.len() {
        1 => tables.pop()?,
        0 => {
            warnings.push(format!(
                "xlsx component `{name}` has fixed records but no supported open row table; composite XLSX sources require exactly one table"
            ));
            return None;
        }
        count => {
            warnings.push(format!(
                "xlsx component `{name}` has fixed records and {count} tables; composite XLSX sources require exactly one table"
            ));
            return None;
        }
    };
    let TableLayout::Flat {
        start_row,
        has_header,
        row_ports,
        columns,
    } = table.layout
    else {
        warnings.push(format!(
            "xlsx component `{name}` combines fixed records with a transposed table; that composite layout is unsupported"
        ));
        return None;
    };
    let Some(table_name) = table.sheet.clone().filter(|sheet| !sheet.is_empty()) else {
        warnings.push(format!(
            "xlsx component `{name}` combines fixed records with a default worksheet table; a static table worksheet name is required"
        ));
        return None;
    };
    let start_row = XlsxRow::new(start_row.unwrap_or(1))?;
    let mut names = BTreeSet::new();
    if !names.insert(table_name.as_str())
        || records
            .iter()
            .any(|record| !names.insert(record.name.as_str()))
    {
        warnings.push(format!(
            "xlsx component `{name}` uses the same worksheet name for more than one composite region; component skipped"
        ));
        return None;
    }

    let mut ports = BTreeMap::new();
    let mut fields = Vec::with_capacity(records.len() + 1);
    let mut fixed_layouts = Vec::with_capacity(records.len());
    for record in records {
        let record_path = vec![record.name.clone()];
        for key in record.ports {
            ports.insert(key, record_path.clone());
        }
        let mut record_fields = Vec::with_capacity(record.cells.len());
        let mut cells = Vec::with_capacity(record.cells.len());
        for cell in record.cells {
            let mut field_path = record_path.clone();
            field_path.push(cell.name.clone());
            for key in cell.ports {
                ports.insert(key, field_path.clone());
            }
            record_fields.push(SchemaNode::scalar(&cell.name, cell.ty));
            cells.push(XlsxFixedCell {
                path: vec![cell.name],
                row: cell.row,
                column: cell.column,
            });
        }
        fields.push(SchemaNode::group(&record.name, record_fields));
        fixed_layouts.push(XlsxFixedRecord {
            path: record_path,
            sheet: record.sheet,
            cells,
        });
    }

    let table_path = vec![table_name.clone()];
    for key in row_ports {
        ports.insert(key, table_path.clone());
    }
    let mut table_fields = Vec::with_capacity(columns.len());
    let mut table_columns = Vec::with_capacity(columns.len());
    for column in columns {
        let index = XlsxColumn::new(column.index)?;
        let mut field_path = table_path.clone();
        field_path.push(column.name.clone());
        for key in column.ports {
            ports.insert(key, field_path.clone());
        }
        table_fields.push(SchemaNode::scalar(&column.name, column.ty));
        table_columns.push(index);
    }
    fields.push(SchemaNode::group(&table_name, table_fields).repeating());

    if excel.attribute("updateexistingfile") == Some("1") {
        warnings.push(format!(
            "xlsx component `{name}` updates an existing workbook; ferrule writes a new workbook, so content outside the selected table will not be preserved"
        ));
    }
    Some(SchemaComponent {
        name: name.clone(),
        format: ComponentFormat::Xlsx,
        schema: SchemaNode::group(&name, fields),
        input_instance: excel.attribute("inputinstance").map(str::to_string),
        output_instance: excel.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            xlsx_composite: Some(XlsxCompositeLayout {
                table: XlsxTableRegion {
                    path: table_path,
                    sheet: table.sheet,
                    start_row,
                    columns: table_columns,
                    has_header,
                },
                records: fixed_layouts,
            }),
            ..FormatOptions::default()
        },
        is_source,
        is_default_output,
        is_variable: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn inspect_worksheet(
    worksheet: roxmltree::Node<'_, '_>,
    component_name: &str,
    is_source: bool,
    warnings: &mut Vec<String>,
    tables: &mut Vec<Table>,
    records: &mut Vec<FixedRecord>,
) {
    let range_by_id: BTreeMap<&str, roxmltree::Node<'_, '_>> = worksheet
        .children()
        .find(|node| node.has_tag_name("ranges"))
        .into_iter()
        .flat_map(|ranges| ranges.children().filter(|node| node.has_tag_name("range")))
        .filter_map(|range| range.attribute("id").map(|id| (id, range)))
        .collect();

    let mut transposed_rows = Vec::new();
    let mut transposed_index_ports = Vec::new();
    let mut fixed_cells = Vec::new();
    for row in worksheet
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Row"))
    {
        let Some(range_id) = selected_range_id(row) else {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` has a connected row without a fixed range selector; that row was skipped"
                ));
            }
            continue;
        };
        let Some(range) = range_by_id.get(range_id.as_str()).copied() else {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` references missing range `{range_id}`; connected ports under that row were skipped"
                ));
            }
            continue;
        };
        if range.attribute("count") == Some("1")
            && range.attribute("offset").is_none()
            && is_source
            && let Some(transposed) = read_transposed_row(
                row,
                range,
                &range_id,
                component_name,
                warnings,
                &mut transposed_index_ports,
            )
        {
            transposed_rows.push(transposed);
            continue;
        }
        if range.attribute("count") == Some("1") && range.attribute("offset").is_none() && is_source
        {
            let cells = read_fixed_cells(row, range, component_name, warnings);
            if !cells.is_empty() {
                fixed_cells.extend(cells);
                continue;
            }
        }
        if range.attribute("count").is_some() {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` has connected fixed range `{range_id}` without a supported open Cell sequence; those ports were skipped"
                ));
            }
            continue;
        }
        if range.attribute("offset").is_some() {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` positions range `{range_id}` relative to another range; the table was skipped because relative placement is unsupported"
                ));
            }
            continue;
        }

        let sheet = match worksheet_name(worksheet) {
            Ok(sheet) => sheet,
            Err(()) => {
                if subtree_has_ports(row) {
                    warnings.push(format!(
                        "xlsx component `{component_name}` selects a worksheet dynamically; the connected table was skipped"
                    ));
                }
                continue;
            }
        };
        warn_dynamic_sheet_ports(worksheet, component_name, warnings);
        warn_physical_index_ports(row, component_name, warnings);

        let start_row = match range.attribute("start") {
            Some(value) => match value.parse::<u32>() {
                Ok(0) | Err(_) => {
                    warnings.push(format!(
                        "xlsx component `{component_name}` range `{range_id}` has invalid one-based start row `{value}`; table skipped"
                    ));
                    continue;
                }
                Ok(value) if value <= MAX_WORKSHEET_ROW => Some(value),
                Ok(_) => {
                    warnings.push(format!(
                        "xlsx component `{component_name}` range `{range_id}` starts beyond Excel's row limit; table skipped"
                    ));
                    continue;
                }
            },
            None => None,
        };
        let columns = read_columns(row, component_name, warnings);
        if columns.is_empty() {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` range `{range_id}` has no supported annotation-named, fixed-index columns; table skipped"
                ));
            }
            continue;
        }
        if duplicate_column(&columns) {
            warnings.push(format!(
                "xlsx component `{component_name}` range `{range_id}` maps a column index or field name more than once; table skipped"
            ));
            continue;
        }
        let row_ports = port_keys(row);
        tables.push(Table {
            sheet,
            layout: TableLayout::Flat {
                start_row,
                has_header: row.attribute("enabletitlerow") == Some("1"),
                row_ports,
                columns,
            },
        });
    }

    if !transposed_rows.is_empty() {
        if duplicate_transposed_row(&transposed_rows) {
            warnings.push(format!(
                "xlsx component `{component_name}` maps a transposed physical row or field name more than once; table skipped"
            ));
        } else {
            let sheet = match worksheet_name(worksheet) {
                Ok(sheet) => sheet,
                Err(()) => {
                    warnings.push(format!(
                        "xlsx component `{component_name}` selects a worksheet dynamically; the connected transposed table was skipped"
                    ));
                    return;
                }
            };
            warn_dynamic_sheet_ports(worksheet, component_name, warnings);
            tables.push(Table {
                sheet,
                layout: TableLayout::Transposed {
                    rows: transposed_rows,
                    index_ports: transposed_index_ports,
                },
            });
        }
    }

    if fixed_cells.is_empty() {
        return;
    }
    let sheet = match worksheet_name(worksheet) {
        Ok(Some(sheet)) => sheet,
        Ok(None) => {
            warnings.push(format!(
                "xlsx component `{component_name}` has connected fixed cells on the default worksheet; a static worksheet name is required for a composite record"
            ));
            return;
        }
        Err(()) => {
            warnings.push(format!(
                "xlsx component `{component_name}` selects a worksheet dynamically; the connected fixed cells were skipped"
            ));
            return;
        }
    };
    if duplicate_fixed_cell(&fixed_cells) {
        warnings.push(format!(
            "xlsx component `{component_name}` maps a fixed-cell coordinate or field name more than once on worksheet `{sheet}`; record skipped"
        ));
        return;
    }
    warn_dynamic_sheet_name_port(worksheet, component_name, warnings);
    records.push(FixedRecord {
        name: sheet.clone(),
        sheet: Some(sheet),
        ports: port_keys(worksheet),
        cells: fixed_cells,
    });
}

fn read_fixed_cells(
    row: roxmltree::Node<'_, '_>,
    range: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Vec<FixedCell> {
    let Some(physical_row) = range
        .attribute("start")
        .and_then(|value| value.parse::<u32>().ok())
        .and_then(XlsxRow::new)
    else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    for cell in row.children().filter(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("Cell")
            && !port_keys(*node).is_empty()
    }) {
        let Some(column) = selected_column(cell).and_then(XlsxColumn::new) else {
            continue;
        };
        let Some(name) = cell.attribute("annotation").filter(|name| !name.is_empty()) else {
            warnings.push(format!(
                "xlsx component `{component_name}` fixed cell at row {} column {} has no annotation name; that cell was skipped",
                physical_row.get(),
                column.get()
            ));
            continue;
        };
        if cell.children().any(|node| {
            node.has_tag_name("entry")
                && matches!(node.attribute("name"), Some("n" | "r"))
                && !port_keys(node).is_empty()
        }) {
            warnings.push(format!(
                "xlsx component `{component_name}` exposes physical indexes below fixed cell `{name}`; those index ports were skipped"
            ));
        }
        cells.push(FixedCell {
            name: name.to_string(),
            row: physical_row,
            column,
            ty: scalar_type(cell.attribute("datatype")),
            ports: port_keys(cell),
        });
    }
    cells
}

fn read_transposed_row(
    row: roxmltree::Node<'_, '_>,
    range: roxmltree::Node<'_, '_>,
    range_id: &str,
    component_name: &str,
    warnings: &mut Vec<String>,
    index_ports: &mut Vec<u32>,
) -> Option<TransposedRow> {
    let physical_row = match range.attribute("start")?.parse::<u32>() {
        Ok(value) if (1..=MAX_WORKSHEET_ROW).contains(&value) => value,
        _ => {
            warnings.push(format!(
                "xlsx component `{component_name}` fixed range `{range_id}` has an invalid one-based row; that range was skipped"
            ));
            return None;
        }
    };
    let connected_cells: Vec<_> = row
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && node.attribute("name") == Some("Cell")
                && !port_keys(*node).is_empty()
        })
        .collect();
    if connected_cells.len() != 1
        || selected_column(connected_cells[0]).is_some()
        || connected_cells[0]
            .children()
            .any(|node| node.has_tag_name("condition"))
    {
        return None;
    }
    let cell = connected_cells[0];
    let physical_row_port = row.children().any(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("r")
            && !port_keys(node).is_empty()
    });
    if physical_row_port {
        warnings.push(format!(
            "xlsx component `{component_name}` exposes physical Row/@r ports; those index values were skipped"
        ));
    }
    index_ports.extend(
        cell.children()
            .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("n"))
            .flat_map(port_keys),
    );
    let name = cell
        .attribute("annotation")
        .or_else(|| row.attribute("annotation"))
        .or_else(|| range.attribute("annotation"))
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("Range{range_id}"), str::to_string);
    Some(TransposedRow {
        name,
        row: physical_row,
        ty: scalar_type(cell.attribute("datatype")),
        ports: port_keys(cell),
    })
}

fn read_columns(
    row: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Vec<Column> {
    let mut columns = Vec::new();
    for cell in row
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Cell"))
    {
        let ports = port_keys(cell);
        if ports.is_empty() {
            continue;
        }
        let Some(index) = selected_column(cell) else {
            warnings.push(format!(
                "xlsx component `{component_name}` has a connected Cell without a fixed one-based column selector; that cell was skipped"
            ));
            continue;
        };
        let Some(name) = cell.attribute("annotation").filter(|name| !name.is_empty()) else {
            warnings.push(format!(
                "xlsx component `{component_name}` column {index} has no annotation name; that cell was skipped"
            ));
            continue;
        };
        columns.push(Column {
            name: name.to_string(),
            index,
            ty: scalar_type(cell.attribute("datatype")),
            ports,
        });
    }
    columns
}

fn worksheet_name(worksheet: roxmltree::Node<'_, '_>) -> Result<Option<String>, ()> {
    let Some(condition) = worksheet
        .children()
        .find(|node| node.has_tag_name("condition"))
    else {
        return Ok(None);
    };
    let selector = condition.descendants().find(|node| {
        node.has_tag_name("function")
            && node.attribute("name") == Some("equal-ignorecase")
            && node.attribute("library") == Some("xlsx")
    });
    let Some(selector) = selector else {
        return Err(());
    };
    if !selector
        .descendants()
        .any(|node| node.has_tag_name("attribute") && node.attribute("name") == Some("Name"))
    {
        return Err(());
    }
    selector
        .descendants()
        .find(|node| node.has_tag_name("constant"))
        .and_then(|constant| constant.attribute("value"))
        .filter(|name| !name.is_empty())
        .map(|name| Some(name.to_string()))
        .ok_or(())
}

pub(super) fn selected_range_id(row: roxmltree::Node<'_, '_>) -> Option<String> {
    let function = row.descendants().find(|node| {
        node.has_tag_name("function") && node.attribute("name") == Some("is-range-id")
    })?;
    function
        .descendants()
        .find(|node| node.has_tag_name("constant"))?
        .attribute("value")
        .map(str::to_string)
}

pub(super) fn selected_column(cell: roxmltree::Node<'_, '_>) -> Option<u32> {
    cell.descendants()
        .filter(|node| node.has_tag_name("function") && node.attribute("name") == Some("equal"))
        .find_map(|equal| {
            equal
                .descendants()
                .any(|node| node.has_tag_name("attribute") && node.attribute("name") == Some("n"))
                .then(|| {
                    equal
                        .descendants()
                        .find(|node| node.has_tag_name("constant"))
                        .and_then(|constant| parse_u32(constant.attribute("value")))
                        .filter(|index| (1..=MAX_WORKSHEET_COLUMN).contains(index))
                })
                .flatten()
        })
}

pub(super) fn scalar_type(datatype: Option<&str>) -> ScalarType {
    match datatype {
        Some("double" | "decimal" | "number" | "float") => ScalarType::Float,
        Some("integer" | "int" | "long") => ScalarType::Int,
        Some("boolean" | "bool") => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

fn warn_dynamic_sheet_ports(
    worksheet: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) {
    let has_worksheet_port = !port_keys(worksheet).is_empty();
    let has_name_port = worksheet.children().any(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("Name")
            && !port_keys(node).is_empty()
    });
    if has_worksheet_port || has_name_port {
        warnings.push(format!(
            "xlsx component `{component_name}` has connected dynamic worksheet/name ports; the configured static worksheet is used and those ports were skipped"
        ));
    }
}

fn warn_dynamic_sheet_name_port(
    worksheet: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) {
    let has_name_port = worksheet.children().any(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("Name")
            && !port_keys(node).is_empty()
    });
    if has_name_port {
        warnings.push(format!(
            "xlsx component `{component_name}` has a connected dynamic worksheet-name port; the configured static worksheet is used and that port was skipped"
        ));
    }
}

fn warn_physical_index_ports(
    row: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) {
    let row_index = row.children().any(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("r")
            && !port_keys(node).is_empty()
    });
    let cell_index = row.children().any(|cell| {
        cell.has_tag_name("entry")
            && cell.attribute("name") == Some("Cell")
            && cell.children().any(|node| {
                node.has_tag_name("entry")
                    && node.attribute("name") == Some("n")
                    && !port_keys(node).is_empty()
            })
    });
    if row_index {
        warnings.push(format!(
            "xlsx component `{component_name}` exposes physical Row/@r ports; those index values were skipped"
        ));
    }
    if cell_index {
        warnings.push(format!(
            "xlsx component `{component_name}` exposes physical Cell/@n ports; those index values were skipped"
        ));
    }
}

pub(super) fn port_keys(node: roxmltree::Node<'_, '_>) -> Vec<u32> {
    [node.attribute("outkey"), node.attribute("inpkey")]
        .into_iter()
        .filter_map(parse_u32)
        .collect()
}

fn subtree_has_ports(node: roxmltree::Node<'_, '_>) -> bool {
    node.descendants().any(|entry| !port_keys(entry).is_empty())
}

fn duplicate_column(columns: &[Column]) -> bool {
    let mut names = BTreeSet::new();
    let mut indexes = BTreeSet::new();
    columns
        .iter()
        .any(|column| !names.insert(&column.name) || !indexes.insert(column.index))
}

fn duplicate_transposed_row(rows: &[TransposedRow]) -> bool {
    let mut names = BTreeSet::from(["n"]);
    let mut indexes = BTreeSet::new();
    rows.iter()
        .any(|row| !names.insert(row.name.as_str()) || !indexes.insert(row.row))
}

fn duplicate_fixed_cell(cells: &[FixedCell]) -> bool {
    let mut names = BTreeSet::new();
    let mut coordinates = BTreeSet::new();
    cells.iter().any(|cell| {
        !names.insert(cell.name.as_str())
            || !coordinates.insert((cell.row.get(), cell.column.get()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_grid(
        header_start: u32,
        data_start: u32,
        header_row_key: Option<u32>,
        value_ports: bool,
    ) -> String {
        let header_row_key = header_row_key
            .map(|key| format!(r#" outkey="{key}""#))
            .unwrap_or_default();
        let header_value_key = if value_ports { r#" outkey="1""# } else { "" };
        let data_value_key = if value_ports { r#" outkey="4""# } else { "" };
        format!(
            r#"
            <component name="Grid">
              <data>
                <root><entry name="Workbook"><entry name="Worksheet">
                  <ranges>
                    <range id="1" start="{header_start}" count="1"/>
                    <range id="2" start="{data_start}"/>
                  </ranges>
                  <entry name="Row"{header_row_key}>
                    <condition><function name="is-range-id"><constant value="1"/></function></condition>
                    <entry name="Cell"{header_value_key}><entry name="n" outkey="2"/></entry>
                  </entry>
                  <entry name="Row" outkey="3">
                    <condition><function name="is-range-id"><constant value="2"/></function></condition>
                    <entry name="Cell"{data_value_key}><entry name="n" outkey="5"/></entry>
                  </entry>
                </entry></entry></root>
                <excel inputinstance="grid.xlsx"/>
              </data>
            </component>
            "#
        )
    }

    fn read_minimal_grid(xml: &str) -> (SchemaComponent, Vec<String>) {
        let document = roxmltree::Document::parse(xml).unwrap();
        let mut warnings = Vec::new();
        let component = read(&document.root_element(), &mut warnings).unwrap();
        (component, warnings)
    }

    #[test]
    fn unsupported_composite_shape_falls_back_to_its_supported_table() {
        let document = roxmltree::Document::parse(
            r#"
            <component name="MixedWorkbook">
              <data>
                <root>
                  <entry name="FileInstance">
                    <entry name="document">
                      <entry name="Workbook">
                        <entry name="Worksheet">
                          <condition><expression><function name="equal-ignorecase" library="xlsx">
                            <expression><attribute name="Name"/></expression>
                            <expression><constant value="Sales"/></expression>
                          </function></expression></condition>
                          <ranges>
                            <range id="fixed" start="1" count="1"/>
                            <range id="row" start="2" count="1"/>
                          </ranges>
                          <entry name="Row">
                            <condition><expression><function name="is-range-id">
                              <expression><constant value="fixed"/></expression>
                            </function></expression></condition>
                            <entry name="Cell" outkey="101" annotation="Year" datatype="string">
                              <condition><expression><function name="equal" library="core">
                                <expression><attribute name="n"/></expression>
                                <expression><constant value="1" datatype="long"/></expression>
                              </function></expression></condition>
                            </entry>
                          </entry>
                          <entry name="Row">
                            <condition><expression><function name="is-range-id">
                              <expression><constant value="row"/></expression>
                            </function></expression></condition>
                            <entry name="Cell" outkey="102" annotation="Month" datatype="string">
                              <entry name="n" outkey="103"/>
                            </entry>
                          </entry>
                        </entry>
                      </entry>
                    </entry>
                  </entry>
                </root>
                <excel inputinstance="mixed.xlsx"/>
              </data>
            </component>
            "#,
        )
        .unwrap();
        let mut warnings = Vec::new();

        let component = read(&document.root_element(), &mut warnings).unwrap();

        assert!(component.options.xlsx_composite.is_none());
        assert_eq!(component.options.xlsx_rows, [2]);
        assert!(warnings.iter().any(|warning| warning.contains(
            "combines fixed records with a transposed table; that composite layout is unsupported"
        )));
    }

    #[test]
    fn grid_field_collision_warns_and_falls_back() {
        let document = roxmltree::Document::parse(
            r#"
            <component name="Grid">
              <data>
                <root><entry name="Workbook"><entry name="Worksheet">
                  <ranges>
                    <range id="1" start="1" count="1"/>
                    <range id="2" start="2"/>
                  </ranges>
                  <entry name="Row">
                    <condition><function name="is-range-id"><constant value="1"/></function></condition>
                    <entry name="Cell" annotation="value" outkey="1">
                      <entry name="n" outkey="2"/>
                    </entry>
                  </entry>
                  <entry name="Row" outkey="3">
                    <condition><function name="is-range-id"><constant value="2"/></function></condition>
                    <entry name="Cell" outkey="4"><entry name="n" outkey="5"/></entry>
                  </entry>
                </entry></entry></root>
                <excel inputinstance="grid.xlsx"/>
              </data>
            </component>
            "#,
        )
        .unwrap();
        let mut warnings = Vec::new();

        let component = read(&document.root_element(), &mut warnings).unwrap();

        assert!(component.options.xlsx_grid.is_none());
        assert!(warnings.iter().any(|warning| {
            warning.contains("nested-grid header name `value` conflicts with a generated field")
        }));
    }

    #[test]
    fn grid_rejects_runtime_invalid_row_order_and_connected_header_rows() {
        let (component, warnings) = read_minimal_grid(&minimal_grid(2, 1, None, true));
        assert!(component.options.xlsx_grid.is_none());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("data row must start after its header row"))
        );

        let (component, warnings) = read_minimal_grid(&minimal_grid(1, 2, Some(6), true));
        assert!(component.options.xlsx_grid.is_none());
        assert!(
            warnings
                .iter()
                .any(|warning| { warning.contains("header Row connections are not supported") })
        );
    }

    #[test]
    fn grid_accepts_position_only_cell_sequences() {
        let (component, warnings) = read_minimal_grid(&minimal_grid(1, 2, None, false));

        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(component.options.xlsx_grid.is_some());
        assert!(!component.ports.contains_key(&1));
        assert!(!component.ports.contains_key(&4));
        assert_eq!(component.ports.get(&2), Some(&vec!["HeaderColumn".into()]));
        assert_eq!(
            component.ports.get(&5),
            Some(&vec!["Rows".into(), "Cells".into(), "CellColumn".into()])
        );
    }

    #[test]
    fn flat_target_retains_existing_workbook_update_mode() {
        let document = roxmltree::Document::parse(
            r#"
            <component name="Report">
              <data>
                <root><entry name="Workbook"><entry name="Worksheet">
                  <condition><function name="equal-ignorecase" library="xlsx">
                    <attribute name="Name"/><constant value="Sales"/>
                  </function></condition>
                  <ranges><range id="2" start="5"/></ranges>
                  <entry name="Row" inpkey="10" enabletitlerow="1">
                    <condition><function name="is-range-id"><constant value="2"/></function></condition>
                    <entry name="Cell" inpkey="11" annotation="Month" datatype="string">
                      <condition><function name="equal"><attribute name="n"/><constant value="1"/></function></condition>
                    </entry>
                  </entry>
                </entry></entry></root>
                <excel outputinstance="report.xlsx" updateexistingfile="1"/>
              </data>
            </component>
            "#,
        )
        .unwrap();
        let mut warnings = Vec::new();

        let component = read(&document.root_element(), &mut warnings).unwrap();

        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(component.options.xlsx_update_existing);
        assert_eq!(component.options.xlsx_sheet.as_deref(), Some("Sales"));
        assert_eq!(component.options.xlsx_start_row, Some(5));
    }
}
