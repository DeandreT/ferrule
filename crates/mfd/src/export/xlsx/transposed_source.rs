//! Canonical .mfd components for transposed XLSX sources.
//!
//! The reference application reads each selected row as its own sequence of
//! non-empty cells; it does not line rows up by column. The exported design
//! therefore iterates records over the first (driver) row's cells and reads
//! every later row through a filter that keeps the cell whose `n` equals the
//! driver cell's `n`. Import recognizes that filter and restores the plain
//! row field.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use crate::MfdError;

use super::super::schema::{KeyAlloc, PortTree, SideFormat, xml_escape};
use super::super::source::SourceExports;
use super::{RenderArgs, direct_scalar_fields, unsupported};

/// Whether `options` select the transposed layout rather than another
/// retained XLSX source layout.
pub(in crate::export) fn is_transposed_source(options: &FormatOptions) -> bool {
    !options.xlsx_rows.is_empty()
        && options.xlsx_grid.is_none()
        && options.xlsx_composite.is_none()
        && options.xlsx_worksheet_set.is_none()
}

struct Layout<'a> {
    /// One scalar field per selected row, in row order.
    rows: &'a [SchemaNode],
    /// The schema ends with the integer `n` column field.
    has_index: bool,
}

fn layout<'a>(schema: &'a SchemaNode, options: &FormatOptions) -> Result<Layout<'a>, MfdError> {
    let fields = direct_scalar_fields(schema).ok_or_else(|| {
        unsupported("a transposed XLSX source must have a flat scalar root schema")
    })?;
    let row_count = options.xlsx_rows.len();
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
    if options
        .xlsx_rows
        .iter()
        .any(|row| *row == 0 || *row > 1_048_576 || !seen.insert(*row))
    {
        return Err(unsupported(
            "transposed XLSX source rows must be unique one-based Excel row numbers",
        ));
    }
    if let Some(field) = fields[..row_count].iter().find(|field| {
        matches!(
            field.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        )
    }) {
        return Err(unsupported(&format!(
            "transposed XLSX source integer row `{}` has no verified exact native conversion; reading it as double would lose i64 precision",
            field.name
        )));
    }
    Ok(Layout {
        rows: &fields[..row_count],
        has_index,
    })
}

/// A native row sequence omits empty cells and includes values in columns
/// without a driver. Only the driver row has the record sequence's cardinality.
/// Reject unproved reductions independently of the design-time workbook: it
/// may be replaced before execution.
pub(in crate::export) fn validate_transposed_aggregate(
    sources: &SourceExports<'_>,
    port: u32,
    computed: bool,
) -> Result<(), MfdError> {
    for source in sources
        .iter()
        .filter(|source| source.format == SideFormat::Xlsx && is_transposed_source(source.options))
    {
        let Some(path) = source.ports.path_for_key(port) else {
            continue;
        };
        let layout = layout(source.schema, source.options)?;
        if computed {
            return Err(unsupported(
                "computed aggregates over transposed XLSX records have no verified native record-aligned representation",
            ));
        }
        if matches!(path, [name] if layout.rows.iter().skip(1).any(|field| field.name == *name)) {
            return Err(unsupported(
                "aggregates over later transposed XLSX rows cannot preserve driver-column membership and missing-cell positions",
            ));
        }
    }
    Ok(())
}

/// Allocates one `Cell/@n` output per selected row. The driver row reuses the
/// schema's `n` port when the schema has one.
pub(in crate::export) fn allocate_source_row_index(
    schema: &SchemaNode,
    options: &FormatOptions,
    ports: &mut PortTree,
    keys: &mut KeyAlloc,
) {
    // An invalid layout is reported when the component renders.
    let Ok(layout) = layout(schema, options) else {
        return;
    };
    let driver = layout
        .has_index
        .then(|| ports.key_for_abs(&["n".to_string()]))
        .flatten();
    let index = (0..layout.rows.len())
        .map(|row| match (row, driver) {
            (0, Some(key)) => key,
            _ => keys.next(),
        })
        .collect();
    ports.set_transposed_row_index(index);
}

pub(in crate::export) struct SourceWiring<'a> {
    pub(in crate::export) schema: &'a SchemaNode,
    pub(in crate::export) options: &'a FormatOptions,
    pub(in crate::export) ports: &'a PortTree,
    pub(in crate::export) keys: &'a mut KeyAlloc,
    pub(in crate::export) uid: &'a mut u32,
    pub(in crate::export) components: &'a mut String,
    pub(in crate::export) edges: &'a mut Vec<(u32, u32)>,
    pub(in crate::export) structural_edges: &'a mut BTreeSet<(u32, u32)>,
    /// Safe driver-row aggregate inputs read the complete row sequence.
    pub(in crate::export) whole_sequence_inputs: &'a BTreeSet<u32>,
}

/// Moves record iteration from the unrendered root port to the driver row's
/// Cell, and reads every connected later row through an `n`-matching filter.
/// Later-row aggregate consumers have already been rejected.
pub(in crate::export) fn connect_transposed_source(
    wiring: SourceWiring<'_>,
) -> Result<(), MfdError> {
    let SourceWiring {
        schema,
        options,
        ports,
        keys,
        uid,
        components,
        edges,
        structural_edges,
        whole_sequence_inputs,
    } = wiring;
    let layout = layout(schema, options)?;
    let index = row_index(ports, &layout)?;
    let cells = layout
        .rows
        .iter()
        .map(|field| {
            ports.required_key_for_abs(std::slice::from_ref(&field.name), "transposed XLSX field")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let Some((&driver, later)) = cells.split_first() else {
        return Ok(());
    };
    if let Some(root) = ports.key_for_abs(&[]) {
        reroute(edges, structural_edges, root, driver, |_| true);
    }
    let per_record = |to: u32| !whole_sequence_inputs.contains(&to);
    for (&cell, &row_n) in later.iter().zip(&index[1..]) {
        if !edges
            .iter()
            .any(|&(from, to)| from == cell && per_record(to))
        {
            continue;
        }
        let [nodes, predicate, aligned, left, right, matched] =
            std::array::from_fn(|_| keys.next());
        reroute(edges, structural_edges, cell, aligned, per_record);
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{nodes}\"/><datapoint pos=\"1\" key=\"{predicate}\"/></sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{aligned}\"/><datapoint/></targets>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n"
        );
        *uid += 1;
        let _ = write!(
            components,
            "\t\t\t\t<component name=\"equal\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
             \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{left}\"/><datapoint pos=\"1\" key=\"{right}\"/></sources>\n\
             \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{matched}\"/></targets>\n\
             \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
             \t\t\t\t</component>\n"
        );
        edges.extend([
            (cell, nodes),
            (row_n, left),
            (index[0], right),
            (matched, predicate),
        ]);
    }
    Ok(())
}

fn row_index<'a>(ports: &'a PortTree, layout: &Layout<'_>) -> Result<&'a [u32], MfdError> {
    let index = ports.transposed_row_index();
    if index.len() != layout.rows.len() {
        return Err(unsupported(
            "internal transposed XLSX row-index ports were not allocated for every row",
        ));
    }
    Ok(index)
}

/// Moves the connections from `from` into inputs `keep` admits onto `to`.
fn reroute(
    edges: &mut [(u32, u32)],
    structural_edges: &mut BTreeSet<(u32, u32)>,
    from: u32,
    to: u32,
    keep: impl Fn(u32) -> bool,
) {
    let moves = |&(edge_from, edge_to): &(u32, u32)| edge_from == from && keep(edge_to);
    for edge in edges.iter_mut().filter(|edge| moves(edge)) {
        edge.0 = to;
    }
    *structural_edges = std::mem::take(structural_edges)
        .into_iter()
        .map(|edge| if moves(&edge) { (to, edge.1) } else { edge })
        .collect();
}

/// Renders a retained transposed XLSX source in the shape the reference
/// application opens: the generic selection Worksheet and Row entries, then
/// one single-row range per field. The workbook has no record port; records
/// follow the driver row's Cell.
pub(super) fn render_source(
    args: &RenderArgs<'_>,
    used_ports: &BTreeSet<u32>,
) -> Result<String, MfdError> {
    let layout = layout(args.schema, args.options)?;
    let index = row_index(args.ports, &layout)?;
    let mut ranges = String::new();
    let mut rows = String::new();
    for (position, (field, physical_row)) in
        layout.rows.iter().zip(&args.options.xlsx_rows).enumerate()
    {
        let range_id = position + 1;
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
            "\t\t\t\t\t\t\t\t\t\t\t\t<range id=\"{range_id}\" start=\"{physical_row}\" count=\"1\"/>"
        );
        let index_key = index[position];
        let index_entry = if position == 0 && layout.has_index {
            format!(
                "\t\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"n\" type=\"attribute\" outkey=\"{index_key}\"/>\n"
            )
        } else if used_ports.contains(&index_key) {
            // Only aligns this row with the driver row; not the schema's `n`.
            format!(
                "\t\t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"n\" type=\"attribute\" outkey=\"{index_key}\" ferrule-row-alignment=\"1\"/>\n"
            )
        } else {
            String::new()
        };
        let _ = write!(
            rows,
            "\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Row\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"is-range-id\"><expression><constant value=\"{range_id}\" datatype=\"long\"/></expression></function></expression></condition>\n\
             \t\t\t\t\t\t\t\t\t\t\t\t<entry name=\"Cell\" outkey=\"{field_key}\" annotation=\"{}\" {} expanded=\"1\">\n\
             {index_entry}\
             \t\t\t\t\t\t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t\t\t\t\t\t</entry>\n",
            xml_escape(&field.name),
            source_datatype(ty)?,
        );
    }

    if args
        .options
        .xlsx_sheet
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err(unsupported("XLSX worksheet names cannot be empty"));
    }
    let condition = args
        .options
        .xlsx_sheet
        .as_deref()
        .map(|sheet| {
            format!(
                "\t\t\t\t\t\t\t\t\t\t\t<condition><expression><function name=\"equal-ignorecase\" library=\"xlsx\"><expression><attribute name=\"Name\"/></expression><expression><constant value=\"{}\"/></expression></function></expression></condition>\n",
                xml_escape(sheet)
            )
        })
        .unwrap_or_default();
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
         \t\t\t\t\t\t<excel{instance}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
    ))
}

/// Integer cell reads must not pass through a lossy double conversion.
fn source_datatype(ty: ScalarType) -> Result<&'static str, MfdError> {
    match ty {
        ScalarType::String => Ok("datatype=\"string\""),
        ScalarType::Int => Err(unsupported(
            "transposed XLSX source integer rows have no verified exact native conversion",
        )),
        ScalarType::Float => Ok("datatype=\"double\""),
        ScalarType::Bool => Ok("datatype=\"boolean\""),
    }
}
