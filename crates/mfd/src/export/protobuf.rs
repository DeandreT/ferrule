use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use ir::SchemaNode;
use mapping::FormatOptions;

use crate::MfdError;

use super::concatenation::TargetBranches;
use super::schema::{GeneratedSibling, PortTree, RenderedSchemaComponent, Side, xml_escape};

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) side: Side,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) mfd_path: &'a Path,
    pub(super) target_branches: Option<&'a TargetBranches>,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: &'a str,
    pub(super) default_output: bool,
}

pub(super) fn validate_target(
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<(), MfdError> {
    let Some(protobuf) = options.protobuf.as_ref() else {
        return Ok(());
    };
    if has_conflicting_options(options) {
        return Err(unsupported(
            "a protobuf target cannot combine protobuf metadata with another format's options",
        ));
    }
    let layout = format_protobuf::Layout::parse(&protobuf.schema).map_err(|error| {
        unsupported(format!("the embedded protobuf schema is invalid: {error}"))
    })?;
    let root = layout
        .resolve_message(&protobuf.root_message)
        .map_err(|error| {
            unsupported(format!(
                "the embedded protobuf root message cannot be resolved: {error}"
            ))
        })?;
    let root_name = layout
        .message(root)
        .map(|message| message.full_name())
        .ok_or_else(|| unsupported("the resolved protobuf root message is missing"))?;
    let projected = format_protobuf::to_ir_schema(&layout, root_name).map_err(|error| {
        unsupported(format!(
            "the embedded protobuf root cannot be represented as a mapping schema: {error}"
        ))
    })?;
    if projected != *schema {
        return Err(unsupported(
            "the target schema does not exactly match its embedded protobuf root message",
        ));
    }
    Ok(())
}

pub(super) fn render(args: RenderArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    if args.side != Side::Target {
        return Err(unsupported(
            "protobuf source component export is not supported; protobuf is an output-only format",
        ));
    }
    validate_target(args.schema, args.options)?;
    let protobuf = args.options.protobuf.as_ref().ok_or_else(|| {
        unsupported("internal protobuf target is missing its embedded schema metadata")
    })?;
    let layout = format_protobuf::Layout::parse(&protobuf.schema).map_err(|error| {
        unsupported(format!("the embedded protobuf schema is invalid: {error}"))
    })?;
    let root = layout
        .resolve_message(&protobuf.root_message)
        .map_err(|error| {
            unsupported(format!(
                "the embedded protobuf root message cannot be resolved: {error}"
            ))
        })?;
    let root_name = layout
        .message(root)
        .map(|message| message.full_name())
        .ok_or_else(|| unsupported("the resolved protobuf root message is missing"))?;

    let stem = args
        .mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let schema_file = format!("{stem}-{}.proto", args.sibling_suffix);
    let schema_path = args
        .mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&schema_file);
    let root_key = args
        .ports
        .required_key_for_abs(&[], "protobuf document root")?;
    let properties = if args.default_output {
        "<properties XSLTDefaultOutput=\"1\"/>"
    } else {
        "<properties/>"
    };
    let instance = args
        .instance_path
        .map(|path| format!(" outputinstance=\"{}\"", xml_escape(path)))
        .unwrap_or_default();
    let entries =
        args.ports
            .entries_xml(args.schema, "inpkey", 10, false, None, args.target_branches);

    let mut namespaces = BTreeSet::new();
    namespaces.extend(layout.messages().iter().map(|message| message.full_name()));
    namespaces.extend(
        layout
            .enums()
            .iter()
            .map(|enumeration| enumeration.full_name()),
    );
    let mut namespace_xml = String::new();
    namespace_xml.push_str("\t\t\t\t\t\t\t\t<namespace/>\n");
    for namespace in namespaces {
        let _ = writeln!(
            namespace_xml,
            "\t\t\t\t\t\t\t\t<namespace uid=\"{}\"/>",
            xml_escape(namespace)
        );
    }
    namespace_xml.push_str("\t\t\t\t\t\t\t\t<namespace uid=\"http://www.altova.com/mapforce\"/>\n");

    let mut xml = String::new();
    let _ = write!(
        xml,
        "\t\t\t\t<component name=\"{}\" library=\"binary\" uid=\"{}\" kind=\"33\">\n\
         \t\t\t\t\t{properties}\n\
         \t\t\t\t\t<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces>\n\
         {namespace_xml}\
         \t\t\t\t\t\t\t</namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" inpkey=\"{root_key}\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-protobuf\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<document schemafile=\"{}\" root=\"{}\"/>\n\
         {entries}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<binary{instance}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
        xml_escape(&schema_file),
        xml_escape(&expanded_name(root_name)),
    );
    Ok(RenderedSchemaComponent {
        xml,
        siblings: vec![GeneratedSibling {
            path: schema_path,
            contents: protobuf.schema.clone(),
        }],
    })
}

fn expanded_name(full_name: &str) -> String {
    full_name.rsplit_once('.').map_or_else(
        || format!("{{}}{full_name}"),
        |(namespace, local)| format!("{{{namespace}}}{local}"),
    )
}

fn has_conflicting_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.xbrl.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn unsupported(message: impl Into<String>) -> MfdError {
    MfdError::Unsupported(message.into())
}
