use mapping::FormatOptions;

use super::{Json5BoundaryPolicyError, Json5BoundarySide};

pub(super) fn validate(
    options: &FormatOptions,
    side: Json5BoundarySide,
) -> Result<(), Json5BoundaryPolicyError> {
    // No wildcard or cloning: every retained option has an explicit policy.
    let FormatOptions {
        lenient_segments,
        edi_kind,
        edi_config_reference,
        edi_implied_decimals,
        edi_lexical_formats,
        edi_value_constraints,
        edi_autocomplete,
        x12_separators,
        x12_interchange_version,
        idoc,
        idoc_native_config,
        idoc_native_text_settings,
        swift_mt,
        xml_document,
        xml_schema_hints,
        xml_allow_inactive_root_type_members,
        xml_root_view_read_policy,
        wsdl,
        local_xml_file_set,
        json_document: _,
        json_schema_unresolved_reference,
        mfd_decimal_input_names,
        json5: _,
        tabular_kind,
        delimiter,
        csv_quote,
        csv_quote_disabled,
        csv_utf8_bom,
        csv_preserve_empty_strings,
        csv_text_repair_dependency,
        has_header_row,
        fixed_width,
        flextext,
        pdf,
        http_get,
        external_source,
        json_lines,
        protobuf,
        xbrl,
        xlsx_sheet,
        xlsx_start_row,
        xlsx_columns,
        xlsx_headers,
        xlsx_update_existing,
        xlsx_rows,
        xlsx_composite,
        xlsx_worksheet_set,
        xlsx_grid,
        xlsx_hierarchical,
    } = options;
    macro_rules! none { ($($field:ident),+ $(,)?) => { $(if $field.is_some() { return Err(Json5BoundaryPolicyError::FormatOption { side, field: stringify!($field) }); })+ }; }
    macro_rules! empty { ($($field:ident),+ $(,)?) => { $(if !$field.is_empty() { return Err(Json5BoundaryPolicyError::FormatOption { side, field: stringify!($field) }); })+ }; }
    macro_rules! off { ($($field:ident),+ $(,)?) => { $(if *$field { return Err(Json5BoundaryPolicyError::FormatOption { side, field: stringify!($field) }); })+ }; }
    none!(
        edi_kind,
        edi_config_reference,
        edi_autocomplete,
        x12_separators,
        x12_interchange_version,
        idoc,
        idoc_native_config,
        idoc_native_text_settings,
        swift_mt,
        xml_schema_hints,
        wsdl,
        json_schema_unresolved_reference,
        tabular_kind,
        delimiter,
        csv_quote,
        csv_text_repair_dependency,
        has_header_row,
        fixed_width,
        flextext,
        pdf,
        http_get,
        external_source,
        protobuf,
        xbrl,
        xlsx_sheet,
        xlsx_start_row,
        xlsx_composite,
        xlsx_worksheet_set,
        xlsx_grid,
        xlsx_hierarchical
    );
    empty!(
        edi_implied_decimals,
        edi_lexical_formats,
        edi_value_constraints,
        mfd_decimal_input_names,
        xlsx_columns,
        xlsx_headers,
        xlsx_rows
    );
    off!(
        lenient_segments,
        xml_document,
        xml_allow_inactive_root_type_members,
        xml_root_view_read_policy,
        local_xml_file_set,
        csv_quote_disabled,
        csv_utf8_bom,
        csv_preserve_empty_strings,
        json_lines,
        xlsx_update_existing
    );
    Ok(())
}
