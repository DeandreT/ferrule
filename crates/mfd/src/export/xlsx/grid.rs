//! Canonical .mfd worksheets for header-driven XLSX grids.

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{XlsxGridLayout, XlsxGridRegion};

use crate::MfdError;

use super::super::schema::xml_escape;
use super::{
    RenderArgs, direct_child, one_segment, render_component, render_fixed_cell, scalar_type,
    schema_at, type_name, unsupported, worksheet, worksheet_body,
};

/// Renders a single-grid source, whose header records are the root.
pub(super) fn render_source(
    args: &RenderArgs<'_>,
    layout: &XlsxGridLayout,
) -> Result<String, MfdError> {
    let (ranges, rows) = grid_rows(args, args.schema, &[], layout)?;
    render_component(args, worksheet(layout.sheet.as_deref(), &ranges, &rows)?)
}

/// Renders the worksheet of one composite grid region. The caller checks
/// that its one-segment path names its worksheet.
pub(super) fn render_region(
    args: &RenderArgs<'_>,
    region: &XlsxGridRegion,
) -> Result<String, MfdError> {
    let sheet = one_segment(&region.path, "composite XLSX grid path")?;
    let record = schema_at(args.schema, &region.path).ok_or_else(|| {
        unsupported("a composite XLSX grid path is missing from the source schema")
    })?;
    if !record.repeating || !matches!(record.kind, SchemaKind::Group { .. }) {
        return Err(unsupported(
            "a composite XLSX grid path must select a repeating group",
        ));
    }
    let group_key = args
        .ports
        .required_key_for_abs(&region.path, "composite XLSX grid")?;
    let (ranges, rows) = grid_rows(args, record, &region.path, &region.layout)?;
    Ok(worksheet_body(sheet, &ranges, &rows, Some(group_key)))
}

/// The worksheet ranges and rows of one grid whose header record is `record`
/// at the absolute path `prefix`.
fn grid_rows(
    args: &RenderArgs<'_>,
    record: &SchemaNode,
    prefix: &[String],
    layout: &XlsxGridLayout,
) -> Result<(String, String), MfdError> {
    validate_grid_names(layout)?;
    let header = direct_child(record, &layout.header_value_field)
        .ok_or_else(|| unsupported("the XLSX grid header-value field is missing"))?;
    let header_ty = scalar_type(header, "XLSX grid header-value field")?;
    require_int_child(record, &layout.header_position_field)?;
    let rows_schema = direct_child(record, &layout.rows_field)
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

    let path = |segments: &[&String]| {
        prefix
            .iter()
            .chain(segments.iter().copied())
            .cloned()
            .collect::<Vec<_>>()
    };
    let key =
        |segments: &[&String], label: &str| args.ports.required_key_for_abs(&path(segments), label);
    let header_key = key(&[&layout.header_value_field], "XLSX grid header value")?;
    let header_position_key = key(
        &[&layout.header_position_field],
        "XLSX grid header position",
    )?;
    let rows_key = key(&[&layout.rows_field], "XLSX grid rows")?;
    let value_key = key(
        &[
            &layout.rows_field,
            &layout.cells_field,
            &layout.cell_value_field,
        ],
        "XLSX grid cell value",
    )?;
    let position_key = key(
        &[
            &layout.rows_field,
            &layout.cells_field,
            &layout.cell_position_field,
        ],
        "XLSX grid cell position",
    )?;

    let mut ranges = format!(
        "\t\t\t\t\t\t\t\t\t\t\t<range id=\"1\" start=\"{}\" count=\"1\"/>\n\
         \t\t\t\t\t\t\t\t\t\t\t<range id=\"2\" start=\"{}\"/>\n",
        layout.header_row.get(),
        layout.data_start_row.get(),
    );
    let mut rows = format!(
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
        let field = schema_at(record, &cell.path)
            .ok_or_else(|| unsupported("an XLSX grid fixed-cell field is missing"))?;
        let ty = scalar_type(field, "XLSX grid fixed-cell field")?;
        let mut absolute = prefix.to_vec();
        absolute.extend(cell.path.iter().cloned());
        let key = args
            .ports
            .required_key_for_abs(&absolute, "XLSX grid fixed cell")?;
        render_fixed_cell(index + 3, cell, field_name, ty, key, &mut ranges, &mut rows);
    }
    Ok((ranges, rows))
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
            "XLSX grid generated field names are not canonical .mfd names",
        ));
    }
    if layout.data_start_row.get() <= layout.header_row.get() {
        return Err(unsupported(
            "the XLSX grid data start row must follow its header row",
        ));
    }
    Ok(())
}
