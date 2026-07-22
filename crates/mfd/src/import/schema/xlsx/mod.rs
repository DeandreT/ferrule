use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::{
    FormatOptions, TabularBoundaryKind, XlsxColumn, XlsxCompositeLayout, XlsxFixedCell,
    XlsxFixedRecord, XlsxRow, XlsxTableRegion,
};

use super::{ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32};

mod grid;
mod hierarchical;
mod worksheet_set;

const MAX_WORKSHEET_ROW: u32 = 1_048_576;
const MAX_WORKSHEET_COLUMN: u32 = 16_384;

#[derive(Clone)]
struct Column {
    name: String,
    header: String,
    index: u32,
    ty: ScalarType,
    ports: Vec<u32>,
}

#[derive(Clone)]
struct Table {
    sheet: Option<String>,
    worksheet_ports: Vec<u32>,
    worksheet_name_ports: Vec<u32>,
    layout: TableLayout,
}

#[derive(Clone)]
enum TableLayout {
    Flat {
        start_row: Option<u32>,
        has_header: bool,
        row_ports: Vec<u32>,
        row_number_ports: Vec<u32>,
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
        return Some(with_workbook_root_ports(hierarchical, workbook));
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
        return Some(with_workbook_root_ports(grid, workbook));
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

    if let Some(worksheet_set) = worksheet_set::read(
        &name,
        excel,
        &tables,
        input_keys.clone(),
        output_keys.clone(),
        is_source,
        is_default_output(component),
        warnings,
    ) {
        return Some(with_workbook_root_ports(worksheet_set, workbook));
    }

    if (!records.is_empty() || tables.len() > 1)
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
        return Some(with_workbook_root_ports(composite, workbook));
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

    warn_dynamic_table_ports(&table, &name, warnings);
    let mut ports = BTreeMap::new();
    let (fields, options) = match table.layout {
        TableLayout::Flat {
            start_row,
            has_header,
            row_ports,
            row_number_ports,
            columns,
        } => {
            for key in row_ports {
                ports.insert(key, Vec::new());
            }
            if !row_number_ports.is_empty() {
                warnings.push(format!(
                    "xlsx component `{name}` exposes physical Row/@r ports; those index values were skipped"
                ));
            }
            let mut fields = Vec::with_capacity(columns.len());
            let mut xlsx_columns = Vec::with_capacity(columns.len());
            let xlsx_headers = if columns.iter().any(|column| column.name != column.header) {
                columns.iter().map(|column| column.header.clone()).collect()
            } else {
                Vec::new()
            };
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
                    tabular_kind: Some(TabularBoundaryKind::Xlsx),
                    has_header_row: Some(has_header),
                    xlsx_sheet: table.sheet,
                    xlsx_start_row: start_row,
                    xlsx_columns,
                    xlsx_headers,
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
                    tabular_kind: Some(TabularBoundaryKind::Xlsx),
                    has_header_row: Some(false),
                    xlsx_sheet: table.sheet,
                    xlsx_rows,
                    ..FormatOptions::default()
                },
            )
        }
    };

    Some(with_workbook_root_ports(
        SchemaComponent {
            name: name.clone(),
            format: ComponentFormat::Xlsx,
            schema: SchemaNode::group(&name, fields),
            input_instance: excel.attribute("inputinstance").map(str::to_string),
            output_instance: excel.attribute("outputinstance").map(str::to_string),
            options,
            is_source,
            is_default_output: is_default_output(component),
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
        },
        workbook,
    ))
}

fn with_workbook_root_ports(
    mut component: SchemaComponent,
    workbook: roxmltree::Node<'_, '_>,
) -> SchemaComponent {
    for key in port_keys(workbook) {
        component.ports.insert(key, Vec::new());
    }
    component
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
    if tables.is_empty() {
        warnings.push(format!(
            "xlsx component `{name}` has fixed records but no supported open row table; composite XLSX sources require at least one table"
        ));
        return None;
    }
    if tables
        .iter()
        .any(|table| matches!(table.layout, TableLayout::Transposed { .. }))
    {
        let reason = if !records.is_empty() && tables.len() == 1 {
            "combines fixed records with a transposed table"
        } else {
            "combines multiple tables with a transposed table"
        };
        warnings.push(format!(
            "xlsx component `{name}` {reason}; that composite layout is unsupported"
        ));
        return None;
    }
    let mut names = BTreeSet::new();
    for table in &tables {
        let Some(sheet) = table.sheet.as_deref().filter(|sheet| !sheet.is_empty()) else {
            warnings.push(format!(
                "xlsx component `{name}` combines workbook regions with a default worksheet table; every table needs a static worksheet name"
            ));
            return None;
        };
        if !names.insert(sheet) {
            warnings.push(format!(
                "xlsx component `{name}` uses worksheet `{sheet}` for more than one composite table; component skipped"
            ));
            return None;
        }
    }
    if records
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

    let mut table_layouts = Vec::with_capacity(tables.len());
    for table in tables.drain(..) {
        let table_name = table.sheet.clone()?;
        let TableLayout::Flat {
            start_row,
            has_header,
            row_ports,
            row_number_ports,
            columns,
        } = table.layout
        else {
            return None;
        };
        let table_path = vec![table_name.clone()];
        for key in row_ports.into_iter().chain(table.worksheet_ports) {
            ports.insert(key, table_path.clone());
        }
        let mut table_fields = Vec::with_capacity(columns.len() + 1);
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
        let row_number_field = if row_number_ports.is_empty() {
            None
        } else {
            let field_name = unique_field_name("r", &table_fields);
            let mut field_path = table_path.clone();
            field_path.push(field_name.clone());
            for key in row_number_ports {
                ports.insert(key, field_path.clone());
            }
            table_fields.push(SchemaNode::scalar(&field_name, ScalarType::Int));
            Some(field_name)
        };
        if !table.worksheet_name_ports.is_empty() {
            warnings.push(format!(
                "xlsx component `{name}` has connected worksheet-name ports on static worksheet `{table_name}`; those constant names were skipped"
            ));
        }
        fields.push(SchemaNode::group(&table_name, table_fields).repeating());
        table_layouts.push(XlsxTableRegion {
            path: table_path,
            sheet: table.sheet,
            start_row: XlsxRow::new(start_row.unwrap_or(1))?,
            columns: table_columns,
            has_header,
            row_number_field,
        });
    }
    let table = table_layouts.remove(0);

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
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            xlsx_composite: Some(XlsxCompositeLayout {
                table,
                additional_tables: table_layouts,
                records: fixed_layouts,
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
        warn_physical_cell_index_ports(row, component_name, warnings);

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
        let mut columns = read_columns(row, component_name, warnings);
        if columns.is_empty() {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` range `{range_id}` has no supported annotation-named, fixed-index columns; table skipped"
                ));
            }
            continue;
        }
        if duplicate_column_index(&columns) {
            warnings.push(format!(
                "xlsx component `{component_name}` range `{range_id}` maps a physical column more than once; table skipped"
            ));
            continue;
        }
        if duplicate_column_name(&columns) {
            disambiguate_column_names(&mut columns);
        }
        let row_ports = port_keys(row);
        tables.push(Table {
            sheet,
            worksheet_ports: port_keys(worksheet),
            worksheet_name_ports: worksheet_name_ports(worksheet),
            layout: TableLayout::Flat {
                start_row,
                has_header: row.attribute("enabletitlerow") == Some("1"),
                row_ports,
                row_number_ports: physical_row_ports(row),
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
            tables.push(Table {
                sheet,
                worksheet_ports: port_keys(worksheet),
                worksheet_name_ports: worksheet_name_ports(worksheet),
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
        let semantic_name = cell
            .attribute("ferrulefield")
            .filter(|name| !name.is_empty());
        let Some(header) = cell
            .attribute("annotation")
            .filter(|header| !header.is_empty() || semantic_name.is_some())
        else {
            warnings.push(format!(
                "xlsx component `{component_name}` column {index} has no annotation name; that cell was skipped"
            ));
            continue;
        };
        let name = semantic_name.unwrap_or(header);
        columns.push(Column {
            name: name.to_string(),
            header: header.to_string(),
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

fn warn_dynamic_table_ports(table: &Table, component_name: &str, warnings: &mut Vec<String>) {
    if !table.worksheet_ports.is_empty() || !table.worksheet_name_ports.is_empty() {
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

fn warn_physical_cell_index_ports(
    row: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) {
    let cell_index = row.children().any(|cell| {
        cell.has_tag_name("entry")
            && cell.attribute("name") == Some("Cell")
            && cell.children().any(|node| {
                node.has_tag_name("entry")
                    && node.attribute("name") == Some("n")
                    && !port_keys(node).is_empty()
            })
    });
    if cell_index {
        warnings.push(format!(
            "xlsx component `{component_name}` exposes physical Cell/@n ports; those index values were skipped"
        ));
    }
}

fn worksheet_name_ports(worksheet: roxmltree::Node<'_, '_>) -> Vec<u32> {
    worksheet
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Name"))
        .flat_map(port_keys)
        .collect()
}

fn physical_row_ports(row: roxmltree::Node<'_, '_>) -> Vec<u32> {
    row.children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("r"))
        .flat_map(port_keys)
        .collect()
}

fn unique_field_name(base: &str, fields: &[SchemaNode]) -> String {
    if fields.iter().all(|field| field.name != base) {
        return base.to_string();
    }
    for suffix in 2usize.. {
        let candidate = format!("{base}_{suffix}");
        if fields.iter().all(|field| field.name != candidate) {
            return candidate;
        }
    }
    base.to_string()
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

fn duplicate_column_index(columns: &[Column]) -> bool {
    let mut indexes = BTreeSet::new();
    columns.iter().any(|column| !indexes.insert(column.index))
}

fn duplicate_column_name(columns: &[Column]) -> bool {
    let mut names = BTreeSet::new();
    columns.iter().any(|column| !names.insert(&column.name))
}

fn disambiguate_column_names(columns: &mut [Column]) {
    let mut available = columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for column in columns {
        if seen.insert(column.name.clone()) {
            continue;
        }
        let base = format!("{}_{}", column.name, column.index);
        let mut candidate = base.clone();
        let mut suffix = 2_u32;
        while available.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        available.insert(candidate.clone());
        seen.insert(candidate.clone());
        column.name = candidate;
    }
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
mod tests;
