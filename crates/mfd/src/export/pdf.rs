use std::fmt::Write as _;
use std::path::Path;

use ir::SchemaNode;
use mapping::{FormatOptions, PdfLayout};

use crate::MfdError;

use super::schema::{GeneratedSibling, PortTree, RenderedSchemaComponent, Side, xml_escape};

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) side: Side,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) mfd_path: &'a Path,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: &'a str,
    pub(super) force_root_port: bool,
}

pub(super) fn validate_side(
    schema: &SchemaNode,
    options: &FormatOptions,
    side: Side,
    side_name: &str,
) -> Result<(), MfdError> {
    let Some(layout) = options.pdf.as_ref() else {
        return Ok(());
    };
    if side != Side::Source {
        return Err(unsupported(format!(
            "the {side_name} PDF boundary is input-only"
        )));
    }
    if has_conflicting_options(options) {
        return Err(unsupported(format!(
            "the {side_name} PDF boundary conflicts with another format's options"
        )));
    }
    if layout.schema() != *schema {
        return Err(unsupported(format!(
            "the {side_name} schema does not exactly match its embedded PDF layout"
        )));
    }
    Ok(())
}

pub(super) fn render(args: RenderArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    validate_side(args.schema, args.options, args.side, "mapping side")?;
    let layout = args.options.pdf.as_ref().ok_or_else(|| {
        unsupported("internal PDF export is missing its visual extraction layout")
    })?;
    let instance_path = args
        .instance_path
        .ok_or_else(|| unsupported("a PDF source requires its input instance path"))?;
    let stem = args
        .mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let template_file = format!("{stem}-{}.pxt", args.sibling_suffix);
    let template_path = args
        .mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&template_file);
    let template = canonical_template(layout)?;
    let entries =
        args.ports
            .entries_xml(args.schema, "outkey", 10, args.force_root_port, None, None);

    let mut xml = String::new();
    let _ = write!(
        xml,
        "\t\t\t\t<component name=\"{}\" library=\"pdf\" uid=\"{}\" kind=\"34\">\n\
         \t\t\t\t\t<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<file role=\"inputinstance\" name=\"{}\"/>\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-pdf\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<document schemafile=\"{}\" root=\"{}\"/>\n\
         {entries}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
        xml_escape(instance_path),
        xml_escape(&template_file),
        xml_escape(layout.root_name()),
    );
    Ok(RenderedSchemaComponent {
        xml,
        siblings: vec![GeneratedSibling {
            path: template_path,
            contents: template,
        }],
    })
}

fn canonical_template(layout: &PdfLayout) -> Result<String, MfdError> {
    let encoded = serde_json::to_string(layout).map_err(|error| {
        unsupported(format!(
            "could not serialize the retained PDF layout ({error})"
        ))
    })?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document>\n\
         \t<FerruleLayout version=\"1\">{}</FerruleLayout>\n\
         </Document>\n",
        xml_escape(&encoded)
    ))
}

fn has_conflicting_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn unsupported(message: impl Into<String>) -> MfdError {
    MfdError::Unsupported(message.into())
}
