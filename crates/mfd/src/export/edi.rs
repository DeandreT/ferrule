use std::collections::BTreeSet;
use std::fmt::Write as _;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions};

use crate::MfdError;

use super::schema::{PortTree, RenderedSchemaComponent, Side, xml_escape};

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) side: Side,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) force_root_port: bool,
    pub(super) default_output: bool,
}

pub(super) fn validate_side(
    schema: &SchemaNode,
    options: &FormatOptions,
    side_name: &str,
) -> Result<(), MfdError> {
    let Some(kind) = options.edi_kind else {
        return Ok(());
    };
    if options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json_lines
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
    {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} EDI boundary conflicts with another format's options"
        )));
    }
    match kind {
        EdiBoundaryKind::Idoc => {
            if options.idoc.is_none() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} IDoc boundary has no retained runtime layout"
                )));
            }
            if options.swift_mt.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} IDoc boundary also retains a SWIFT MT layout"
                )));
            }
        }
        EdiBoundaryKind::SwiftMt => {
            if options.swift_mt.is_none() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} SWIFT MT boundary has no retained runtime layout"
                )));
            }
            if options.idoc.is_some() {
                return Err(MfdError::Unsupported(format!(
                    "the {side_name} SWIFT MT boundary also retains an IDoc layout"
                )));
            }
        }
        EdiBoundaryKind::X12
        | EdiBoundaryKind::Edifact
        | EdiBoundaryKind::Hl7
        | EdiBoundaryKind::Tradacoms
            if options.idoc.is_some() || options.swift_mt.is_some() =>
        {
            return Err(MfdError::Unsupported(format!(
                "the {side_name} EDI boundary retains an incompatible IDoc or SWIFT layout"
            )));
        }
        _ => {}
    }
    if options.x12_separators.is_some() && kind != EdiBoundaryKind::X12 {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} non-X12 EDI boundary retains X12 separator metadata"
        )));
    }
    if options.x12_interchange_version.is_some() && kind != EdiBoundaryKind::X12 {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} non-X12 EDI boundary retains an X12 interchange version"
        )));
    }
    if let Some(autocomplete) = options.edi_autocomplete.as_ref() {
        let compatible = matches!(
            (kind, autocomplete),
            (EdiBoundaryKind::X12, EdiAutocomplete::X12(_))
                | (EdiBoundaryKind::Edifact, EdiAutocomplete::Edifact(_))
                | (EdiBoundaryKind::Hl7, EdiAutocomplete::Hl7)
                | (EdiBoundaryKind::Tradacoms, EdiAutocomplete::Tradacoms)
                | (EdiBoundaryKind::Idoc, EdiAutocomplete::Idoc)
                | (EdiBoundaryKind::SwiftMt, EdiAutocomplete::SwiftMt)
        );
        if !compatible {
            return Err(MfdError::Unsupported(format!(
                "the {side_name} EDI boundary retains autocomplete metadata for a different dialect"
            )));
        }
    }
    if let Some(version) = options.x12_interchange_version.as_deref()
        && (version.len() != 5 || !version.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(MfdError::Unsupported(format!(
            "the {side_name} X12 interchange version must contain exactly five ASCII digits"
        )));
    }
    let mut count = 0usize;
    validate_schema_node(schema, true, &mut count).map_err(|reason| {
        MfdError::Unsupported(format!(
            "the {side_name} EDI schema cannot be exported losslessly: {reason}"
        ))
    })
}

pub(super) fn render(args: RenderArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    validate_side(args.schema, args.options, "mapping side")?;
    let kind = args.options.edi_kind.ok_or_else(|| {
        MfdError::Unsupported("internal EDI export is missing its dialect marker".to_string())
    })?;
    let (header, view) = match args.side {
        Side::Source => ("", "<view rbx=\"300\" rby=\"400\"/>"),
        Side::Target => (
            if args.default_output {
                "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t"
            } else {
                ""
            },
            "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
        ),
    };
    let attr = match args.side {
        Side::Source => "outkey",
        Side::Target => "inpkey",
    };
    let instance_role = match args.side {
        Side::Source => "inputinstance",
        Side::Target => "outputinstance",
    };
    let instance_file = args
        .instance_path
        .map(|path| {
            format!(
                "\t\t\t\t\t\t\t\t<file role=\"{instance_role}\" name=\"{}\"/>\n",
                xml_escape(path)
            )
        })
        .unwrap_or_default();
    let entries = entries_xml(args.schema, args.ports, attr, args.force_root_port)?;
    let retained_layout = retained_layout_xml(kind, args.options)?;
    let retained_settings = retained_settings_xml(kind, args.options);
    let mut out = String::new();
    let _ = write!(
        out,
        "\t\t\t\t<component name=\"{}\" library=\"text\" uid=\"{}\" kind=\"16\">\n\
         \t\t\t\t\t{header}{view}\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         {instance_file}\
         \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
         {entries}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<text type=\"edi\" kind=\"{}\">{retained_settings}{retained_layout}\t\t\t\t\t\t</text>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
        mfd_kind(kind),
    );
    Ok(RenderedSchemaComponent {
        xml: out,
        siblings: Vec::new(),
    })
}

fn retained_settings_xml(kind: EdiBoundaryKind, options: &FormatOptions) -> String {
    let autocomplete = options.edi_autocomplete.is_some();
    if kind != EdiBoundaryKind::X12 {
        return match options.edi_autocomplete.as_ref() {
            Some(EdiAutocomplete::Edifact(config)) => {
                let attribute = |name: &str, value: Option<&str>| {
                    value
                        .map(|value| format!(" {name}=\"{}\"", xml_escape(value)))
                        .unwrap_or_default()
                };
                format!(
                    "\n\t\t\t\t\t\t\t<settings autocompletedata=\"true\"{}{}{}{} />",
                    attribute("syntaxlevel", config.syntax_level.as_deref()),
                    attribute("syntaxversionnumber", config.syntax_version.as_deref()),
                    attribute("controllingagency", config.controlling_agency.as_deref()),
                    attribute("ferrulemessagetype", config.message_type.as_deref()),
                )
            }
            Some(_) => "\n\t\t\t\t\t\t\t<settings autocompletedata=\"true\"/>".to_string(),
            None => String::new(),
        };
    }
    if options.x12_separators.is_none()
        && options.x12_interchange_version.is_none()
        && !autocomplete
    {
        return String::new();
    }
    let separators = options.x12_separators.unwrap_or(mapping::X12Separators {
        element: '*',
        component: ':',
        segment: '~',
        repetition: Some('^'),
        release: None,
    });
    let repetition = separator_attribute(separators.repetition);
    let release = separator_attribute(separators.release);
    let version = options
        .x12_interchange_version
        .as_deref()
        .map(|value| format!(" interchangecontrolversionnumber=\"{}\"", xml_escape(value)))
        .unwrap_or_default();
    let (autocomplete, acknowledgement, transaction_set) = match options.edi_autocomplete.as_ref() {
        Some(EdiAutocomplete::X12(config)) => (
            " autocompletedata=\"true\"",
            if config.request_acknowledgement {
                " requestacknowledgement=\"true\""
            } else {
                " requestacknowledgement=\"false\""
            },
            config
                .transaction_set
                .as_deref()
                .map(|value| format!(" ferruletransactionset=\"{}\"", xml_escape(value)))
                .unwrap_or_default(),
        ),
        _ => ("", "", String::new()),
    };
    format!(
        "\n\t\t\t\t\t\t\t<settings{autocomplete}{acknowledgement}{transaction_set}{version}><separators dataelement=\"{}\" component=\"{}\" segment=\"{}\" repetition=\"{}\" escape=\"{}\"/></settings>",
        xml_escape(&separators.element.to_string()),
        xml_escape(&separators.component.to_string()),
        xml_escape(&separators.segment.to_string()),
        xml_escape(&repetition),
        xml_escape(&release),
    )
}

fn separator_attribute(separator: Option<char>) -> String {
    separator
        .map(|character| character.to_string())
        .unwrap_or_default()
}

fn validate_schema_node(
    node: &SchemaNode,
    root: bool,
    count: &mut usize,
) -> Result<(), &'static str> {
    *count = count.checked_add(1).ok_or("the entry count overflows")?;
    if *count > 65_536 {
        return Err("the entry tree exceeds 65,536 nodes");
    }
    if node.name.is_empty() {
        return Err("an entry name is empty");
    }
    if root && node.repeating {
        return Err("the document root is repeating");
    }
    if node.recursive_ref.is_some() {
        return Err("recursive schema references are not representable");
    }
    if node.attribute || node.text || node.nillable {
        return Err("XML-only attribute, text, or nillable metadata is present");
    }
    match &node.kind {
        SchemaKind::Scalar { .. } => Ok(()),
        SchemaKind::Group {
            children,
            alternatives,
            dynamic,
        } => {
            if node.fixed.is_some() {
                return Err("a group carries scalar fixed-value metadata");
            }
            if !alternatives.is_empty() || dynamic.is_some() {
                return Err("group alternatives or dynamic fields are present");
            }
            let mut names = BTreeSet::new();
            if children
                .iter()
                .any(|child| !names.insert(child.name.as_str()))
            {
                return Err("a group contains duplicate child names");
            }
            for child in children {
                validate_schema_node(child, false, count)?;
            }
            Ok(())
        }
    }
}

fn entries_xml(
    schema: &SchemaNode,
    ports: &PortTree,
    attr: &str,
    force_root_port: bool,
) -> Result<String, MfdError> {
    fn walk(
        node: &SchemaNode,
        path: &mut Vec<String>,
        ports: &PortTree,
        attr: &str,
        indent: usize,
        force_port: bool,
        out: &mut String,
    ) -> Result<(), MfdError> {
        let pad = "\t".repeat(indent);
        let port = if force_port || !path.is_empty() {
            let key = ports.required_key_for_abs(path, "EDI entry")?;
            format!(" {attr}=\"{key}\"")
        } else {
            String::new()
        };
        let fixed = node
            .fixed
            .as_deref()
            .map(|value| format!(" ferrule-fixed=\"{}\"", xml_escape(value)))
            .unwrap_or_default();
        let scalar_type = match &node.kind {
            SchemaKind::Scalar { ty } => {
                format!(" datatype=\"{}\"", scalar_type_name(*ty))
            }
            SchemaKind::Group { .. } => String::new(),
        };
        let node_kind = match &node.kind {
            SchemaKind::Scalar { .. } => "scalar",
            SchemaKind::Group { .. } => "group",
        };
        let _ = write!(
            out,
            "{pad}<entry name=\"{}\" ferrule-kind=\"{node_kind}\" ferrule-repeating=\"{}\"{fixed}{scalar_type}{port} expanded=\"1\"",
            xml_escape(&node.name),
            u8::from(node.repeating),
        );
        let SchemaKind::Group { children, .. } = &node.kind else {
            out.push_str("/>\n");
            return Ok(());
        };
        out.push_str(">\n");
        for child in children {
            path.push(child.name.clone());
            walk(child, path, ports, attr, indent + 1, false, out)?;
            path.pop();
        }
        let _ = writeln!(out, "{pad}</entry>");
        Ok(())
    }

    let mut out = String::new();
    walk(
        schema,
        &mut Vec::new(),
        ports,
        attr,
        9,
        force_root_port,
        &mut out,
    )?;
    Ok(out)
}

fn retained_layout_xml(kind: EdiBoundaryKind, options: &FormatOptions) -> Result<String, MfdError> {
    let serialized = match kind {
        EdiBoundaryKind::Idoc => options
            .idoc
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| {
                MfdError::Unsupported(format!("could not serialize the IDoc layout: {error}"))
            })?
            .map(|layout| ("idoc", layout)),
        EdiBoundaryKind::SwiftMt => options
            .swift_mt
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| {
                MfdError::Unsupported(format!("could not serialize the SWIFT MT layout: {error}"))
            })?
            .map(|layout| ("swift_mt", layout)),
        _ => None,
    };
    let mut output = serialized.map_or_else(
        || "\n".to_string(),
        |(kind, layout)| {
            format!(
                "\n\t\t\t\t\t\t\t<ferrule-layout kind=\"{kind}\">{}</ferrule-layout>\n",
                xml_escape(&layout)
            )
        },
    );
    if !options.edi_lexical_formats.is_empty() {
        let formats = serde_json::to_string(&options.edi_lexical_formats).map_err(|error| {
            MfdError::Unsupported(format!("could not serialize EDI lexical formats: {error}"))
        })?;
        let _ = writeln!(
            output,
            "\t\t\t\t\t\t\t<ferrule-lexical-formats>{}</ferrule-lexical-formats>",
            xml_escape(&formats)
        );
    }
    Ok(output)
}

const fn mfd_kind(kind: EdiBoundaryKind) -> &'static str {
    match kind {
        EdiBoundaryKind::X12 => "EDIX12",
        EdiBoundaryKind::Edifact => "EDIFACT",
        EdiBoundaryKind::Hl7 => "EDIHL7",
        EdiBoundaryKind::Tradacoms => "EDITRADACOMS",
        EdiBoundaryKind::Idoc => "EDIFIXED",
        EdiBoundaryKind::SwiftMt => "SWIFTMT",
    }
}

const fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}
