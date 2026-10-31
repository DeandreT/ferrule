use ir::{SchemaKind, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions, WsdlMessageRole, XbrlBoundaryMode};

use super::ValidationIssue;

pub(super) fn validate_inactive_root_xml_read_options(
    location: &str,
    options: &FormatOptions,
    schema: &SchemaNode,
    path: Option<&str>,
    source_side: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    if !options.xml_allow_inactive_root_type_members && !options.xml_root_view_read_policy {
        return;
    }
    let accepted_options = FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: options.xml_root_view_read_policy,
        ..Default::default()
    };
    let remote = path.is_some_and(|path| {
        path.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        })
    });
    if !source_side
        || !options.xml_document
        || !options.xml_allow_inactive_root_type_members
        || *options != accepted_options
        || remote
        || !ir::xml_inactive_root_type_members_are_supported(schema)
        || (options.xml_root_view_read_policy
            && !ir::xml_root_view_read_policy_is_supported(schema))
    {
        issues.push(ValidationIssue::new(
            location,
            "`xml_allow_inactive_root_type_members` requires a closed flat typed local XML input with no other format options",
        ));
    }
}

pub(super) fn validate_csv_dialect_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    let conflicting_format = options
        .tabular_kind
        .is_some_and(|kind| kind != mapping::TabularBoundaryKind::Csv)
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json5
        || options.json_lines
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.xbrl.is_some()
        || options.protobuf.is_some()
        || options.wsdl.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || has_xlsx_format_options(options);
    if options.csv_preserve_empty_strings && conflicting_format {
        issues.push(ValidationIssue::new(
            location,
            "`csv_preserve_empty_strings` cannot be combined with another format's options",
        ));
    }
    if options.csv_utf8_bom && conflicting_format {
        issues.push(ValidationIssue::new(
            location,
            "`csv_utf8_bom` cannot be combined with another format's options",
        ));
    }
    let delimiter = options.delimiter.unwrap_or(',');
    if !delimiter.is_ascii() || matches!(delimiter, '\0' | '\r' | '\n') {
        issues.push(ValidationIssue::new(
            location,
            "CSV delimiter must be one non-NUL, non-newline byte",
        ));
    }
    if options.csv_quote_disabled {
        if options.csv_quote.is_some() {
            issues.push(ValidationIssue::new(
                location,
                "CSV quote cannot be set while quoting is disabled",
            ));
        }
        return;
    }
    let quote = options.csv_quote.unwrap_or('"');
    if !quote.is_ascii_graphic() {
        issues.push(ValidationIssue::new(
            location,
            "CSV quote must be one printable ASCII character",
        ));
    }
    if delimiter == quote {
        issues.push(ValidationIssue::new(
            location,
            "CSV delimiter and quote must be different characters",
        ));
    }
}

pub(super) fn validate_target_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_xml_schema_hint_options(location, options, true, issues);
    validate_json5_options(location, options, issues);
    validate_structured_edi_options(location, options, issues);
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
    if options.local_xml_file_set {
        issues.push(ValidationIssue::new(
            location,
            "local XML file sets are valid only for mapping sources",
        ));
    }
}

pub(super) fn validate_json5_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(dependency) = options.json5_interchange_blocker() {
        issues.push(ValidationIssue::new(location, dependency.to_string()));
    }
    if !options.json5 {
        return;
    }
    if options.json_lines {
        issues.push(ValidationIssue::new(
            location,
            "JSON5 document syntax cannot be combined with JSON Lines",
        ));
    }
    if options.lenient_segments
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.swift_mt.is_some()
        || options.xml_document
        || options.wsdl.is_some()
        || options.local_xml_file_set
        || options.tabular_kind.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_xlsx_format_options(options)
    {
        issues.push(ValidationIssue::new(
            location,
            "JSON5 document syntax cannot be combined with another format's options",
        ));
    }
}

pub(super) fn validate_structured_edi_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    if options.edi_config_reference.is_some() && options.edi_kind.is_none() {
        issues.push(ValidationIssue::new(
            location,
            "an unresolved EDI configuration dependency requires `edi_kind`",
        ));
    }
    if options.edi_config_reference.is_some()
        && (options.idoc.is_some() || options.swift_mt.is_some())
    {
        issues.push(ValidationIssue::new(
            location,
            "an unresolved EDI configuration dependency cannot be combined with an embedded EDI layout",
        ));
    }
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

pub(super) fn validate_idoc_native_options(
    location: &str,
    options: &FormatOptions,
    schema: &SchemaNode,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(settings) = options.idoc_native_text_settings.as_ref() {
        if options.idoc_native_config.is_none() {
            issues.push(ValidationIssue::new(
                location,
                "`idoc_native_text_settings` requires a certified `idoc_native_config` descriptor",
            ));
        }
        if options.edi_kind != Some(EdiBoundaryKind::Idoc) {
            issues.push(ValidationIssue::new(
                location,
                "`idoc_native_text_settings` requires `edi_kind` to be `idoc`",
            ));
        }
        if !matches!(
            (
                settings.autocomplete_data(),
                options.edi_autocomplete.as_ref()
            ),
            (true, Some(EdiAutocomplete::Idoc)) | (false, None)
        ) {
            issues.push(ValidationIssue::new(
                location,
                "`idoc_native_text_settings` conflicts with the EDI autocomplete option",
            ));
        }
    }
    let Some(descriptor) = &options.idoc_native_config else {
        return;
    };
    if options.edi_kind != Some(EdiBoundaryKind::Idoc) {
        issues.push(ValidationIssue::new(
            location,
            "`idoc_native_config` requires `edi_kind` to be `idoc`",
        ));
    }
    if options.edi_config_reference.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "`idoc_native_config` cannot be combined with an unresolved EDI configuration dependency",
        ));
    }
    let Some(layout) = &options.idoc else {
        issues.push(ValidationIssue::new(
            location,
            "`idoc_native_config` requires an embedded `idoc` layout",
        ));
        return;
    };
    match descriptor.project() {
        Ok((projected_schema, projected_layout)) => {
            if &projected_schema != schema {
                issues.push(ValidationIssue::new(
                    location,
                    "`idoc_native_config` does not match the boundary schema",
                ));
            }
            if &projected_layout != layout {
                issues.push(ValidationIssue::new(
                    location,
                    "`idoc_native_config` does not match the embedded `idoc` layout",
                ));
            }
        }
        Err(error) => issues.push(ValidationIssue::new(
            location,
            format!("`idoc_native_config` cannot project to an IDoc boundary: {error}"),
        )),
    }
}

fn has_non_idoc_format_options(options: &FormatOptions) -> bool {
    options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.json5
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_xlsx_format_options(options)
}

fn has_non_swift_format_options(options: &FormatOptions) -> bool {
    options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.json5
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
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.json_lines
        || options.json5
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
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.json5
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
    // Update-existing is checked against each layout below.
    let legacy_layout = options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
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
    let has_grid = options.has_xlsx_grid();
    if options.xlsx_update_existing {
        let refused_layout = if !options.xlsx_rows.is_empty() {
            Some("a transposed")
        } else if has_grid {
            Some("a grid")
        } else if options.xlsx_composite.is_some() {
            Some("a composite")
        } else if options.xlsx_worksheet_set.is_some() {
            Some("a worksheet-set")
        } else {
            None
        };
        if let Some(layout) = refused_layout {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "`xlsx_update_existing` cannot update {layout} XLSX layout; only flat tables and hierarchical layouts update existing workbooks"
                ),
            ));
        }
    }
    if options.xlsx_worksheet_set.is_some() && !source_side {
        issues.push(ValidationIssue::new(
            location,
            "`xlsx_worksheet_set` is valid only for mapping sources",
        ));
    }
    if let Some(composite) = &options.xlsx_composite {
        if let Err(message) = composite.validate_regions() {
            issues.push(ValidationIssue::new(location, message));
        }
        let mut grid_sheets = std::collections::BTreeSet::new();
        if composite
            .grids
            .iter()
            .any(|grid| !grid_sheets.insert(grid.layout.sheet.as_deref()))
        {
            issues.push(ValidationIssue::new(
                location,
                "several XLSX grids cannot share one worksheet; each grid needs its own worksheet",
            ));
        }
    }
    if has_grid && !source_side {
        issues.push(ValidationIssue::new(
            location,
            "grid XLSX output is not supported; XLSX grids are valid only for mapping sources",
        ));
    }
    if !options.xlsx_rows.is_empty() && !source_side {
        validate_transposed_xlsx_target(location, options, schema, issues);
    }
    if !options.xlsx_headers.is_empty() {
        let field_count = match &schema.kind {
            SchemaKind::Group { children, .. }
                if !schema.repeating
                    && children.iter().all(|child| {
                        !child.repeating && !child.attribute && !child.text && child.is_scalar()
                    }) =>
            {
                Some(children.len())
            }
            SchemaKind::Scalar { .. }
            | SchemaKind::ScalarUnion { .. }
            | SchemaKind::Group { .. } => None,
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

/// A transposed target writes one record per worksheet column: each data
/// field owns one selected row, and an optional integer `n` field selects the
/// record's column. It writes no header and has no table start or columns.
fn validate_transposed_xlsx_target(
    location: &str,
    options: &FormatOptions,
    schema: &SchemaNode,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut refuse = |message: String| issues.push(ValidationIssue::new(location, message));
    if options.has_header_row == Some(true) {
        refuse(
            "a transposed XLSX target writes no header row; leave `has_header_row` unset or false"
                .into(),
        );
    }
    if options.xlsx_start_row.is_some() || !options.xlsx_columns.is_empty() {
        refuse("a transposed XLSX target places fields by `xlsx_rows` and records by column; `xlsx_start_row` and `xlsx_columns` cannot be used with it".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    if options
        .xlsx_rows
        .iter()
        .any(|row| !(1..=MAX_XLSX_ROW).contains(row) || !seen.insert(*row))
    {
        refuse(format!(
            "transposed XLSX target rows must be distinct worksheet rows from 1 to {MAX_XLSX_ROW}"
        ));
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        refuse("a transposed XLSX target needs a non-repeating group of scalar fields".into());
        return;
    };
    if schema.repeating
        || children.iter().any(|child| {
            child.repeating
                || child.attribute
                || child.text
                || !matches!(child.kind, SchemaKind::Scalar { .. })
        })
    {
        refuse("a transposed XLSX target needs a non-repeating group of scalar fields".into());
        return;
    }
    let positions = children
        .iter()
        .filter(|child| child.name == "n")
        .collect::<Vec<_>>();
    if positions.len() > 1
        || positions.iter().any(|child| {
            !matches!(
                child.kind,
                SchemaKind::Scalar {
                    ty: ir::ScalarType::Int
                }
            )
        })
    {
        refuse("a transposed XLSX target's `n` field must be one integer column position".into());
    }
    let data_fields = children.len() - positions.len();
    if data_fields != options.xlsx_rows.len() {
        refuse(format!(
            "a transposed XLSX target has {} row selector(s) for {data_fields} data field(s)",
            options.xlsx_rows.len()
        ));
    }
}

const MAX_XLSX_ROW: u32 = 1_048_576;

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
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.swift_mt.is_some()
        || options.local_xml_file_set
        || options.json_document
        || options.json_lines
        || options.json5
        || options.tabular_kind.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
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

pub(super) fn validate_xml_schema_hint_options(
    location: &str,
    options: &FormatOptions,
    output: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Err(message) = options.validate_xml_schema_hint_options(output) {
        issues.push(ValidationIssue::new(location, message));
    }
}

#[cfg(test)]
mod idoc_text_tests {
    use mapping::{
        IdocNativeTextSettings, IdocNativeValidationAction, IdocNativeValidationCase,
        IdocNativeValidationKind,
    };

    use super::*;

    #[test]
    fn text_certificate_requires_a_paired_descriptor_and_autocomplete_state() {
        let settings = IdocNativeTextSettings::new_observed_profile(
            false,
            true,
            false,
            IdocNativeValidationKind::ALL
                .into_iter()
                .map(|kind| IdocNativeValidationCase::new(kind, IdocNativeValidationAction::Stop))
                .collect(),
        )
        .unwrap();
        let options = FormatOptions {
            edi_kind: Some(EdiBoundaryKind::Idoc),
            idoc_native_text_settings: Some(settings),
            ..FormatOptions::default()
        };
        let mut issues = Vec::new();
        validate_idoc_native_options(
            "source",
            &options,
            &SchemaNode::group("IDOC", Vec::new()),
            &mut issues,
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("requires a certified"))
        );
        assert!(issues.iter().any(|issue| {
            issue
                .message
                .contains("conflicts with the EDI autocomplete")
        }));
    }
}
