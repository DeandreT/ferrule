use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::FormatOptions;

use super::{ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32};

const MAX_WORKSHEET_ROW: u32 = 1_048_576;
const MAX_WORKSHEET_COLUMN: u32 = 16_384;

struct Column {
    name: String,
    index: u32,
    ty: ScalarType,
    ports: Vec<u32>,
}

struct Table {
    sheet: Option<String>,
    start_row: Option<u32>,
    has_header: bool,
    row_ports: Vec<u32>,
    columns: Vec<Column>,
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

    let mut tables = Vec::new();
    for worksheet in workbook
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Worksheet"))
    {
        inspect_worksheet(worksheet, &name, warnings, &mut tables);
    }

    let table = match tables.len() {
        0 => {
            warnings.push(format!(
                "xlsx component `{name}` has no supported flat table: select one worksheet with one open row range and annotation-named, fixed-index columns"
            ));
            return None;
        }
        1 => tables.pop()?,
        count => {
            warnings.push(format!(
                "xlsx component `{name}` contains {count} flat tables; ferrule currently imports one worksheet/table per component"
            ));
            return None;
        }
    };

    if excel.attribute("updateexistingfile") == Some("1") {
        warnings.push(format!(
            "xlsx component `{name}` updates an existing workbook; ferrule writes a new workbook, so content outside the selected table will not be preserved"
        ));
    }

    let mut ports = BTreeMap::new();
    for key in table.row_ports {
        ports.insert(key, Vec::new());
    }
    let mut fields = Vec::with_capacity(table.columns.len());
    let mut xlsx_columns = Vec::with_capacity(table.columns.len());
    for column in table.columns {
        for key in column.ports {
            ports.insert(key, vec![column.name.clone()]);
        }
        fields.push(SchemaNode::scalar(&column.name, column.ty));
        xlsx_columns.push(column.index);
    }

    Some(SchemaComponent {
        name: name.clone(),
        format: ComponentFormat::Xlsx,
        schema: SchemaNode::group(&name, fields),
        input_instance: excel.attribute("inputinstance").map(str::to_string),
        output_instance: excel.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            has_header_row: Some(table.has_header),
            xlsx_sheet: table.sheet,
            xlsx_start_row: table.start_row,
            xlsx_columns,
            ..FormatOptions::default()
        },
        is_source,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn inspect_worksheet(
    worksheet: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
    tables: &mut Vec<Table>,
) {
    let range_by_id: BTreeMap<&str, roxmltree::Node<'_, '_>> = worksheet
        .children()
        .find(|node| node.has_tag_name("ranges"))
        .into_iter()
        .flat_map(|ranges| ranges.children().filter(|node| node.has_tag_name("range")))
        .filter_map(|range| range.attribute("id").map(|id| (id, range)))
        .collect();

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
        if range.attribute("count").is_some() {
            if subtree_has_ports(row) {
                warnings.push(format!(
                    "xlsx component `{component_name}` has connected fixed range `{range_id}`; only open row tables import, so those ports were skipped"
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
            start_row,
            has_header: row.attribute("enabletitlerow") == Some("1"),
            row_ports,
            columns,
        });
    }
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

fn selected_range_id(row: roxmltree::Node<'_, '_>) -> Option<String> {
    let function = row.descendants().find(|node| {
        node.has_tag_name("function") && node.attribute("name") == Some("is-range-id")
    })?;
    function
        .descendants()
        .find(|node| node.has_tag_name("constant"))?
        .attribute("value")
        .map(str::to_string)
}

fn selected_column(cell: roxmltree::Node<'_, '_>) -> Option<u32> {
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

fn scalar_type(datatype: Option<&str>) -> ScalarType {
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

fn port_keys(node: roxmltree::Node<'_, '_>) -> Vec<u32> {
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
