use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::{
    FormatOptions, TabularBoundaryKind, XlsxFixedCell, XlsxGridLayout, XlsxGridRegion, XlsxRow,
};

use super::{
    ComponentFormat, FixedCell, SchemaComponent, duplicate_fixed_cell, is_default_output,
    port_keys, read_fixed_cells, scalar_type, selected_column, selected_range_id, worksheet_name,
};

const HEADER_POSITION_FIELD: &str = "HeaderColumn";
const ROWS_FIELD: &str = "Rows";
const CELLS_FIELD: &str = "Cells";
const CELL_VALUE_FIELD: &str = "value";
const CELL_POSITION_FIELD: &str = "CellColumn";

#[derive(Clone)]
struct OpenCells {
    row: XlsxRow,
    value_type: ScalarType,
    value_ports: Vec<u32>,
    position_ports: Vec<u32>,
    row_ports: Vec<u32>,
    field_name: String,
}

/// One worksheet that holds a header-driven grid.
pub(super) struct GridWorksheet {
    pub(super) node: roxmltree::NodeId,
    /// The Worksheet entry's own ports. A composite region iterates its
    /// header records from them, as composite tables iterate their rows.
    ports: Vec<u32>,
    candidate: Candidate,
}

impl GridWorksheet {
    pub(super) fn sheet(&self) -> Option<&str> {
        self.candidate.sheet.as_deref()
    }
}

/// Finds the grid on each source worksheet. Several grids on one worksheet
/// refuse the whole component, because pairing their header and data ranges
/// needs native observation that ferrule does not have.
pub(super) fn scan(
    workbook: roxmltree::Node<'_, '_>,
    component_name: &str,
    is_source: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<GridWorksheet>, ()> {
    if !is_source {
        return Ok(Vec::new());
    }
    let mut grids = Vec::new();
    for worksheet in workbook
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Worksheet"))
    {
        if let Some(candidate) = inspect_worksheet(worksheet, component_name, warnings)? {
            grids.push(GridWorksheet {
                node: worksheet.id(),
                ports: port_keys(worksheet),
                candidate,
            });
        }
    }
    Ok(grids)
}

/// The single-grid source: the header records are the repeated root.
pub(super) fn single_component(
    component: roxmltree::Node<'_, '_>,
    excel: roxmltree::Node<'_, '_>,
    component_name: &str,
    grid: GridWorksheet,
    input_keys: BTreeSet<u32>,
    output_keys: BTreeSet<u32>,
    warnings: &mut Vec<String>,
) -> SchemaComponent {
    let mut ports = BTreeMap::new();
    let (fields, layout) = grid_fields(grid.candidate, &[], &mut ports);

    if excel.attribute("updateexistingfile") == Some("1") {
        warnings.push(format!(
            "xlsx component `{component_name}` updates an existing workbook; ferrule writes a new workbook, so content outside the selected table will not be preserved"
        ));
    }
    SchemaComponent {
        name: component_name.to_string(),
        format: ComponentFormat::Xlsx,
        schema: SchemaNode::group(component_name, fields),
        input_instance: excel.attribute("inputinstance").map(str::to_string),
        output_instance: excel.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            xlsx_grid: Some(layout),
            ..FormatOptions::default()
        },
        is_source: true,
        is_default_output: is_default_output(&component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_branch_paths: Vec::new(),
        ambiguous_branch_inputs: Default::default(),
        has_target_node_controls: false,
        input_keys,
        output_keys,
        json_schema_authoritative: false,
        port_declarations: None,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
        target_schema_hints: None,
    }
}

/// A grid region of a composite source: a repeating group named after its
/// static worksheet, holding one record per non-empty header cell.
pub(super) fn composite_region(
    grid: GridWorksheet,
    component_name: &str,
    ports: &mut BTreeMap<u32, Vec<String>>,
    warnings: &mut Vec<String>,
) -> Option<(SchemaNode, XlsxGridRegion)> {
    let Some(sheet) = grid
        .candidate
        .sheet
        .clone()
        .filter(|sheet| !sheet.is_empty())
    else {
        warnings.push(format!(
            "xlsx component `{component_name}` combines workbook regions with a default worksheet grid; every grid needs a static worksheet name"
        ));
        return None;
    };
    let path = vec![sheet.clone()];
    for key in grid.ports {
        ports.insert(key, path.clone());
    }
    let (fields, layout) = grid_fields(grid.candidate, &path, ports);
    Some((
        SchemaNode::group(&sheet, fields).repeating(),
        XlsxGridRegion { path, layout },
    ))
}

/// The fields of one header record and its layout. Ports are recorded below
/// `prefix`, the record group's absolute path.
fn grid_fields(
    candidate: Candidate,
    prefix: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
) -> (Vec<SchemaNode>, XlsxGridLayout) {
    let path = |segments: &[&str]| {
        prefix
            .iter()
            .cloned()
            .chain(segments.iter().map(|segment| (*segment).to_string()))
            .collect::<Vec<_>>()
    };
    for key in &candidate.header.value_ports {
        ports.insert(*key, path(&[&candidate.header.field_name]));
    }
    for key in &candidate.header.position_ports {
        ports.insert(*key, path(&[HEADER_POSITION_FIELD]));
    }

    let mut fields = vec![
        SchemaNode::scalar(&candidate.header.field_name, candidate.header.value_type),
        SchemaNode::scalar(HEADER_POSITION_FIELD, ScalarType::Int),
    ];
    let mut fixed_layout = Vec::with_capacity(candidate.fixed.len());
    for cell in &candidate.fixed {
        for key in &cell.ports {
            ports.insert(*key, path(&[&cell.name]));
        }
        fields.push(SchemaNode::scalar(&cell.name, cell.ty));
        fixed_layout.push(XlsxFixedCell {
            path: vec![cell.name.clone()],
            row: cell.row,
            column: cell.column,
        });
    }

    for key in &candidate.data.row_ports {
        ports.insert(*key, path(&[ROWS_FIELD]));
    }
    for key in &candidate.data.value_ports {
        ports.insert(*key, path(&[ROWS_FIELD, CELLS_FIELD, CELL_VALUE_FIELD]));
    }
    for key in &candidate.data.position_ports {
        ports.insert(*key, path(&[ROWS_FIELD, CELLS_FIELD, CELL_POSITION_FIELD]));
    }
    fields.push(
        SchemaNode::group(
            ROWS_FIELD,
            vec![
                SchemaNode::group(
                    CELLS_FIELD,
                    vec![
                        SchemaNode::scalar(CELL_VALUE_FIELD, candidate.data.value_type),
                        SchemaNode::scalar(CELL_POSITION_FIELD, ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        )
        .repeating(),
    );
    let layout = XlsxGridLayout {
        sheet: candidate.sheet,
        header_row: candidate.header.row,
        data_start_row: candidate.data.row,
        header_value_field: candidate.header.field_name,
        header_position_field: HEADER_POSITION_FIELD.to_string(),
        rows_field: ROWS_FIELD.to_string(),
        cells_field: CELLS_FIELD.to_string(),
        cell_value_field: CELL_VALUE_FIELD.to_string(),
        cell_position_field: CELL_POSITION_FIELD.to_string(),
        fixed_cells: fixed_layout,
    };
    (fields, layout)
}

struct Candidate {
    sheet: Option<String>,
    header: OpenCells,
    data: OpenCells,
    fixed: Vec<FixedCell>,
}

fn inspect_worksheet(
    worksheet: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<Candidate>, ()> {
    Ok(inspect_grid(worksheet, component_name, warnings)?
        .and_then(|raw| validate_candidate(worksheet, raw, component_name, warnings)))
}

struct RawGrid {
    headers: Vec<OpenCells>,
    data_rows: Vec<OpenCells>,
    fixed: Vec<FixedCell>,
    local_warnings: Vec<String>,
}

fn inspect_grid(
    worksheet: roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<RawGrid>, ()> {
    let range_by_id: BTreeMap<&str, roxmltree::Node<'_, '_>> = worksheet
        .children()
        .find(|node| node.has_tag_name("ranges"))
        .into_iter()
        .flat_map(|ranges| ranges.children().filter(|node| node.has_tag_name("range")))
        .filter_map(|range| range.attribute("id").map(|id| (id, range)))
        .collect();
    let mut headers = Vec::new();
    let mut data_rows = Vec::new();
    let mut fixed = Vec::new();
    let mut local_warnings = Vec::new();

    for row in worksheet
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Row"))
    {
        let Some(range_id) = selected_range_id(row) else {
            continue;
        };
        let Some(range) = range_by_id.get(range_id.as_str()).copied() else {
            continue;
        };
        if range.attribute("offset").is_some() {
            continue;
        }
        if range.attribute("count") == Some("1") {
            if let Some(header) = open_cells(row, range, format!("Range{range_id}")) {
                headers.push(header);
            } else {
                fixed.extend(read_fixed_cells(
                    row,
                    range,
                    component_name,
                    &mut local_warnings,
                ));
            }
        } else if range.attribute("count").is_none()
            && let Some(data) = open_cells(row, range, CELL_VALUE_FIELD.to_string())
            && !data.row_ports.is_empty()
        {
            data_rows.push(data);
        }
    }

    if headers.is_empty() || data_rows.is_empty() {
        return Ok(None);
    }
    if headers.len() > 1 && data_rows.len() > 1 {
        let sheet = match worksheet_name(worksheet) {
            Ok(Some(sheet)) => format!(" `{sheet}`"),
            Ok(None) | Err(()) => String::new(),
        };
        warnings.push(format!(
            "xlsx component `{component_name}` has several nested grids on worksheet{sheet}; ferrule imports one grid per worksheet because pairing several header and data ranges on one worksheet is unsupported"
        ));
        return Err(());
    }
    Ok(Some(RawGrid {
        headers,
        data_rows,
        fixed,
        local_warnings,
    }))
}

fn validate_candidate(
    worksheet: roxmltree::Node<'_, '_>,
    raw: RawGrid,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Option<Candidate> {
    let RawGrid {
        mut headers,
        mut data_rows,
        fixed,
        local_warnings,
    } = raw;
    if headers.len() != 1 || data_rows.len() != 1 {
        warnings.push(format!(
            "xlsx component `{component_name}` has an ambiguous nested grid: expected one fixed header row and one open data row range"
        ));
        return None;
    }
    let header = headers.pop()?;
    let data = data_rows.pop()?;
    if data.row.get() <= header.row.get() {
        warnings.push(format!(
            "xlsx component `{component_name}` nested grid data row must start after its header row"
        ));
        return None;
    }
    if !header.row_ports.is_empty() {
        warnings.push(format!(
            "xlsx component `{component_name}` connects the nested grid header Row sequence; header Row connections are not supported"
        ));
        return None;
    }
    if header.position_ports.is_empty() || data.position_ports.is_empty() {
        warnings.push(format!(
            "xlsx component `{component_name}` nested grid must expose physical Cell/@n ports for both header and data cells"
        ));
        return None;
    }
    if duplicate_fixed_cell(&fixed) {
        warnings.push(format!(
            "xlsx component `{component_name}` nested grid maps a fixed-cell coordinate or field name more than once"
        ));
        return None;
    }
    let generated_names = BTreeSet::from([
        HEADER_POSITION_FIELD,
        ROWS_FIELD,
        CELLS_FIELD,
        CELL_VALUE_FIELD,
        CELL_POSITION_FIELD,
    ]);
    if generated_names.contains(header.field_name.as_str()) {
        warnings.push(format!(
            "xlsx component `{component_name}` nested-grid header name `{}` conflicts with a generated field",
            header.field_name
        ));
        return None;
    }
    if fixed
        .iter()
        .any(|cell| cell.name == header.field_name || generated_names.contains(cell.name.as_str()))
    {
        warnings.push(format!(
            "xlsx component `{component_name}` nested grid uses a fixed-cell name that conflicts with its header or generated row/cell fields"
        ));
        return None;
    }
    let sheet = match worksheet_name(worksheet) {
        Ok(sheet) => sheet,
        Err(()) => {
            warnings.push(format!(
                "xlsx component `{component_name}` selects its nested-grid worksheet dynamically; grid skipped"
            ));
            return None;
        }
    };
    warnings.extend(local_warnings);
    Some(Candidate {
        sheet,
        header,
        data,
        fixed,
    })
}

fn open_cells(
    row: roxmltree::Node<'_, '_>,
    range: roxmltree::Node<'_, '_>,
    field_name: String,
) -> Option<OpenCells> {
    let physical_row = range
        .attribute("start")
        .and_then(|value| value.parse::<u32>().ok())
        .and_then(XlsxRow::new)?;
    let mut cells = row.children().filter(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("Cell")
            && selected_column(*node).is_none()
            && (!port_keys(*node).is_empty()
                || node.children().any(|child| {
                    child.has_tag_name("entry")
                        && child.attribute("name") == Some("n")
                        && !port_keys(child).is_empty()
                }))
    });
    let cell = cells.next()?;
    if cells.next().is_some() || cell.children().any(|node| node.has_tag_name("condition")) {
        return None;
    }
    let position_ports = cell
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("n"))
        .flat_map(port_keys)
        .collect();
    Some(OpenCells {
        row: physical_row,
        value_type: scalar_type(cell.attribute("datatype")),
        value_ports: port_keys(cell),
        position_ports,
        row_ports: port_keys(row),
        field_name: cell
            .attribute("annotation")
            .or_else(|| row.attribute("annotation"))
            .or_else(|| range.attribute("annotation"))
            .filter(|name| !name.is_empty())
            .map_or(field_name, str::to_string),
    })
}
