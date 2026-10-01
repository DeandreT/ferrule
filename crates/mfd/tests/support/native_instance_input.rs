//! Private read-only native instance helpers shared by opt-in local corpus tests.
//! Keep adapter semantics identical to the execution survey; no output publication belongs here.

use ir::{Instance, SchemaNode};
use mapping::{
    EdiBoundaryKind, ExternalPayloadFormat, FormatOptions, ProtobufOptions, TabularBoundaryKind,
};
use std::path::{Path, PathBuf};

pub(crate) fn extension(path: &Path) -> Result<String, String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("path `{}` has no usable extension", path.display()))
}

pub(crate) fn extension_for_dispatch(
    path: &Path,
    options: &FormatOptions,
) -> Result<String, String> {
    let explicit = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match (explicit, options.tabular_kind) {
        (Some(extension), _) if is_recognized_instance_extension(&extension) => Ok(extension),
        (_, Some(TabularBoundaryKind::Csv)) => Ok("csv".to_string()),
        (_, Some(TabularBoundaryKind::Xlsx)) => Ok("xlsx".to_string()),
        (Some(extension), None) => Ok(extension),
        (None, None) => Err(format!("path `{}` has no usable extension", path.display())),
    }
}

fn is_recognized_instance_extension(extension: &str) -> bool {
    matches!(
        extension,
        "csv"
            | "txt"
            | "xlsx"
            | "xml"
            | "json"
            | "jsonl"
            | "ndjson"
            | "db"
            | "sqlite"
            | "sqlite3"
            | "edi"
            | "x12"
            | "edifact"
            | "hl7"
            | "idoc"
            | "fin"
            | "swift"
            | "pdf"
            | "xbrl"
    )
}

pub(crate) fn is_http(value: &str) -> bool {
    value.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

pub(crate) fn portable_path(value: &str) -> PathBuf {
    PathBuf::from(value.replace('\\', "/"))
}

pub(crate) fn resolve_sample_input(samples_root: &Path, stored: &str) -> Result<PathBuf, String> {
    resolve_sample_input_from(samples_root, samples_root, stored)
}

pub(crate) fn resolve_sample_input_from(
    samples_root: &Path,
    design_base: &Path,
    stored: &str,
) -> Result<PathBuf, String> {
    if stored.trim().is_empty() {
        return Err("input instance path is empty".to_string());
    }
    if is_http(stored) {
        return Err("network input is disabled by the read-only execution survey".to_string());
    }
    let stored = portable_path(stored);
    let candidate = if stored.is_absolute() {
        stored
    } else {
        design_base.join(stored)
    };
    let resolved = std::fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "local input `{}` is unavailable: {error}",
            candidate.display()
        )
    })?;
    let canonical_root = std::fs::canonicalize(samples_root)
        .map_err(|error| format!("resolving sample root failed: {error}"))?;
    if !resolved.starts_with(&canonical_root) {
        return Err(format!(
            "local input `{}` escapes the read-only sample directory",
            candidate.display()
        ));
    }
    if !resolved.is_file() {
        return Err(format!(
            "local input `{}` is not a file",
            resolved.display()
        ));
    }
    Ok(resolved)
}

pub(crate) fn read_instance(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<Instance, String> {
    if let Some(xbrl) = &options.xbrl {
        return format_xbrl::read_with_options(path, schema, xbrl)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.idoc {
        return format_edi::idoc::read(path, schema, layout, options.lenient_segments)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.swift_mt {
        return format_edi::swift::read(path, schema, layout, options.lenient_segments)
            .map_err(|error| error.to_string());
    }
    if let Some(boundary) = &options.external_source {
        return match boundary.payload() {
            ExternalPayloadFormat::Json => {
                format_json::read(path, schema).map_err(|error| error.to_string())
            }
            ExternalPayloadFormat::Xml => {
                format_xml::read(path, schema).map_err(|error| error.to_string())
            }
        };
    }
    if let Some(layout) = &options.pdf {
        return format_pdf::read(path, layout).map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.flextext {
        return format_flextext::read(path, schema, layout).map_err(|error| error.to_string());
    }
    if let Some(protobuf) = &options.protobuf {
        let layout = protobuf_layout(protobuf)?;
        return format_protobuf::read(path, &layout, &protobuf.root_message)
            .map_err(|error| error.to_string());
    }
    if let Some(wsdl) = &options.wsdl {
        return format_xml::read_wsdl_message(path, schema, wsdl.operation())
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.fixed_width {
        return format_csv::read_fixed_width(path, schema, layout)
            .map(Instance::Repeated)
            .map_err(|error| error.to_string());
    }
    if options.xml_document {
        return format_xml::read(path, schema).map_err(|error| error.to_string());
    }

    match extension_for_dispatch(path, options)?.as_str() {
        "csv" | "txt" => {
            format_csv::read_with_options(path, schema, &format_csv::CsvReadOptions::from(options))
                .map(Instance::Repeated)
                .map_err(|error| error.to_string())
        }
        "xlsx" => read_xlsx(path, schema, options),
        "xml" => format_xml::read(path, schema).map_err(|error| error.to_string()),
        "json" | "jsonl" | "ndjson" if options.json_lines => {
            format_json::read_lines(path, schema).map_err(|error| error.to_string())
        }
        "json" | "jsonl" | "ndjson" => {
            format_json::read(path, schema).map_err(|error| error.to_string())
        }
        "db" | "sqlite" | "sqlite3" => {
            format_db::read_instance(path, schema).map_err(|error| error.to_string())
        }
        "edi" | "x12" | "edifact" | "hl7" => read_edi(path, schema, options),
        "idoc" => Err("SAP IDoc input has no embedded layout".to_string()),
        "fin" | "swift" => Err("SWIFT MT input has no embedded layout".to_string()),
        "pdf" => Err("PDF input has no embedded extraction layout".to_string()),
        other => Err(format!("unsupported input file extension `.{other}`")),
    }
}

fn read_xlsx(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<Instance, String> {
    if let Some(layout) = &options.xlsx_hierarchical {
        return format_xlsx::read_hierarchical(path, schema, layout)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.xlsx_grid {
        return format_xlsx::read_grid(path, schema, layout)
            .map(Instance::Repeated)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.xlsx_worksheet_set {
        return format_xlsx::read_worksheet_set(path, schema, layout)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.xlsx_composite {
        return format_xlsx::read_composite(path, schema, layout)
            .map_err(|error| error.to_string());
    }
    let rows = if options.xlsx_rows.is_empty() {
        format_xlsx::read(
            path,
            schema,
            options.xlsx_sheet.as_deref(),
            options.xlsx_start_row.unwrap_or(1),
            &options.xlsx_columns,
            options.has_header_row.unwrap_or(true),
        )
    } else {
        format_xlsx::read_transposed(
            path,
            schema,
            options.xlsx_sheet.as_deref(),
            &options.xlsx_rows,
        )
    };
    rows.map(Instance::Repeated)
        .map_err(|error| error.to_string())
}

fn read_edi(path: &Path, schema: &SchemaNode, options: &FormatOptions) -> Result<Instance, String> {
    let mut instance = match edi_boundary_kind(schema, options)? {
        EdiBoundaryKind::X12 => format_edi::x12::read_with_separators(
            path,
            schema,
            options.lenient_segments,
            options.x12_separators.map(x12_separators),
        ),
        EdiBoundaryKind::Edifact => {
            format_edi::edifact::read(path, schema, options.lenient_segments)
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::read(path, schema, options.lenient_segments),
        EdiBoundaryKind::Tradacoms => {
            format_edi::tradacoms::read(path, schema, options.lenient_segments)
        }
        EdiBoundaryKind::Idoc => {
            return Err("SAP IDoc input has no embedded layout".to_string());
        }
        EdiBoundaryKind::SwiftMt => {
            return Err("SWIFT MT input has no embedded layout".to_string());
        }
    }
    .map_err(|error| error.to_string())?;
    format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
        .map_err(|error| error.to_string())?;
    Ok(instance)
}

pub(crate) fn edi_boundary_kind(
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<EdiBoundaryKind, String> {
    if let Some(kind) = options.edi_kind {
        return Ok(kind);
    }
    match format_edi::dialect_of(schema).map_err(|error| error.to_string())? {
        format_edi::Dialect::X12 => Ok(EdiBoundaryKind::X12),
        format_edi::Dialect::Edifact => Ok(EdiBoundaryKind::Edifact),
        format_edi::Dialect::Hl7 => Ok(EdiBoundaryKind::Hl7),
        format_edi::Dialect::Tradacoms => Ok(EdiBoundaryKind::Tradacoms),
    }
}

pub(crate) fn protobuf_layout(
    options: &ProtobufOptions,
) -> Result<format_protobuf::Layout, String> {
    format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn x12_separators(separators: mapping::X12Separators) -> format_edi::x12::Separators {
    format_edi::x12::Separators {
        element: separators.element,
        component: separators.component,
        segment: separators.segment,
        repetition: separators.repetition,
        release: separators.release,
    }
}
