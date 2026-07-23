use std::borrow::Cow;
use std::path::{Path, PathBuf};

use ir::{Instance, SchemaNode};
use mapping::{
    EdiAutocomplete, EdiBoundaryKind, ExternalPayloadFormat, FormatOptions, ProtobufOptions,
    TabularBoundaryKind,
};

use super::FIXED_CURRENT_DATETIME;

pub(super) fn extension(path: &Path) -> Result<String, String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("path `{}` has no usable extension", path.display()))
}

fn extension_for_dispatch(path: &Path, options: &FormatOptions) -> Result<String, String> {
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

pub(super) fn is_http(value: &str) -> bool {
    value.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

pub(super) fn portable_path(value: &str) -> PathBuf {
    PathBuf::from(value.replace('\\', "/"))
}

pub(super) fn resolve_sample_input(samples_root: &Path, stored: &str) -> Result<PathBuf, String> {
    resolve_sample_input_from(samples_root, samples_root, stored)
}

pub(super) fn resolve_sample_input_from(
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

pub(super) fn read_instance(
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
        "csv" | "txt" => format_csv::read(
            path,
            schema,
            options.delimiter,
            options.has_header_row.unwrap_or(true),
        )
        .map(Instance::Repeated)
        .map_err(|error| error.to_string()),
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
    let mut instance = match format_edi::dialect_of(schema).map_err(|error| error.to_string())? {
        format_edi::Dialect::X12 => format_edi::x12::read_with_separators(
            path,
            schema,
            options.lenient_segments,
            options.x12_separators.map(x12_separators),
        ),
        format_edi::Dialect::Edifact => {
            format_edi::edifact::read(path, schema, options.lenient_segments)
        }
        format_edi::Dialect::Hl7 => format_edi::hl7::read(path, schema, options.lenient_segments),
        format_edi::Dialect::Tradacoms => {
            format_edi::tradacoms::read(path, schema, options.lenient_segments)
        }
    }
    .map_err(|error| error.to_string())?;
    format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)
        .map_err(|error| error.to_string())?;
    Ok(instance)
}

pub(super) fn write_instance(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), String> {
    if let Some(xbrl) = &options.xbrl {
        return format_xbrl::write(path, schema, instance, xbrl).map_err(|error| error.to_string());
    }
    if options.idoc.is_some() {
        return Err("SAP IDoc output is not supported".to_string());
    }
    if options.swift_mt.is_some() {
        return Err("SWIFT MT output is not supported".to_string());
    }
    if options.pdf.is_some() {
        return Err("PDF output is not supported".to_string());
    }
    if let Some(layout) = &options.flextext {
        return format_flextext::write(path, schema, instance, layout)
            .map_err(|error| error.to_string());
    }
    if let Some(protobuf) = &options.protobuf {
        let layout = protobuf_layout(protobuf)?;
        return format_protobuf::write(path, &layout, &protobuf.root_message, instance)
            .map_err(|error| error.to_string());
    }
    if let Some(layout) = &options.fixed_width {
        let rows = instance
            .as_repeated()
            .ok_or_else(|| "fixed-width output is not a repeating row set".to_string())?;
        return format_csv::write_fixed_width(path, schema, rows, layout)
            .map_err(|error| error.to_string());
    }
    if options.xml_document {
        return format_xml::write(path, schema, instance).map_err(|error| error.to_string());
    }

    match extension_for_dispatch(path, options)?.as_str() {
        "csv" | "txt" => {
            let rows = instance
                .as_repeated()
                .ok_or_else(|| "CSV output is not a repeating row set".to_string())?;
            format_csv::write(
                path,
                schema,
                rows,
                options.delimiter,
                options.has_header_row.unwrap_or(true),
            )
            .map_err(|error| error.to_string())
        }
        "xlsx" => write_xlsx(path, schema, instance, options),
        "xml" => format_xml::write(path, schema, instance).map_err(|error| error.to_string()),
        "json" | "jsonl" | "ndjson" if options.json_lines => {
            format_json::write_lines(path, schema, instance).map_err(|error| error.to_string())
        }
        "json" | "jsonl" | "ndjson" => {
            format_json::write(path, schema, instance).map_err(|error| error.to_string())
        }
        "db" | "sqlite" | "sqlite3" => {
            format_db::write_instance(path, schema, instance).map_err(|error| error.to_string())
        }
        "edi" | "x12" | "edifact" => {
            let formatted = formatted_edi_output(instance, options)?;
            match format_edi::dialect_of(schema).map_err(|error| error.to_string())? {
                format_edi::Dialect::X12 => write_x12(path, schema, &formatted, options),
                format_edi::Dialect::Edifact => write_edifact(path, schema, &formatted, options),
                format_edi::Dialect::Hl7 => format_edi::hl7::write(path, schema, &formatted),
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::write(path, schema, &formatted)
                }
            }
            .map_err(|error| error.to_string())
        }
        "hl7" => {
            let formatted = formatted_edi_output(instance, options)?;
            format_edi::hl7::write(path, schema, &formatted).map_err(|error| error.to_string())
        }
        other => Err(format!("unsupported output file extension `.{other}`")),
    }
}

fn protobuf_layout(options: &ProtobufOptions) -> Result<format_protobuf::Layout, String> {
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

fn formatted_edi_output<'a>(
    instance: &'a Instance,
    options: &FormatOptions,
) -> Result<Cow<'a, Instance>, String> {
    if options.edi_lexical_formats.is_empty() {
        return Ok(Cow::Borrowed(instance));
    }
    let mut formatted = instance.clone();
    format_edi::apply_output_lexical_formats(&mut formatted, &options.edi_lexical_formats)
        .map_err(|error| error.to_string())?;
    Ok(Cow::Owned(formatted))
}

fn write_x12(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), format_edi::EdiFormatError> {
    let separators = options
        .x12_separators
        .map(x12_separators)
        .unwrap_or_default();
    match options.edi_autocomplete.as_ref() {
        Some(EdiAutocomplete::X12(config)) => format_edi::x12::write_with_syntax_and_autocomplete(
            path,
            schema,
            instance,
            separators,
            options.x12_interchange_version.as_deref(),
            format_edi::x12::Autocomplete {
                current_datetime: FIXED_CURRENT_DATETIME,
                request_acknowledgement: config.request_acknowledgement,
                transaction_set: config.transaction_set.as_deref(),
            },
        ),
        _ => format_edi::x12::write_with_syntax(
            path,
            schema,
            instance,
            separators,
            options.x12_interchange_version.as_deref(),
        ),
    }
}

fn write_edifact(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), format_edi::EdiFormatError> {
    if let Some(EdiAutocomplete::Edifact(config)) = options.edi_autocomplete.as_ref() {
        format_edi::edifact::write_with_autocomplete(
            path,
            schema,
            instance,
            format_edi::edifact::Autocomplete {
                current_datetime: FIXED_CURRENT_DATETIME,
                syntax_level: config.syntax_level.as_deref(),
                syntax_version: config.syntax_version.as_deref(),
                controlling_agency: config.controlling_agency.as_deref(),
                message_type: config.message_type.as_deref(),
            },
        )
    } else {
        format_edi::edifact::write(path, schema, instance)
    }
}

fn x12_separators(separators: mapping::X12Separators) -> format_edi::x12::Separators {
    format_edi::x12::Separators {
        element: separators.element,
        component: separators.component,
        segment: separators.segment,
        repetition: separators.repetition,
        release: separators.release,
    }
}

fn write_xlsx(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), String> {
    if let Some(layout) = &options.xlsx_hierarchical {
        return format_xlsx::write_hierarchical(path, schema, instance, layout)
            .map(|_| ())
            .map_err(|error| error.to_string());
    }
    if options.xlsx_grid.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_composite.is_some()
        || !options.xlsx_rows.is_empty()
    {
        return Err("the selected XLSX input layout cannot be used for output".to_string());
    }
    let rows = instance
        .as_repeated()
        .ok_or_else(|| "XLSX output is not a repeating row set".to_string())?;
    let result = if options.xlsx_update_existing {
        format_xlsx::update_with_options(
            path,
            schema,
            rows,
            format_xlsx::FlatTableWriteOptions {
                sheet: options.xlsx_sheet.as_deref(),
                start_row: options.xlsx_start_row.unwrap_or(1),
                columns: &options.xlsx_columns,
                headers: &options.xlsx_headers,
                has_header: options.has_header_row.unwrap_or(true),
            },
        )
    } else {
        format_xlsx::write_with_options(
            path,
            schema,
            rows,
            format_xlsx::FlatTableWriteOptions {
                sheet: options.xlsx_sheet.as_deref(),
                start_row: options.xlsx_start_row.unwrap_or(1),
                columns: &options.xlsx_columns,
                headers: &options.xlsx_headers,
                has_header: options.has_header_row.unwrap_or(true),
            },
        )
    };
    result.map_err(|error| error.to_string())
}

pub(super) fn inferred_extension(options: &FormatOptions) -> Option<&'static str> {
    if options.xbrl.is_some() {
        Some("xbrl")
    } else if options.protobuf.is_some() {
        Some("bin")
    } else if options.flextext.is_some() || options.fixed_width.is_some() {
        Some("txt")
    } else if options.tabular_kind == Some(TabularBoundaryKind::Xlsx) {
        Some("xlsx")
    } else if options.tabular_kind == Some(TabularBoundaryKind::Csv) {
        Some("csv")
    } else if options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
    {
        Some("xlsx")
    } else if options.delimiter.is_some() || options.has_header_row.is_some() {
        Some("csv")
    } else if options.json_lines {
        Some("jsonl")
    } else if options.json_document {
        Some("json")
    } else if options.xml_document {
        Some("xml")
    } else {
        match options.edi_kind {
            Some(EdiBoundaryKind::X12) => Some("x12"),
            Some(EdiBoundaryKind::Edifact) => Some("edifact"),
            Some(EdiBoundaryKind::Hl7) => Some("hl7"),
            Some(EdiBoundaryKind::Tradacoms) => Some("edi"),
            Some(EdiBoundaryKind::Idoc) => Some("idoc"),
            Some(EdiBoundaryKind::SwiftMt) => Some("fin"),
            None => None,
        }
    }
}

pub(super) fn output_path(
    sample_dir: &Path,
    stored: Option<&str>,
    options: &FormatOptions,
    label: &str,
) -> Result<PathBuf, String> {
    let file_name = stored
        .filter(|value| !value.trim().is_empty() && !is_http(value))
        .and_then(|value| portable_path(value).file_name().map(|name| name.to_owned()));
    if let Some(file_name) = file_name {
        return Ok(sample_dir.join(file_name));
    }
    let extension = inferred_extension(options)
        .ok_or_else(|| format!("{label} has no stored output path or retained format marker"))?;
    Ok(sample_dir.join(format!("{label}.{extension}")))
}
