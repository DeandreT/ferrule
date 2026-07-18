//! Canonical MapForce XLSX source components for retained non-flat layouts.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FormatOptions, XlsxCompositeLayout, XlsxFixedCell, XlsxGridLayout};

use crate::MfdError;

use super::schema::{PortTree, xml_escape};

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
}

/// Renders a retained transposed, composite, or two-dimensional XLSX source.
/// `None` leaves ordinary flat worksheet rendering to `schema`.
pub(super) fn render(args: RenderArgs<'_>) -> Result<Option<String>, MfdError> {
    if let Some(layout) = &args.options.xlsx_grid {
        return render_grid(&args, layout).map(Some);
    }
    if let Some(layout) = &args.options.xlsx_composite {
        return render_composite(&args, layout).map(Some);
    }
    if !args.options.xlsx_rows.is_empty() {
        return render_transposed(&args).map(Some);
    }
    Ok(None)
}

fn render_transposed(args: &RenderArgs<'_>) -> Result<String, MfdError> {
    let fields = direct_scalar_fields(args.schema).ok_or_else(|| {
        unsupported("a transposed XLSX source must have a flat scalar root schema")
    })?;
    let row_count = args.options.xlsx_rows.len();
    let has_index = fields.len() == row_count + 1
        && fields.last().is_some_and(|field| {
            field.name == "n"
                && matches!(
                    field.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::Int
                    }
                )
        });
    if fields.len() != row_count + usize::from(has_index) {
        return Err(unsupported(
            "a transposed XLSX source needs one scalar field per configured row and only an optional trailing integer `n` field",
        ));
    }
    let mut seen = BTreeSet::new();
    if args
        .options
        .xlsx_rows
        .iter()
        .any(|row| *row == 0 || *row > 1_048_576 || !seen.insert(*row))
    {
        return Err(unsupported(
            "transposed XLSX source rows must be unique one-based Excel row numbers",
        ));
    }

    let mut ranges = String::new();
    let mut rows = String::new();
    for (index, (field, physical_row)) in fields
        .iter()
        .take(row_count)
        .zip(&args.options.xlsx_rows)
        .enumerate()
    {
        let range_id = index + 1;
        let field_key = args
            .ports
            .required_key_for_abs(std::slice::from_ref(&field.name), "transposed XLSX field")?;
        let SchemaKind::Scalar { ty } = field.kind else {
            return Err(unsupported(
                "a transposed XLSX source contains a non-scalar row field",
            ));
        };
        let _ = writeln!(
            ranges,
            "\t\t\t\t\t\t\t\t\t\t\t<range id=\"{range_id}\" start=\"{physical_row}\" count=\"1\"/>"
        );
        let index_entry = if index == 0 && has_index {
            let index_key = args
                .ports
                .required_key_for_abs(&["n".to_string()], "transposed XLSX column index")?;
            format!(
                "\n\t\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"n\" type=\"attribute\" outkey=\"{index_key}\"/>"
            )
        } else {
            String::new()
        };
        let _ = writeln!(
            rows,
            "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"{range_id}\" datatype=\"long\"/></expression></function></expression></condition>\n\
             \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" outkey=\"{field_key}\" annotation=\"{}\" datatype=\"{}\" expanded=\"1\">{index_entry}\n\
             \t\t\t\t\t\t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t\t\t\t\t\t</entry>",
            xml_escape(&field.name),
            type_name(ty),
        );
    }
    render_component(
        args,
        worksheet(args.options.xlsx_sheet.as_deref(), &ranges, &rows)?,
    )
}

fn render_composite(
    args: &RenderArgs<'_>,
    layout: &XlsxCompositeLayout,
) -> Result<String, MfdError> {
    let mut worksheets = String::new();
    let mut names = BTreeSet::new();
    for record in &layout.records {
        let record_name = one_segment(&record.path, "composite XLSX fixed-record path")?;
        let sheet = required_matching_sheet(record.sheet.as_deref(), record_name)?;
        if !names.insert(sheet) {
            return Err(unsupported("composite XLSX worksheet names must be unique"));
        }
        let record_schema = schema_at(args.schema, &record.path).ok_or_else(|| {
            unsupported("a composite XLSX fixed-record path is missing from the source schema")
        })?;
        if record_schema.repeating || !matches!(record_schema.kind, SchemaKind::Group { .. }) {
            return Err(unsupported(
                "a composite XLSX fixed-record path must select a non-repeating group",
            ));
        }
        let group_key = args
            .ports
            .required_key_for_abs(&record.path, "composite XLSX fixed record")?;
        let mut ranges = String::new();
        let mut rows = String::new();
        for (index, cell) in record.cells.iter().enumerate() {
            let field_name = one_segment(&cell.path, "composite XLSX fixed-cell path")?;
            let field = schema_at(record_schema, &cell.path).ok_or_else(|| {
                unsupported("a composite XLSX fixed-cell path is missing from its record schema")
            })?;
            let ty = scalar_type(field, "composite XLSX fixed cell")?;
            let mut absolute = record.path.clone();
            absolute.extend(cell.path.iter().cloned());
            let key = args
                .ports
                .required_key_for_abs(&absolute, "composite XLSX fixed cell")?;
            render_fixed_cell(index + 1, cell, field_name, ty, key, &mut ranges, &mut rows);
        }
        let body = worksheet_body(sheet, &ranges, &rows, Some(group_key));
        worksheets.push_str(&body);
    }

    let table_name = one_segment(&layout.table.path, "composite XLSX table path")?;
    let table_sheet = required_matching_sheet(layout.table.sheet.as_deref(), table_name)?;
    if !names.insert(table_sheet) {
        return Err(unsupported("composite XLSX worksheet names must be unique"));
    }
    let table = schema_at(args.schema, &layout.table.path)
        .ok_or_else(|| unsupported("the composite XLSX table path is missing from the schema"))?;
    if !table.repeating {
        return Err(unsupported(
            "the composite XLSX table path must select a repeating group",
        ));
    }
    let fields = direct_scalar_fields(table)
        .ok_or_else(|| unsupported("the composite XLSX table must be a flat scalar group"))?;
    let columns = table_columns(fields.len(), &layout.table.columns)?;
    let table_key = args
        .ports
        .required_key_for_abs(&layout.table.path, "composite XLSX table")?;
    let mut cells = String::new();
    for (field, column) in fields.iter().zip(columns) {
        let mut path = layout.table.path.clone();
        path.push(field.name.clone());
        let key = args
            .ports
            .required_key_for_abs(&path, "composite XLSX table field")?;
        let ty = scalar_type(field, "composite XLSX table field")?;
        render_selected_cell(&field.name, ty, column, key, &mut cells);
    }
    let range = format!(
        "\t\t\t\t\t\t\t\t\t\t\t<range id=\"1\" start=\"{}\"/>\n",
        layout.table.start_row.get()
    );
    let header = if layout.table.has_header {
        " enabletitlerow=\"1\""
    } else {
        ""
    };
    let rows = format!(
        "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" outkey=\"{table_key}\" expanded=\"1\"{header}>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"1\" datatype=\"long\"/></expression></function></expression></condition>\n\
         {cells}\
         \t\t\t\t\t\t\t\t\t\t\t</entry>\n"
    );
    worksheets.push_str(&worksheet_body(table_sheet, &range, &rows, None));
    render_component(args, worksheets)
}

fn render_grid(args: &RenderArgs<'_>, layout: &XlsxGridLayout) -> Result<String, MfdError> {
    validate_grid_names(layout)?;
    let header = direct_child(args.schema, &layout.header_value_field)
        .ok_or_else(|| unsupported("the XLSX grid header-value field is missing"))?;
    let header_ty = scalar_type(header, "XLSX grid header-value field")?;
    require_int_child(args.schema, &layout.header_position_field)?;
    let rows_schema = direct_child(args.schema, &layout.rows_field)
        .ok_or_else(|| unsupported("the XLSX grid row collection is missing"))?;
    if !rows_schema.repeating {
        return Err(unsupported(
            "the XLSX grid row collection must be repeating",
        ));
    }
    let cells_schema = direct_child(rows_schema, &layout.cells_field)
        .ok_or_else(|| unsupported("the XLSX grid cell collection is missing"))?;
    if !cells_schema.repeating {
        return Err(unsupported(
            "the XLSX grid cell collection must be repeating",
        ));
    }
    let value = direct_child(cells_schema, &layout.cell_value_field)
        .ok_or_else(|| unsupported("the XLSX grid cell-value field is missing"))?;
    let value_ty = scalar_type(value, "XLSX grid cell-value field")?;
    require_int_child(cells_schema, &layout.cell_position_field)?;

    let header_key = args.ports.required_key_for_abs(
        std::slice::from_ref(&layout.header_value_field),
        "XLSX grid header value",
    )?;
    let header_position_key = args.ports.required_key_for_abs(
        std::slice::from_ref(&layout.header_position_field),
        "XLSX grid header position",
    )?;
    let rows_key = args
        .ports
        .required_key_for_abs(std::slice::from_ref(&layout.rows_field), "XLSX grid rows")?;
    let value_path = vec![
        layout.rows_field.clone(),
        layout.cells_field.clone(),
        layout.cell_value_field.clone(),
    ];
    let value_key = args
        .ports
        .required_key_for_abs(&value_path, "XLSX grid cell value")?;
    let position_path = vec![
        layout.rows_field.clone(),
        layout.cells_field.clone(),
        layout.cell_position_field.clone(),
    ];
    let position_key = args
        .ports
        .required_key_for_abs(&position_path, "XLSX grid cell position")?;

    let mut ranges = format!(
        "\t\t\t\t\t\t\t\t\t\t\t<range id=\"1\" start=\"{}\" count=\"1\"/>\n\
         \t\t\t\t\t\t\t\t\t\t\t<range id=\"2\" start=\"{}\"/>\n",
        layout.header_row.get(),
        layout.data_start_row.get(),
    );
    let mut row_entries = format!(
        "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"1\" datatype=\"long\"/></expression></function></expression></condition>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" outkey=\"{header_key}\" annotation=\"{}\" datatype=\"{}\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"n\" type=\"attribute\" outkey=\"{header_position_key}\"/>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" outkey=\"{rows_key}\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"2\" datatype=\"long\"/></expression></function></expression></condition>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" outkey=\"{value_key}\" datatype=\"{}\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"n\" type=\"attribute\" outkey=\"{position_key}\"/>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t\t\t</entry>\n",
        xml_escape(&layout.header_value_field),
        type_name(header_ty),
        type_name(value_ty),
    );
    for (index, cell) in layout.fixed_cells.iter().enumerate() {
        let field_name = one_segment(&cell.path, "XLSX grid fixed-cell path")?;
        let field = schema_at(args.schema, &cell.path)
            .ok_or_else(|| unsupported("an XLSX grid fixed-cell field is missing"))?;
        let ty = scalar_type(field, "XLSX grid fixed-cell field")?;
        let key = args
            .ports
            .required_key_for_abs(&cell.path, "XLSX grid fixed cell")?;
        render_fixed_cell(
            index + 3,
            cell,
            field_name,
            ty,
            key,
            &mut ranges,
            &mut row_entries,
        );
    }
    render_component(
        args,
        worksheet(
            args.options
                .xlsx_grid
                .as_ref()
                .and_then(|grid| grid.sheet.as_deref()),
            &ranges,
            &row_entries,
        )?,
    )
}

fn render_component(args: &RenderArgs<'_>, worksheets: String) -> Result<String, MfdError> {
    let root_key = args
        .ports
        .required_key_for_abs(&[], "special XLSX source root")?;
    let instance = args
        .instance_path
        .map(|path| format!(" inputinstance=\"{}\"", xml_escape(path)))
        .unwrap_or_default();
    Ok(format!(
        "\t\t\t\t<component name=\"{}\" library=\"xlsx\" uid=\"{}\" kind=\"26\">\n\
         \t\t\t\t\t<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/><namespace uid=\"http://www.altova.com/mapforce\"/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" ns=\"1\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" ns=\"1\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<entry name=\"Workbook\" outkey=\"{root_key}\" expanded=\"1\">\n\
         {worksheets}\
         \t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<excel{instance}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
    ))
}

fn worksheet(sheet: Option<&str>, ranges: &str, rows: &str) -> Result<String, MfdError> {
    if sheet.is_some_and(str::is_empty) {
        return Err(unsupported("XLSX worksheet names cannot be empty"));
    }
    Ok(worksheet_body_optional(sheet, ranges, rows, None))
}

fn worksheet_body(sheet: &str, ranges: &str, rows: &str, key: Option<u32>) -> String {
    worksheet_body_optional(Some(sheet), ranges, rows, key)
}

fn worksheet_body_optional(
    sheet: Option<&str>,
    ranges: &str,
    rows: &str,
    key: Option<u32>,
) -> String {
    let key = key
        .map(|key| format!(" outkey=\"{key}\""))
        .unwrap_or_default();
    let condition = sheet
        .map(|sheet| {
            format!(
                "\n\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"equal-ignorecase\" library=\"xlsx\"><expression><attribute name=\"Name\"/></expression><expression><constant value=\"{}\"/></expression></function></expression></condition>",
                xml_escape(sheet)
            )
        })
        .unwrap_or_default();
    format!(
        "\t\t\t\t\t\t\t\t\t\t<entry name=\"Worksheet\"{key} expanded=\"1\">{condition}\n\
         \t\t\t\t\t\t\t\t\t\t\t<ranges>\n\
         {ranges}\
         \t\t\t\t\t\t\t\t\t\t\t</ranges>\n\
         {rows}\
         \t\t\t\t\t\t\t\t\t\t</entry>\n"
    )
}

fn render_fixed_cell(
    range_id: usize,
    cell: &XlsxFixedCell,
    field_name: &str,
    ty: ScalarType,
    key: u32,
    ranges: &mut String,
    rows: &mut String,
) {
    let _ = writeln!(
        ranges,
        "\t\t\t\t\t\t\t\t\t\t\t<range id=\"{range_id}\" start=\"{}\" count=\"1\"/>",
        cell.row.get()
    );
    let _ = writeln!(
        rows,
        "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"{range_id}\" datatype=\"long\"/></expression></function></expression></condition>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" outkey=\"{key}\" annotation=\"{}\" datatype=\"{}\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"equal\" library=\"core\"><expression><attribute name=\"n\"/></expression><expression><constant value=\"{}\" datatype=\"long\"/></expression></function></expression></condition>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t\t\t</entry>",
        xml_escape(field_name),
        type_name(ty),
        cell.column.get(),
    );
}

fn render_selected_cell(
    field_name: &str,
    ty: ScalarType,
    column: u32,
    key: u32,
    output: &mut String,
) {
    let _ = writeln!(
        output,
        "\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" outkey=\"{key}\" annotation=\"{}\" datatype=\"{}\">\n\
         \t\t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"equal\" library=\"core\"><expression><attribute name=\"n\"/></expression><expression><constant value=\"{column}\" datatype=\"long\"/></expression></function></expression></condition>\n\
         \t\t\t\t\t\t\t\t\t\t\t\t</entry>",
        xml_escape(field_name),
        type_name(ty),
    );
}

fn table_columns(
    field_count: usize,
    configured: &[mapping::XlsxColumn],
) -> Result<Vec<u32>, MfdError> {
    let columns: Vec<u32> = if configured.is_empty() {
        (1..=u32::try_from(field_count)
            .map_err(|_| unsupported("the composite XLSX table has too many fields"))?)
            .collect()
    } else {
        configured.iter().map(|column| column.get()).collect()
    };
    if columns.len() != field_count {
        return Err(unsupported(
            "the composite XLSX table column count does not match its scalar fields",
        ));
    }
    let unique: BTreeSet<_> = columns.iter().copied().collect();
    if unique.len() != columns.len() {
        return Err(unsupported(
            "the composite XLSX table columns must be unique",
        ));
    }
    Ok(columns)
}

fn direct_scalar_fields(schema: &SchemaNode) -> Option<&[SchemaNode]> {
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
    } = &schema.kind
    else {
        return None;
    };
    (schema.recursive_ref.is_none()
        && alternatives.is_empty()
        && dynamic.is_none()
        && children.iter().all(|child| {
            !child.repeating && !child.attribute && matches!(child.kind, SchemaKind::Scalar { .. })
        }))
    .then_some(children)
}

fn schema_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    path.iter()
        .try_fold(schema, |node, segment| node.child(segment))
}

fn direct_child<'a>(schema: &'a SchemaNode, name: &str) -> Option<&'a SchemaNode> {
    schema.child(name)
}

fn scalar_type(node: &SchemaNode, label: &str) -> Result<ScalarType, MfdError> {
    match node.kind {
        SchemaKind::Scalar { ty } if !node.repeating && !node.attribute => Ok(ty),
        _ => Err(unsupported(&format!("the {label} must be a scalar"))),
    }
}

fn require_int_child(schema: &SchemaNode, name: &str) -> Result<(), MfdError> {
    match direct_child(schema, name) {
        Some(SchemaNode {
            repeating: false,
            attribute: false,
            kind: SchemaKind::Scalar {
                ty: ScalarType::Int,
            },
            ..
        }) => Ok(()),
        _ => Err(unsupported(
            "XLSX grid physical-position fields must be non-repeating integers",
        )),
    }
}

fn one_segment<'a>(path: &'a [String], label: &str) -> Result<&'a str, MfdError> {
    match path {
        [name] if !name.is_empty() => Ok(name),
        _ => Err(unsupported(&format!(
            "the {label} must contain one segment"
        ))),
    }
}

fn required_matching_sheet<'a>(
    sheet: Option<&'a str>,
    field: &'a str,
) -> Result<&'a str, MfdError> {
    match sheet {
        Some(sheet) if sheet == field => Ok(sheet),
        _ => Err(unsupported(
            "composite XLSX worksheet names must match their top-level schema fields",
        )),
    }
}

fn validate_grid_names(layout: &XlsxGridLayout) -> Result<(), MfdError> {
    let names = [
        layout.header_value_field.as_str(),
        layout.header_position_field.as_str(),
        layout.rows_field.as_str(),
        layout.cells_field.as_str(),
        layout.cell_value_field.as_str(),
        layout.cell_position_field.as_str(),
    ];
    if names.iter().any(|name| name.is_empty()) {
        return Err(unsupported("XLSX grid field names cannot be empty"));
    }
    if layout.header_position_field != "HeaderColumn"
        || layout.rows_field != "Rows"
        || layout.cells_field != "Cells"
        || layout.cell_value_field != "value"
        || layout.cell_position_field != "CellColumn"
    {
        return Err(unsupported(
            "XLSX grid generated field names are not canonical MapForce names",
        ));
    }
    if layout.data_start_row.get() <= layout.header_row.get() {
        return Err(unsupported(
            "the XLSX grid data start row must follow its header row",
        ));
    }
    Ok(())
}

const fn type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}

fn unsupported(message: &str) -> MfdError {
    MfdError::Unsupported(message.to_string())
}
