use std::fmt::Write as _;

use ir::SchemaNode;
use mapping::{FormatOptions, Project, WsdlMessageRole};

use crate::MfdError;

use super::concatenation::TargetBranches;
use super::schema::{PortTree, RenderedSchemaComponent, Side, xml_escape};

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) side: Side,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) target_branches: Option<&'a TargetBranches>,
    pub(super) component_uid: u32,
    pub(super) force_root_port: bool,
    pub(super) default_output: bool,
}

pub(super) fn render(arguments: RenderArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    let options = arguments.options.wsdl.as_ref().ok_or_else(|| {
        MfdError::Unsupported("internal WSDL renderer has no message metadata".to_string())
    })?;
    let attr = match arguments.side {
        Side::Source => "outkey",
        Side::Target => "inpkey",
    };
    let properties = if arguments.default_output {
        "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t"
    } else {
        ""
    };
    let view = match arguments.side {
        Side::Source => "<view rbx=\"300\" rby=\"400\"/>",
        Side::Target => "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
    };
    let namespace = expanded_namespace(options.operation())
        .or_else(|| expanded_namespace(options.service()))
        .map(|namespace| format!("<namespace uid=\"{}\"/>", xml_escape(namespace)))
        .unwrap_or_else(|| "<namespace/>".to_string());
    let role = match options.role() {
        WsdlMessageRole::Request => arguments.instance_path.map_or_else(
            || "<wsdl/>".to_string(),
            |path| {
                format!(
                    "<wsdl previewRequestInstanceFile=\"{}\"/>",
                    xml_escape(path)
                )
            },
        ),
        WsdlMessageRole::Response => "<wsdl kind=\"output\"/>".to_string(),
        WsdlMessageRole::Fault => format!(
            "<wsdl kind=\"fault\" faultName=\"{}\"/>",
            xml_escape(options.fault_name().unwrap_or_default())
        ),
    };

    let mut xml = String::new();
    let _ = write!(
        xml,
        "\t\t\t\t<component name=\"wsdl\" library=\"wsdl\" uid=\"{}\" kind=\"17\">\n\
         \t\t\t\t\t{properties}{view}\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces>{namespace}</namespaces></header>\n\
         {}\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t{role}\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        arguments.component_uid,
        arguments.ports.entries_xml(
            arguments.schema,
            attr,
            7,
            arguments.force_root_port,
            None,
            arguments.target_branches,
        ),
    );
    Ok(RenderedSchemaComponent {
        xml,
        siblings: Vec::new(),
    })
}

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    let sources = std::iter::once(("source", &project.source_options))
        .chain(
            project
                .extra_sources
                .iter()
                .map(|source| ("additional source", &source.options)),
        )
        .collect::<Vec<_>>();
    let targets = std::iter::once((
        "target",
        &project.target_options,
        project.target_path.as_deref(),
    ))
    .chain(
        project
            .extra_targets
            .iter()
            .map(|target| ("additional target", &target.options, target.path.as_deref())),
    )
    .collect::<Vec<_>>();
    let mut messages = sources
        .iter()
        .filter_map(|(side, options)| options.wsdl.as_ref().map(|wsdl| (*side, wsdl)))
        .chain(
            targets
                .iter()
                .filter_map(|(side, options, _)| options.wsdl.as_ref().map(|wsdl| (*side, wsdl))),
        );
    let Some((_, contract)) = messages.next() else {
        return Ok(());
    };
    if messages.any(|(_, message)| !contract.same_contract(message)) {
        return Err(MfdError::Unsupported(
            "all WSDL message boundaries must belong to one service operation".to_string(),
        ));
    }
    let request_count = sources
        .iter()
        .filter(|(_, options)| {
            options
                .wsdl
                .as_ref()
                .is_some_and(|wsdl| wsdl.role() == WsdlMessageRole::Request)
        })
        .count();
    if request_count != 1 {
        return Err(MfdError::Unsupported(
            "WSDL export requires exactly one request message source".to_string(),
        ));
    }
    for (side, options) in sources {
        if let Some(wsdl) = &options.wsdl
            && wsdl.role() != WsdlMessageRole::Request
        {
            return Err(MfdError::Unsupported(format!(
                "the {side} WSDL boundary must be a request message"
            )));
        }
        validate_format_identity(side, options)?;
    }
    for (side, options, path) in targets {
        if let Some(wsdl) = &options.wsdl {
            if wsdl.role() == WsdlMessageRole::Request {
                return Err(MfdError::Unsupported(format!(
                    "the {side} WSDL boundary must be a response or fault message"
                )));
            }
            if path.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "the {side} WSDL message cannot carry an output instance path"
                )));
            }
        }
        validate_format_identity(side, options)?;
    }
    Ok(())
}

fn validate_format_identity(side: &str, options: &FormatOptions) -> Result<(), MfdError> {
    if options.wsdl.is_none() {
        return Ok(());
    }
    let conflict = options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.local_xml_file_set
        || options.json_document
        || options.json_lines
        || options.tabular_kind.is_some()
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
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
        || options.xlsx_hierarchical.is_some();
    if conflict {
        return Err(MfdError::Unsupported(format!(
            "the {side} WSDL message conflicts with another format identity"
        )));
    }
    Ok(())
}

pub(super) fn mapping_properties(project: &Project) -> Option<String> {
    let options = std::iter::once(&project.source_options)
        .chain(project.extra_sources.iter().map(|source| &source.options))
        .chain(std::iter::once(&project.target_options))
        .chain(project.extra_targets.iter().map(|target| &target.options))
        .find_map(|options| options.wsdl.as_ref())?;
    Some(format!(
        "<properties SelectedLanguage=\"builtin\" WSDLFile=\"{}\" WSDLService=\"{}\" WSDLPort=\"{}\" WSDLOperation=\"{}\"/>",
        xml_escape(options.contract_file()),
        xml_escape(options.service()),
        xml_escape(options.port()),
        xml_escape(options.operation()),
    ))
}

fn expanded_namespace(name: &str) -> Option<&str> {
    let close = name.strip_prefix('{')?.find('}')?;
    let namespace = &name[1..=close];
    (!namespace.is_empty()).then_some(namespace)
}
