use ir::{SchemaKind, SchemaNode};
use mapping::{FormatOptions, WsdlMessageRole, XbrlBoundaryMode};

use super::ValidationIssue;

pub(super) fn validate_target_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_xbrl_options(location, options, XbrlBoundaryMode::ExternalTarget, issues);
    validate_external_source_options(location, options, false, issues);
    if options.http_get.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "HTTP GET transport is valid only for mapping sources",
        ));
    }
    if options.pdf.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "PDF extraction is valid only for mapping sources",
        ));
    }
    if options.idoc.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "SAP IDoc layouts are valid only for mapping sources",
        ));
    }
    if options.swift_mt.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "SWIFT MT layouts are valid only for mapping sources",
        ));
    }
}

pub(super) fn validate_structured_edi_source_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    if options.idoc.is_some() && has_non_idoc_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`idoc` cannot be combined with another format's options",
        ));
    }
    if options.swift_mt.is_some() && has_non_swift_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`swift_mt` cannot be combined with another format's options",
        ));
    }
}

fn has_non_idoc_format_options(options: &FormatOptions) -> bool {
    options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_xlsx_format_options(options)
}

fn has_non_swift_format_options(options: &FormatOptions) -> bool {
    options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_xlsx_format_options(options)
}

fn has_xlsx_format_options(options: &FormatOptions) -> bool {
    options.xlsx_sheet.is_some()
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

pub(super) fn validate_external_source_options(
    location: &str,
    options: &FormatOptions,
    source_side: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    if options.external_source.is_none() {
        return;
    }
    if !source_side {
        issues.push(ValidationIssue::new(
            location,
            "captured external responses are valid only for mapping sources",
        ));
    }
    if has_non_external_source_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`external_source` cannot be combined with another format's options",
        ));
    }
}

fn has_non_external_source_format_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
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

pub(super) fn validate_xbrl_options(
    location: &str,
    options: &FormatOptions,
    expected_mode: XbrlBoundaryMode,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(xbrl) = &options.xbrl else {
        return;
    };
    if xbrl.mode() != expected_mode {
        let actual_mode = match xbrl.mode() {
            XbrlBoundaryMode::ExternalSource => "external source",
            XbrlBoundaryMode::ExternalTarget => "external target",
        };
        let expected_side = match expected_mode {
            XbrlBoundaryMode::ExternalSource => "source",
            XbrlBoundaryMode::ExternalTarget => "target",
        };
        issues.push(ValidationIssue::new(
            location,
            format!("XBRL boundary mode `{actual_mode}` is invalid on a mapping {expected_side}"),
        ));
    }
    if has_non_xbrl_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`xbrl` cannot be combined with another format's options",
        ));
    }
}

fn has_non_xbrl_format_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
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

pub(super) fn validate_xlsx_options(
    location: &str,
    options: &FormatOptions,
    schema: &SchemaNode,
    source_side: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    let retained_layouts = usize::from(options.xlsx_composite.is_some())
        + usize::from(options.xlsx_worksheet_set.is_some())
        + usize::from(options.xlsx_grid.is_some())
        + usize::from(options.xlsx_hierarchical.is_some());
    if retained_layouts > 1 {
        issues.push(ValidationIssue::new(
            location,
            "retained XLSX composite, worksheet-set, grid, and hierarchical layouts are mutually exclusive",
        ));
    }
    let retained_layout = retained_layouts > 0;
    let legacy_layout = options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.has_header_row.is_some();
    if retained_layout && legacy_layout {
        issues.push(ValidationIssue::new(
            location,
            "a retained XLSX layout cannot be combined with flat or transposed XLSX options",
        ));
    }
    let non_flat_layout = !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some();
    if !options.xlsx_headers.is_empty() && non_flat_layout {
        issues.push(ValidationIssue::new(
            location,
            "`xlsx_headers` can be used only with a flat XLSX table",
        ));
    }
    if options.xlsx_update_existing && source_side {
        issues.push(ValidationIssue::new(
            location,
            "`xlsx_update_existing` is valid only for mapping targets",
        ));
    }
    if options.xlsx_update_existing && non_flat_layout {
        issues.push(ValidationIssue::new(
            location,
            "`xlsx_update_existing` can be used only with a flat XLSX table",
        ));
    }
    if options.xlsx_worksheet_set.is_some() && !source_side {
        issues.push(ValidationIssue::new(
            location,
            "`xlsx_worksheet_set` is valid only for mapping sources",
        ));
    }
    if !options.xlsx_headers.is_empty() {
        let field_count = match &schema.kind {
            SchemaKind::Group { children, .. }
                if !schema.repeating
                    && children.iter().all(|child| {
                        !child.repeating
                            && !child.attribute
                            && !child.text
                            && matches!(child.kind, SchemaKind::Scalar { .. })
                    }) =>
            {
                Some(children.len())
            }
            SchemaKind::Scalar { .. } | SchemaKind::Group { .. } => None,
        };
        match field_count {
            Some(count) if count != options.xlsx_headers.len() => {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "`xlsx_headers` has {} value(s) for {count} flat schema field(s)",
                        options.xlsx_headers.len()
                    ),
                ));
            }
            None => issues.push(ValidationIssue::new(
                location,
                "`xlsx_headers` requires a non-repeating group of scalar fields",
            )),
            Some(_) => {}
        }
    }
}

pub(super) fn validate_wsdl_options(
    location: &str,
    options: &FormatOptions,
    source_side: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(wsdl) = &options.wsdl else {
        return;
    };
    let role_is_valid = if source_side {
        wsdl.role() == WsdlMessageRole::Request
    } else {
        wsdl.role() != WsdlMessageRole::Request
    };
    if !role_is_valid {
        issues.push(ValidationIssue::new(
            location,
            if source_side {
                "a WSDL source must be a request message"
            } else {
                "a WSDL target must be a response or fault message"
            },
        ));
    }
    if !options.xml_document {
        issues.push(ValidationIssue::new(
            location,
            "a WSDL message requires XML document identity",
        ));
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
        || has_xlsx_format_options(options);
    if conflict {
        issues.push(ValidationIssue::new(
            location,
            "a WSDL message cannot be combined with another format identity",
        ));
    }
}
