//! Canonical .mfd components for transposed XLSX sources and targets.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind};

use crate::MfdError;

use super::super::schema::{PortTree, xml_escape};
use super::{
    RenderArgs, direct_scalar_fields, render_component, scalar_type, type_name, unsupported,
    worksheet,
};

/// Renders a retained transposed XLSX source.
pub(super) fn render_source(args: &RenderArgs<'_>) -> Result<String, MfdError> {
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

/// Renders a new-workbook transposed XLSX target in the shape the reference
/// application saves: one open Cell sequence per single-row range, each with
/// its own `n` input when the schema positions records. Its record iteration
/// follows the Cell feeds, so the component has no record port.
pub(in crate::export) fn render_transposed_target(
    args: RenderArgs<'_>,
    default_output: bool,
) -> Result<String, MfdError> {
    let options = args.options;
    if options.xlsx_update_existing {
        return Err(unsupported(
            "a transposed XLSX target cannot update an existing workbook",
        ));
    }
    if !options.xlsx_headers.is_empty()
        || options.has_header_row == Some(true)
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
    {
        return Err(unsupported(
            "a transposed XLSX target cannot use flat header, start-row, or column options",
        ));
    }
    let fields = direct_scalar_fields(args.schema)
        .filter(|_| !args.schema.repeating)
        .ok_or_else(|| {
            unsupported("a transposed XLSX target must have a flat scalar root schema")
        })?;
    let (positions, data): (Vec<_>, Vec<_>) = fields.iter().partition(|field| field.name == "n");
    let position_key = match positions.as_slice() {
        [] => None,
        [field]
            if matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::Int
                }
            ) =>
        {
            Some(
                args.ports
                    .required_key_for_abs(&["n".to_string()], "transposed XLSX column index")?,
            )
        }
        _ => {
            return Err(unsupported(
                "a transposed XLSX target's `n` field must be one integer column position",
            ));
        }
    };
    if data.len() != options.xlsx_rows.len() || data.is_empty() {
        return Err(unsupported(
            "a transposed XLSX target needs one scalar field per configured row",
        ));
    }
    let mut seen = BTreeSet::new();
    if options
        .xlsx_rows
        .iter()
        .any(|row| *row == 0 || *row > 1_048_576 || !seen.insert(*row))
    {
        return Err(unsupported(
            "transposed XLSX target rows must be unique one-based Excel row numbers",
        ));
    }
    let position_keys = position_key.map(|key| {
        std::iter::once(key)
            .chain(args.ports.fanout_keys(key).iter().copied())
            .collect::<Vec<_>>()
    });
    if position_keys
        .as_ref()
        .is_some_and(|keys| keys.len() != data.len())
    {
        return Err(unsupported(
            "internal transposed XLSX column-index ports were not allocated for every row",
        ));
    }

    let mut ranges = String::new();
    let mut rows = String::new();
    for (index, (field, physical_row)) in data.iter().zip(&options.xlsx_rows).enumerate() {
        let range_id = index + 1;
        let key = args
            .ports
            .required_key_for_abs(std::slice::from_ref(&field.name), "transposed XLSX field")?;
        let ty = scalar_type(field, "transposed XLSX target field")?;
        let _ = writeln!(
            ranges,
            "\t\t\t\t\t\t\t\t\t\t\t\t<range id=\"{range_id}\" start=\"{physical_row}\" count=\"1\"/>"
        );
        let index_entry = position_keys
            .as_ref()
            .map(|keys| {
                format!(
                    "\t\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"n\" type=\"attribute\" inpkey=\"{}\"/>\n",
                    keys[index]
                )
            })
            .unwrap_or_default();
        let _ = write!(
            rows,
            "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"{range_id}\" datatype=\"long\"/></expression></function></expression></condition>\n\
             \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" inpkey=\"{key}\" expanded=\"1\" annotation=\"{}\" datatype=\"{}\">\n\
             {index_entry}\
             \t\t\t\t\t\t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t\t\t\t\t\t</entry>\n",
            xml_escape(&field.name),
            target_type_name(ty),
        );
    }

    if options.xlsx_sheet.as_deref().is_some_and(str::is_empty) {
        return Err(unsupported("XLSX worksheet names cannot be empty"));
    }
    let condition = options
        .xlsx_sheet
        .as_deref()
        .map(|sheet| {
            format!(
                "\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"equal-ignorecase\" library=\"xlsx\"><expression><attribute name=\"Name\"/></expression><expression><constant value=\"{}\"/></expression></function></expression></condition>\n",
                xml_escape(sheet)
            )
        })
        .unwrap_or_default();
    let properties = if default_output {
        "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t"
    } else {
        ""
    };
    let instance = args
        .instance_path
        .map(|path| format!(" outputinstance=\"{}\"", xml_escape(path)))
        .unwrap_or_default();
    Ok(format!(
        "\t\t\t\t<component name=\"{}\" library=\"xlsx\" uid=\"{}\" kind=\"26\">\n\
         \t\t\t\t\t{properties}<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root scrollposition=\"1\">\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/><namespace uid=\"http://www.altova.com/mapforce\"/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" ns=\"1\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" ns=\"1\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<entry name=\"Workbook\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t<entry name=\"Worksheet\" expanded=\"1\" displayselectionmode=\"selection\">\n\
         \t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\"><entry name=\"Cell\" datatype=\"string\"/></entry>\n\
         \t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t\t<entry name=\"Worksheet\" expanded=\"1\">\n\
         {condition}\
         \t\t\t\t\t\t\t\t\t\t\t<ranges>\n\
         {ranges}\
         \t\t\t\t\t\t\t\t\t\t\t</ranges>\n\
         \t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\" displayselectionmode=\"selection\"><entry name=\"Cell\" datatype=\"string\"/></entry>\n\
         {rows}\
         \t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<wsdl/>\n\
         \t\t\t\t\t\t<excel{instance}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
    ))
}

/// Cell datatypes the reference application keeps on a saved target.
const fn target_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "double",
        ScalarType::Bool => "boolean",
    }
}

/// Fans each `n` connection out to every row's `n` input and drops the
/// record-iteration edge into the unrendered root port: the reference
/// application derives record iteration from the Cell feeds.
pub(in crate::export) fn connect_transposed_target(
    ports: &PortTree,
    edges: &mut Vec<(u32, u32)>,
    structural_edges: &mut BTreeSet<(u32, u32)>,
) {
    if let Some(root) = ports.key_for_abs(&[]) {
        edges.retain(|(_, to)| *to != root);
        structural_edges.retain(|(_, to)| *to != root);
    }
    let Some(position) = ports.key_for_abs(&["n".to_string()]) else {
        return;
    };
    let extra = ports.fanout_keys(position);
    let fanned = edges
        .iter()
        .filter(|(_, to)| *to == position)
        .flat_map(|(from, _)| extra.iter().map(move |to| (*from, *to)))
        .collect::<Vec<_>>();
    edges.extend(fanned);
}
