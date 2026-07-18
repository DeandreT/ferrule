use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::{
    FormatOptions, TabularBoundaryKind, XlsxCellKind, XlsxColumn, XlsxHierarchicalLayout,
    XlsxOutputColumn, XlsxOutputRange, XlsxRangeStart, XlsxRow,
};

use super::{
    ComponentFormat, SchemaComponent, is_default_output, port_keys, scalar_type, selected_column,
    selected_range_id,
};

const WORKSHEETS_FIELD: &str = "Worksheets";
const WORKSHEET_NAME_FIELD: &str = "Name";

#[allow(clippy::too_many_arguments)]
pub(super) fn read(
    component: roxmltree::Node<'_, '_>,
    excel: roxmltree::Node<'_, '_>,
    workbook: roxmltree::Node<'_, '_>,
    component_name: &str,
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
    is_source: bool,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    if is_source {
        return None;
    }
    let mut worksheets = workbook
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Worksheet"))
        .filter(|worksheet| !port_keys(*worksheet).is_empty());
    let worksheet = worksheets.next()?;
    if worksheets.next().is_some() {
        warnings.push(format!(
            "xlsx component `{component_name}` maps multiple dynamic worksheet templates; hierarchical output currently supports one template"
        ));
        return None;
    }
    if worksheet
        .children()
        .any(|node| node.has_tag_name("condition"))
    {
        warnings.push(format!(
            "xlsx component `{component_name}` combines a connected worksheet sequence with a static selector; hierarchical output skipped"
        ));
        return None;
    }
    let name_entry = worksheet.children().find(|node| {
        node.has_tag_name("entry") && node.attribute("name") == Some(WORKSHEET_NAME_FIELD)
    })?;
    let worksheet_keys = port_keys(worksheet);
    let name_keys = port_keys(name_entry);
    if name_keys.is_empty() {
        warnings.push(format!(
            "xlsx component `{component_name}` generates worksheets dynamically but does not map their names"
        ));
        return None;
    }

    let range_by_id: BTreeMap<&str, roxmltree::Node<'_, '_>> = worksheet
        .children()
        .find(|node| node.has_tag_name("ranges"))
        .into_iter()
        .flat_map(|ranges| ranges.children().filter(|node| node.has_tag_name("range")))
        .filter_map(|range| range.attribute("id").map(|id| (id, range)))
        .collect();

    let mut ranges = Vec::new();
    let mut range_fields = Vec::new();
    let mut ports = BTreeMap::new();
    for key in worksheet_keys {
        ports.insert(key, vec![WORKSHEETS_FIELD.to_string()]);
    }
    for key in name_keys {
        ports.insert(
            key,
            vec![
                WORKSHEETS_FIELD.to_string(),
                WORKSHEET_NAME_FIELD.to_string(),
            ],
        );
    }

    for row in worksheet
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Row"))
    {
        let Some(range_id) = selected_range_id(row) else {
            if row.descendants().any(|node| !port_keys(node).is_empty()) {
                warnings.push(format!(
                    "xlsx component `{component_name}` has a connected hierarchical row without a range selector; output skipped"
                ));
                return None;
            }
            continue;
        };
        let Some(range) = range_by_id.get(range_id.as_str()).copied() else {
            warnings.push(format!(
                "xlsx component `{component_name}` references missing hierarchical range `{range_id}`"
            ));
            return None;
        };
        let parsed = read_range(
            row,
            range,
            &range_id,
            component_name,
            ranges.is_empty(),
            warnings,
        )?;
        let range_name = format!("Range{range_id}");
        let range_path = vec![range_name.clone()];
        for key in parsed.row_ports {
            ports.insert(key, vec![WORKSHEETS_FIELD.to_string(), range_name.clone()]);
        }
        let mut fields = Vec::with_capacity(parsed.columns.len());
        let mut layout_columns = Vec::with_capacity(parsed.columns.len());
        for column in parsed.columns {
            for key in column.ports {
                ports.insert(
                    key,
                    vec![
                        WORKSHEETS_FIELD.to_string(),
                        range_name.clone(),
                        column.name.clone(),
                    ],
                );
            }
            fields.push(SchemaNode::scalar(&column.name, column.scalar_type));
            layout_columns.push(XlsxOutputColumn {
                path: vec![column.name],
                column: column.column,
                header: column.header,
                kind: column.kind,
            });
        }
        let field = SchemaNode::group(&range_name, fields);
        range_fields.push(if parsed.repeating {
            field.repeating()
        } else {
            field
        });
        ranges.push(XlsxOutputRange {
            path: range_path,
            start: parsed.start,
            count: parsed.count,
            has_header: parsed.has_header,
            columns: layout_columns,
        });
    }

    if ranges.is_empty() {
        return None;
    }
    if excel.attribute("updateexistingfile") == Some("1") {
        warnings.push(format!(
            "xlsx component `{component_name}` updates an existing workbook; hierarchical import requires a new-workbook target"
        ));
        return None;
    }

    let mut worksheet_fields = Vec::with_capacity(range_fields.len() + 1);
    worksheet_fields.push(SchemaNode::scalar(WORKSHEET_NAME_FIELD, ScalarType::String));
    worksheet_fields.extend(range_fields);
    Some(SchemaComponent {
        name: component_name.to_string(),
        format: ComponentFormat::Xlsx,
        schema: SchemaNode::group(
            component_name,
            vec![SchemaNode::group(WORKSHEETS_FIELD, worksheet_fields).repeating()],
        ),
        input_instance: excel.attribute("inputinstance").map(str::to_string),
        output_instance: excel.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            xlsx_hierarchical: Some(XlsxHierarchicalLayout {
                worksheets_path: vec![WORKSHEETS_FIELD.to_string()],
                worksheet_name_path: vec![WORKSHEET_NAME_FIELD.to_string()],
                ranges,
            }),
            ..FormatOptions::default()
        },
        is_source: false,
        is_default_output: is_default_output(&component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

struct ParsedRange {
    start: XlsxRangeStart,
    count: Option<XlsxRow>,
    has_header: bool,
    repeating: bool,
    row_ports: Vec<u32>,
    columns: Vec<ParsedColumn>,
}

struct ParsedColumn {
    name: String,
    column: XlsxColumn,
    scalar_type: ScalarType,
    kind: XlsxCellKind,
    header: Option<String>,
    ports: Vec<u32>,
}

fn read_range(
    row: roxmltree::Node<'_, '_>,
    range: roxmltree::Node<'_, '_>,
    range_id: &str,
    component_name: &str,
    is_first: bool,
    warnings: &mut Vec<String>,
) -> Option<ParsedRange> {
    let start = if let Some(start) = range.attribute("start") {
        let Some(row) = start.parse::<u32>().ok().and_then(XlsxRow::new) else {
            warnings.push(format!(
                "xlsx component `{component_name}` range `{range_id}` has invalid absolute start row `{start}`"
            ));
            return None;
        };
        XlsxRangeStart::Absolute { row }
    } else if let Some(offset) = range.attribute("offset") {
        let Some(offset) = offset.parse::<u32>().ok().and_then(XlsxRow::new) else {
            warnings.push(format!(
                "xlsx component `{component_name}` range `{range_id}` has invalid previous-range offset `{offset}`"
            ));
            return None;
        };
        XlsxRangeStart::AfterPrevious { offset }
    } else if is_first {
        XlsxRangeStart::Absolute {
            row: XlsxRow::new(1)?,
        }
    } else {
        XlsxRangeStart::AfterPrevious {
            offset: XlsxRow::new(1)?,
        }
    };
    let count = match range.attribute("count") {
        Some(count) => match count.parse::<u32>().ok().and_then(XlsxRow::new) {
            Some(count) => Some(count),
            None => {
                warnings.push(format!(
                    "xlsx component `{component_name}` range `{range_id}` has invalid row count `{count}`"
                ));
                return None;
            }
        },
        None => None,
    };
    let row_ports = port_keys(row);
    let repeating = !row_ports.is_empty();
    if repeating && count.is_some() {
        warnings.push(format!(
            "xlsx component `{component_name}` range `{range_id}` combines a row sequence with a fixed count; truncating hierarchical ranges is not supported"
        ));
        return None;
    }
    if !repeating && count.map(XlsxRow::get) != Some(1) {
        warnings.push(format!(
            "xlsx component `{component_name}` range `{range_id}` must map a row sequence unless it is a single fixed row"
        ));
        return None;
    }
    let has_header = row.attribute("enabletitlerow") == Some("1");
    let mut columns = Vec::new();
    let mut names = BTreeSet::new();
    let mut indexes = BTreeSet::new();
    let connected_cells = row
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && node.attribute("name") == Some("Cell")
                && !port_keys(*node).is_empty()
        })
        .collect::<Vec<_>>();
    for cell in &connected_cells {
        let index = selected_column(*cell)
            .and_then(XlsxColumn::new)
            .or_else(|| {
                (connected_cells.len() == 1)
                    .then(|| XlsxColumn::new(1))
                    .flatten()
            });
        let Some(index) = index else {
            warnings.push(format!(
                "xlsx component `{component_name}` range `{range_id}` has a connected cell without one fixed column"
            ));
            return None;
        };
        let annotation = cell
            .attribute("annotation")
            .filter(|value| !value.is_empty());
        let name = annotation
            .map(str::to_string)
            .unwrap_or_else(|| format!("Column{}", index.get()));
        if !names.insert(name.clone()) || !indexes.insert(index) {
            warnings.push(format!(
                "xlsx component `{component_name}` range `{range_id}` maps a field name or column more than once"
            ));
            return None;
        }
        let datatype = cell.attribute("datatype");
        columns.push(ParsedColumn {
            name,
            column: index,
            scalar_type: scalar_type(datatype),
            kind: cell_kind(datatype),
            header: has_header.then(|| annotation.map_or_else(String::new, str::to_string)),
            ports: port_keys(*cell),
        });
    }
    if columns.is_empty() {
        warnings.push(format!(
            "xlsx component `{component_name}` range `{range_id}` has no connected fixed-column cells"
        ));
        return None;
    }
    Some(ParsedRange {
        start,
        count,
        has_header,
        repeating,
        row_ports,
        columns,
    })
}

fn cell_kind(datatype: Option<&str>) -> XlsxCellKind {
    match datatype {
        Some("double" | "decimal" | "number" | "float" | "integer" | "int" | "long") => {
            XlsxCellKind::Number
        }
        Some("boolean" | "bool") => XlsxCellKind::Boolean,
        Some("date") => XlsxCellKind::Date,
        Some("dateTime") => XlsxCellKind::DateTime,
        Some("time") => XlsxCellKind::Time,
        _ => XlsxCellKind::String,
    }
}
