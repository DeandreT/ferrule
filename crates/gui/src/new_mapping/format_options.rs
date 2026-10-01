use mapping::FormatOptions;

/// Explicit layouts retain their own settings when a filename changes.
pub(crate) fn configured_layout_label(options: &FormatOptions) -> Option<&'static str> {
    if options.csv_text_repair_dependency.is_some() {
        Some("CSV repair required")
    } else if options.protobuf.is_some() {
        Some("Protocol Buffers")
    } else if options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
    {
        Some("SAP IDoc")
    } else if options.swift_mt.is_some() {
        Some("SWIFT MT")
    } else if options.pdf.is_some() {
        Some("PDF")
    } else if options.flextext.is_some() {
        Some("FlexText")
    } else if options.fixed_width.is_some() {
        Some("Fixed-width text")
    } else if options.xbrl.is_some() {
        Some("XBRL")
    } else if options.wsdl.is_some() {
        Some("WSDL message")
    } else if options.http_get.is_some() {
        Some("HTTP XML input")
    } else if options.external_source.is_some() {
        Some("Captured service response")
    } else if options.local_xml_file_set {
        Some("XML file set")
    } else if options.json5 {
        Some("JSON5")
    } else if options.edi_kind.is_some()
        || options.edi_config_reference.is_some()
        || !options.edi_implied_decimals.is_empty()
        || !options.edi_lexical_formats.is_empty()
        || !options.edi_value_constraints.is_empty()
        || options.edi_autocomplete.is_some()
        || options.x12_separators.is_some()
        || options.x12_interchange_version.is_some()
        || options.lenient_segments
    {
        Some("EDI")
    } else {
        None
    }
}

/// Tabular identities are fallbacks: recognized filenames select their own adapter.
fn effective_tabular_kind(
    options: &FormatOptions,
    path: &str,
) -> Option<mapping::TabularBoundaryKind> {
    if configured_layout_label(options).is_some()
        || options.xml_document
        || options.json_document
        || options.json_lines
    {
        return None;
    }
    let extension = std::path::Path::new(path.trim())
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("csv" | "txt") => Some(mapping::TabularBoundaryKind::Csv),
        Some("xlsx") => Some(mapping::TabularBoundaryKind::Xlsx),
        Some(
            "xml" | "json" | "json5" | "jsonl" | "ndjson" | "db" | "sqlite" | "sqlite3" | "edi"
            | "x12" | "edifact" | "hl7" | "idoc" | "fin" | "swift" | "pdf" | "xbrl",
        ) => None,
        _ => options.tabular_kind,
    }
}

pub(crate) fn uses_csv_format(options: &FormatOptions, path: &str) -> bool {
    effective_tabular_kind(options, path) == Some(mapping::TabularBoundaryKind::Csv)
}

pub(crate) fn uses_xlsx_format(options: &FormatOptions, path: &str) -> bool {
    effective_tabular_kind(options, path) == Some(mapping::TabularBoundaryKind::Xlsx)
}

pub(crate) fn configured_layout_label_for_path(
    options: &FormatOptions,
    path: &str,
) -> Option<&'static str> {
    configured_layout_label(options).or_else(|| {
        (has_workbook_options(options) && uses_xlsx_format(options, path))
            .then_some("XLSX workbook")
    })
}

pub(crate) fn uses_path_format(options: &FormatOptions, path: &str) -> bool {
    configured_layout_label_for_path(options, path).is_none()
        && !options.xml_document
        && !options.json_document
        && !options.json_lines
}

pub(crate) fn can_update_existing_workbook(options: &FormatOptions, path: &str) -> bool {
    uses_xlsx_format(options, path)
        && options.xlsx_rows.is_empty()
        && options.xlsx_composite.is_none()
        && options.xlsx_worksheet_set.is_none()
        && options.xlsx_grid.is_none()
        && options.xlsx_hierarchical.is_none()
}

pub(crate) fn has_workbook_options(options: &FormatOptions) -> bool {
    options.tabular_kind == Some(mapping::TabularBoundaryKind::Xlsx)
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
