use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ir::{SchemaKind, SchemaNode};
use mapping::{
    ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOrigin, FormatOptions, Project,
};

use crate::MfdError;

use super::schema::{GeneratedSibling, KeyAlloc, PortTree, RenderedSchemaComponent, xml_escape};

pub(super) struct RequestSchemaArtifact {
    pub(super) file_name: String,
    pub(super) path: PathBuf,
    pub(super) contents: String,
}

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    if project.target_options.external_source.is_some()
        || project
            .extra_targets
            .iter()
            .any(|target| target.options.external_source.is_some())
    {
        return Err(unsupported(
            "captured external responses are source-only and cannot be attached to an .mfd target",
        ));
    }
    validate_boundary(
        &project.source,
        &project.source_options,
        project.source_path.as_deref(),
        "primary source",
    )?;
    for source in &project.extra_sources {
        validate_boundary(
            &source.schema,
            &source.options,
            (!source.path.is_empty()).then_some(source.path.as_str()),
            &format!("secondary source `{}`", source.name),
        )?;
    }
    Ok(())
}

fn validate_boundary(
    schema: &SchemaNode,
    options: &FormatOptions,
    path: Option<&str>,
    role: &str,
) -> Result<(), MfdError> {
    let Some(boundary) = options.external_source.as_ref() else {
        return Ok(());
    };
    if has_conflicting_options(options) {
        return Err(unsupported(
            "captured external responses cannot be combined with another source format option",
        ));
    }
    validate_json_schema(schema, "response")?;
    match boundary.origin() {
        ExternalSourceOrigin::UserFunction { .. } => {
            if boundary.payload() != ExternalPayloadFormat::Json {
                return Err(unsupported(
                    "captured user-function export currently requires a JSON result contract",
                ));
            }
            if path.is_some_and(valid_http_url) {
                return Err(unsupported(
                    "a captured user-function source path must identify a local JSON instance",
                ));
            }
            Ok(())
        }
        ExternalSourceOrigin::HttpPost {
            request_format,
            request_schema,
            ..
        } => {
            if boundary.payload() != ExternalPayloadFormat::Json {
                return Err(unsupported(
                    "captured HTTP POST export currently requires a JSON response contract",
                ));
            }
            if !matches!(request_format, None | Some(ExternalPayloadFormat::Json)) {
                return Err(unsupported(
                    "captured HTTP POST export currently requires a JSON request contract",
                ));
            }
            if request_format.is_some() != request_schema.is_some() {
                return Err(unsupported(
                    "captured HTTP POST request format and schema must both be present or absent",
                ));
            }
            if let Some(schema) = request_schema {
                validate_json_schema(schema, "request")?;
            }
            let url = path.ok_or_else(|| {
                unsupported(format!(
                    "captured HTTP POST {role} requires its retained URL"
                ))
            })?;
            if !valid_http_url(url) {
                return Err(unsupported(
                    "captured HTTP POST export requires an HTTP(S) URL without credentials or a fragment",
                ));
            }
            Ok(())
        }
    }
}

fn validate_json_schema(schema: &SchemaNode, role: &str) -> Result<(), MfdError> {
    if schema.attribute
        || schema.text
        || schema.nillable
        || schema.nullable
        || schema.fixed.is_some()
        || schema.recursive_ref.is_some()
    {
        return Err(unsupported(format!(
            "captured external {role} schema `{}` uses metadata the canonical JSON entry tree cannot preserve",
            schema.name
        )));
    }
    if let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
    } = &schema.kind
    {
        if !alternatives.is_empty() || dynamic.is_some() {
            return Err(unsupported(format!(
                "captured external {role} schema `{}` uses alternatives or dynamic fields the canonical JSON entry tree cannot preserve",
                schema.name
            )));
        }
        for child in children {
            validate_json_schema(child, role)?;
        }
    }
    Ok(())
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
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.local_xml_file_set
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
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

pub(super) fn request_ports(options: &FormatOptions, keys: &mut KeyAlloc) -> Option<PortTree> {
    let boundary = options.external_source.as_ref()?;
    let ExternalSourceOrigin::HttpPost {
        request_schema: Some(schema),
        ..
    } = boundary.origin()
    else {
        return None;
    };
    Some(PortTree::build(schema, keys))
}

pub(super) fn request_schema_artifact(
    options: &FormatOptions,
    mfd_path: &Path,
    suffix: &str,
) -> Option<RequestSchemaArtifact> {
    let boundary = options.external_source.as_ref()?;
    let ExternalSourceOrigin::HttpPost {
        request_schema: Some(schema),
        ..
    } = boundary.origin()
    else {
        return None;
    };
    let stem = mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let file_name = format!("{stem}-{suffix}.schema.json");
    let path = mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&file_name);
    Some(RequestSchemaArtifact {
        file_name,
        path,
        contents: format_json::json_schema::export(schema),
    })
}

pub(super) struct RenderHttpPostArgs<'a> {
    pub(super) component_name: &'a str,
    pub(super) response_schema: &'a ir::SchemaNode,
    pub(super) response_ports: &'a PortTree,
    pub(super) request_ports: Option<&'a PortTree>,
    pub(super) request_schema_file: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) url: Option<&'a str>,
    pub(super) uid: u32,
}

pub(super) struct RenderUserFunctionArgs<'a> {
    pub(super) component_name: &'a str,
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) options: &'a FormatOptions,
    pub(super) instance_path: Option<&'a str>,
    pub(super) mfd_path: &'a Path,
    pub(super) sibling_suffix: &'a str,
    pub(super) uid: u32,
}

pub(super) fn render_user_function(
    args: RenderUserFunctionArgs<'_>,
) -> Result<RenderedSchemaComponent, MfdError> {
    let boundary = args.options.external_source.as_ref().ok_or_else(|| {
        unsupported("internal captured user-function component has no boundary metadata")
    })?;
    let ExternalSourceOrigin::UserFunction { .. } = boundary.origin() else {
        return Err(unsupported(
            "internal external source is not a captured user-function result",
        ));
    };
    if boundary.payload() != ExternalPayloadFormat::Json {
        return Err(unsupported(
            "captured user-function export supports JSON result contracts only",
        ));
    }
    let provenance = serde_json::to_string(boundary).map_err(|error| {
        unsupported(format!(
            "could not serialize captured user-function provenance ({error})"
        ))
    })?;
    let stem = args
        .mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let schema_file = format!("{stem}-{}.schema.json", args.sibling_suffix);
    let schema_path = args
        .mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&schema_file);
    let instance = args
        .instance_path
        .map(|path| format!(" inputinstance=\"{}\"", xml_escape(path)))
        .unwrap_or_default();
    let xml = format!(
        "\t\t\t\t<component name=\"{}\" library=\"json\" uid=\"{}\" kind=\"31\">\n\
         \t\t\t\t\t<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<entry name=\"root\" expanded=\"1\">\n\
         {}\
         \t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<json schema=\"{}\"{instance}>\n\
         \t\t\t\t\t\t\t<ferrule-external-source version=\"1\">{}</ferrule-external-source>\n\
         \t\t\t\t\t\t</json>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.uid,
        args.ports.json_entries_xml(args.schema, "outkey", 10),
        xml_escape(&schema_file),
        xml_escape(&provenance),
    );
    Ok(RenderedSchemaComponent {
        xml,
        siblings: vec![GeneratedSibling {
            path: schema_path,
            contents: format_json::json_schema::export(args.schema),
        }],
    })
}

pub(super) fn render_http_post(args: RenderHttpPostArgs<'_>) -> Result<String, MfdError> {
    let RenderHttpPostArgs {
        component_name,
        response_schema,
        response_ports,
        request_ports,
        request_schema_file,
        options,
        url,
        uid,
    } = args;
    let boundary = options.external_source.as_ref().ok_or_else(|| {
        unsupported("internal captured HTTP POST component has no boundary metadata")
    })?;
    let ExternalSourceOrigin::HttpPost {
        mode,
        timeout_seconds,
        request_format,
        request_schema,
        headers,
    } = boundary.origin()
    else {
        return Err(unsupported(
            "internal external source is not a captured HTTP POST",
        ));
    };
    if boundary.payload() != ExternalPayloadFormat::Json
        || !matches!(request_format, None | Some(ExternalPayloadFormat::Json))
    {
        return Err(unsupported(
            "captured HTTP POST export supports JSON request and response contracts only",
        ));
    }
    let url = url.ok_or_else(|| unsupported("captured HTTP POST source has no URL"))?;
    if !valid_http_url(url) {
        return Err(unsupported("captured HTTP POST source has an invalid URL"));
    }

    let mut roots = String::new();
    if let (Some(schema), Some(ports)) = (request_schema.as_ref(), request_ports) {
        let schema_file = request_schema_file.ok_or_else(|| {
            unsupported("captured HTTP POST request schema artifact was not prepared")
        })?;
        let _ = write!(
            roots,
            "\t\t\t\t\t\t<root>\n\
             \t\t\t\t\t\t\t<entry name=\"HTTPMessage\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t\t<entry name=\"HTTPBody\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-json\" expanded=\"1\">\n\
             \t\t\t\t\t\t\t\t\t\t<document schemafile=\"{}\" encoding=\"UTF-8\"/>\n\
             \t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" expanded=\"1\">\n\
             {}\
             \t\t\t\t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t\t</entry>\n\
             \t\t\t\t\t\t</root>\n",
            xml_escape(schema_file),
            xml_escape(&schema.name),
            ports.json_entries_xml(schema, "inpkey", 11),
        );
    }
    let _ = write!(
        roots,
        "\t\t\t\t\t\t<root rootindex=\"1\">\n\
         \t\t\t\t\t\t\t<entry name=\"HTTPMessage\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"HTTPBody\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-json\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t\t<document encoding=\"UTF-8\"/>\n\
         \t\t\t\t\t\t\t\t\t\t<entry name=\"{}\" expanded=\"1\">\n\
         {}\
         \t\t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n",
        xml_escape(&response_schema.name),
        response_ports.json_entries_xml(response_schema, "outkey", 11),
    );

    let mode = match mode {
        ExternalHttpMode::Manual => "manual",
        ExternalHttpMode::Graphql => "graphql",
    };
    let mut parameters = String::new();
    for header in headers {
        let _ = writeln!(
            parameters,
            "\t\t\t\t\t\t\t<parameter name=\"{}\" style=\"header\" required=\"{}\" mappable=\"{}\"/>",
            xml_escape(header.name()),
            u8::from(header.required()),
            u8::from(header.mapped()),
        );
    }
    Ok(format!(
        "\t\t\t\t<component name=\"{}\" library=\"webservice\" uid=\"{uid}\" kind=\"20\">\n\
         \t\t\t\t\t<properties/>\n\
         \t\t\t\t\t<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         {roots}\
         \t\t\t\t\t\t<wsdl kind=\"call\" sourceMode=\"{mode}\" url=\"{}\" timeout=\"{}\" httpmethod=\"POST\">\n\
         {parameters}\
         \t\t\t\t\t\t</wsdl>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(component_name),
        xml_escape(url),
        timeout_seconds.get(),
    ))
}

fn valid_http_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && !url.contains('#')
        && url.is_ascii()
        && !url
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn unsupported(message: impl Into<String>) -> MfdError {
    MfdError::Unsupported(message.into())
}
