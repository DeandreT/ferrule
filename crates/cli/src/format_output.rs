//! Filesystem output dispatch and format-specific writer options.

use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions};

use crate::format_selection::{
    self, Direction, JsonMode, Origin, SelectedFormat, reject_inactive_root_xml_read_options,
    validate_csv_metadata_identity,
};
use crate::{has_flat_xlsx_layout, protobuf_layout, x12_separators};

pub(super) fn output_format<'a>(
    path: &'a Path,
    schema: &SchemaNode,
    options: &'a FormatOptions,
) -> anyhow::Result<SelectedFormat<'a>> {
    validate_xml_scalar_output_format(path, schema, options)?;
    if options.sqlite_read.is_some() {
        bail!("`sqlite_read` is valid only for mapping sources");
    }
    options
        .validate_xml_schema_hint_options(true)
        .map_err(anyhow::Error::msg)?;
    validate_csv_metadata_identity(path, options, "output")?;
    reject_inactive_root_xml_read_options(path, schema, options, "output")?;
    if options.local_xml_file_set && !options.xml_document {
        bail!("`local_xml_file_set` requires `xml_document` for output");
    }
    if options.local_xml_file_set {
        bail!("`local_xml_file_set` is input-only");
    }
    format_selection::select(path, options, Direction::Output, false)
}

pub(super) fn write_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> anyhow::Result<usize> {
    let selected = output_format(path, schema, options)?;
    if let SelectedFormat::Xbrl(xbrl) = selected {
        format_xbrl::write(path, schema, instance, xbrl)
            .with_context(|| format!("writing XBRL output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Idoc(layout) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        format_edi::idoc::write(path, schema, &formatted, layout)
            .with_context(|| format!("writing SAP IDoc output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Swift(_) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        format_edi::swift::write_with_options(path, schema, &formatted, options)
            .with_context(|| format!("writing SWIFT MT output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Pdf(_) = selected {
        bail!("PDF output is not supported; `pdf` is input-only");
    }
    if let SelectedFormat::FlexText(layout) = selected {
        format_flextext::write(path, schema, instance, layout)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Protobuf(protobuf) = selected {
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        format_protobuf::write(path, &layout, &protobuf.root_message, instance)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Edi(Some(kind)) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        return write_edi_output(path, schema, &formatted, options, kind, current_datetime);
    }
    if let SelectedFormat::Xml(Origin::Explicit) = selected {
        format_xml::write_with_options(path, schema, instance, &xml_write_options(options))
            .with_context(|| format!("writing XML output {}", path.display()))?;
        return Ok(1);
    }
    if let SelectedFormat::Json(mode, Origin::Explicit) = selected {
        let write = if mode == JsonMode::Lines {
            format_json::write_lines
        } else if mode == JsonMode::Json5 {
            format_json::write_json5
        } else {
            format_json::write
        };
        write(path, schema, instance)
            .with_context(|| format!("writing JSON output {}", path.display()))?;
        return Ok(instance.as_repeated().map_or(1, <[Instance]>::len));
    }
    if let SelectedFormat::FixedWidth(layout) = selected {
        let rows = instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        format_csv::write_fixed_width(path, schema, rows, layout)
            .with_context(|| format!("writing output {}", path.display()))?;
        return Ok(rows.len());
    }

    match selected {
        SelectedFormat::Csv => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            format_csv::write_with_options(
                path,
                schema,
                rows,
                &format_csv::CsvWriteOptions::from(options),
            )
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(rows.len())
        }
        SelectedFormat::Xlsx => {
            if let Some(layout) = &options.xlsx_hierarchical {
                if options.xlsx_grid.is_some()
                    || options.xlsx_composite.is_some()
                    || options.xlsx_worksheet_set.is_some()
                    || has_flat_xlsx_layout(options)
                {
                    bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
                }
                if options.xlsx_update_existing {
                    return format_xlsx::update_hierarchical(
                        path,
                        schema,
                        instance,
                        layout,
                        &format_xlsx::XlsxLimits::default(),
                    )
                    .with_context(|| format!("updating output {}", path.display()));
                }
                return format_xlsx::write_hierarchical(path, schema, instance, layout)
                    .with_context(|| format!("writing output {}", path.display()));
            }
            if options.has_xlsx_grid() {
                bail!("grid XLSX output is not supported; XLSX grids are input-only");
            }
            if options.xlsx_worksheet_set.is_some() {
                bail!(
                    "worksheet-set XLSX output is not supported; `xlsx_worksheet_set` is input-only"
                );
            }
            if options.xlsx_composite.is_some() {
                bail!("composite XLSX output is not supported; `xlsx_composite` is input-only");
            }
            if !options.xlsx_rows.is_empty() {
                let records = transposed_xlsx_records(instance, options)?;
                format_xlsx::write_transposed(
                    path,
                    schema,
                    records,
                    options.xlsx_sheet.as_deref(),
                    &options.xlsx_rows,
                )
                .with_context(|| format!("writing output {}", path.display()))?;
                return Ok(records.len());
            }
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for an XLSX output")?;
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
            result.with_context(|| format!("writing output {}", path.display()))?;
            Ok(rows.len())
        }
        SelectedFormat::Xml(Origin::Fallback) => {
            format_xml::write_with_options(path, schema, instance, &xml_write_options(options))
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(1)
        }
        SelectedFormat::Json(mode, Origin::Fallback) => {
            if mode == JsonMode::Lines {
                format_json::write_lines(path, schema, instance)
            } else if mode == JsonMode::Json5 {
                format_json::write_json5(path, schema, instance)
            } else {
                format_json::write(path, schema, instance)
            }
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(instance.as_repeated().map_or(1, <[Instance]>::len))
        }
        SelectedFormat::Sqlite => {
            format_db::write_instance(path, schema, instance)
                .with_context(|| format!("writing output {}", path.display()))?;
            Ok(instance.as_repeated().map_or(1, <[Instance]>::len))
        }
        SelectedFormat::Edi(None) => {
            let formatted = formatted_edi_output(instance, options)?;
            match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => {
                    write_x12_output(path, schema, &formatted, options, current_datetime)
                }
                format_edi::Dialect::Edifact => {
                    write_edifact_output(path, schema, &formatted, options, current_datetime)
                }
                format_edi::Dialect::Hl7 => format_edi::hl7::write(path, schema, &formatted),
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::write(path, schema, &formatted)
                }
            }
            .with_context(|| format!("writing output {}", path.display()))?;
            Ok(1)
        }
        SelectedFormat::MissingPdf => bail!("PDF output is not supported; PDF is input-only"),
        SelectedFormat::MissingIdoc => bail!("unsupported output file extension: .idoc"),
        SelectedFormat::MissingSwift => {
            bail!("SWIFT MT output requires an embedded `swift_mt` layout and message headers")
        }
        SelectedFormat::Unsupported(other) => bail!("unsupported output file extension: .{other}"),
        _ => unreachable!("explicit formats returned above"),
    }
}

pub(super) fn formatted_edi_output<'a>(
    instance: &'a Instance,
    options: &FormatOptions,
) -> anyhow::Result<Cow<'a, Instance>> {
    let formatted = if options.edi_lexical_formats.is_empty() {
        Cow::Borrowed(instance)
    } else {
        let mut formatted = instance.clone();
        format_edi::apply_output_lexical_formats(&mut formatted, &options.edi_lexical_formats)
            .context("applying EDI output lexical formats")?;
        Cow::Owned(formatted)
    };
    let report = format_edi::validate_values(&formatted, &options.edi_value_constraints)
        .context("validating EDI output values")?;
    if !report.is_empty() {
        return Err(format_edi::EdiFormatError::Validation(report))
            .context("validating EDI output values");
    }
    Ok(formatted)
}

fn write_edi_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    current_datetime: &str,
) -> anyhow::Result<usize> {
    match kind {
        EdiBoundaryKind::X12 => write_x12_output(path, schema, instance, options, current_datetime),
        EdiBoundaryKind::Edifact => {
            write_edifact_output(path, schema, instance, options, current_datetime)
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::write(path, schema, instance),
        EdiBoundaryKind::Tradacoms => format_edi::tradacoms::write(path, schema, instance),
        EdiBoundaryKind::Idoc => {
            bail!("SAP IDoc output requires an embedded runtime layout")
        }
        EdiBoundaryKind::SwiftMt => {
            bail!("SWIFT MT output requires an embedded `swift_mt` layout")
        }
    }
    .with_context(|| format!("writing EDI output {}", path.display()))?;
    Ok(1)
}

fn write_x12_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> Result<(), format_edi::EdiFormatError> {
    let separators = options
        .x12_separators
        .map(x12_separators)
        .unwrap_or_default();
    let version = options.x12_interchange_version.as_deref();
    match options.edi_autocomplete.as_ref() {
        Some(EdiAutocomplete::X12(config)) => format_edi::x12::write_with_syntax_and_autocomplete(
            path,
            schema,
            instance,
            separators,
            version,
            format_edi::x12::Autocomplete {
                current_datetime,
                request_acknowledgement: config.request_acknowledgement,
                transaction_set: config.transaction_set.as_deref(),
            },
        ),
        _ => format_edi::x12::write_with_syntax(path, schema, instance, separators, version),
    }
}

fn write_edifact_output(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> Result<(), format_edi::EdiFormatError> {
    if let Some(EdiAutocomplete::Edifact(config)) = options.edi_autocomplete.as_ref() {
        format_edi::edifact::write_with_autocomplete(
            path,
            schema,
            instance,
            format_edi::edifact::Autocomplete {
                current_datetime,
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

/// The records of a new-workbook transposed XLSX target. Engine validation
/// rejects these combinations first; this guards direct library callers.
pub(super) fn transposed_xlsx_records<'a>(
    instance: &'a Instance,
    options: &FormatOptions,
) -> anyhow::Result<&'a [Instance]> {
    if options.xlsx_update_existing {
        bail!(
            "`xlsx_update_existing` cannot update a transposed XLSX layout; transposed targets write a new workbook"
        );
    }
    if options.has_header_row == Some(true)
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
    {
        bail!(
            "transposed XLSX output cannot be combined with flat header, start-row, or column options"
        );
    }
    instance
        .as_repeated()
        .context("mapping did not produce a repeating record set for a transposed XLSX output")
}

pub(super) fn xml_write_options(options: &FormatOptions) -> format_xml::XmlWriteOptions {
    format_xml::XmlWriteOptions {
        scalar_output: options.xml_scalar_output.clone(),
        schema_hints: options.xml_schema_hints.clone(),
        ..Default::default()
    }
}

pub(super) fn validate_xml_scalar_output_format(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
) -> anyhow::Result<()> {
    if let Some(policy) = &options.xml_scalar_output {
        options
            .validate_xml_scalar_output_options()
            .map_err(anyhow::Error::msg)?;
        policy.validate_schema(schema)?;
        if path
            .extension()
            .is_some_and(|extension| !extension.eq_ignore_ascii_case("xml"))
        {
            bail!("explicit XML scalar output cannot be used with a non-XML output path");
        }
    }
    Ok(())
}
