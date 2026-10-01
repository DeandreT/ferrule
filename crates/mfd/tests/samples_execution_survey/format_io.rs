use std::borrow::Cow;
use std::path::{Path, PathBuf};

use ir::{Instance, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions, TabularBoundaryKind};

use super::FIXED_CURRENT_DATETIME;

#[path = "../support/native_instance_input.rs"]
mod native_input;

use native_input::{edi_boundary_kind, extension_for_dispatch, protobuf_layout, x12_separators};
pub(super) use native_input::{
    extension, is_http, portable_path, read_instance, resolve_sample_input,
    resolve_sample_input_from,
};

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
            format_csv::write_with_options(
                path,
                schema,
                rows,
                &format_csv::CsvWriteOptions::from(options),
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
            match edi_boundary_kind(schema, options)? {
                EdiBoundaryKind::X12 => write_x12(path, schema, &formatted, options),
                EdiBoundaryKind::Edifact => write_edifact(path, schema, &formatted, options),
                EdiBoundaryKind::Hl7 => format_edi::hl7::write(path, schema, &formatted),
                EdiBoundaryKind::Tradacoms => {
                    format_edi::tradacoms::write(path, schema, &formatted)
                }
                EdiBoundaryKind::Idoc => {
                    return Err("SAP IDoc output is not supported".to_string());
                }
                EdiBoundaryKind::SwiftMt => {
                    return Err("SWIFT MT output is not supported".to_string());
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

fn formatted_edi_output<'a>(
    instance: &'a Instance,
    options: &FormatOptions,
) -> Result<Cow<'a, Instance>, String> {
    let formatted = if options.edi_lexical_formats.is_empty() {
        Cow::Borrowed(instance)
    } else {
        let mut formatted = instance.clone();
        format_edi::apply_output_lexical_formats(&mut formatted, &options.edi_lexical_formats)
            .map_err(|error| error.to_string())?;
        Cow::Owned(formatted)
    };
    let report = format_edi::validate_values(&formatted, &options.edi_value_constraints)
        .map_err(|error| error.to_string())?;
    if report.is_empty() {
        Ok(formatted)
    } else {
        Err(report.to_string())
    }
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
    } else if options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
    {
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

#[cfg(test)]
mod tests {
    use ir::ScalarType;

    use super::*;

    #[test]
    fn retained_edi_boundary_kind_overrides_a_partial_schema_trigger() {
        let schema = SchemaNode::group(
            "HL7",
            vec![SchemaNode::group(
                "Message",
                vec![SchemaNode::group(
                    "QRD",
                    vec![SchemaNode::scalar("QRD-1", ScalarType::String)],
                )],
            )],
        );
        assert!(format_edi::dialect_of(&schema).is_err());
        let options = FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Hl7),
            ..FormatOptions::default()
        };

        assert_eq!(
            edi_boundary_kind(&schema, &options),
            Ok(EdiBoundaryKind::Hl7)
        );
    }
}
