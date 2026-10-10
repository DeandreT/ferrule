//! Shared flat CSV input options, schema admission and exact typed failures.

use crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES;
use ir::{
    GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode, XmlAlternativeKind,
    XmlWildcardProcessContents,
};
use mapping::{FormatOptions, TabularBoundaryKind};
use std::collections::BTreeSet;
use std::fmt;

/// Literal native CSV input settings. Input accepts either UTF-8 BOM form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvInputPolicy {
    pub delimiter: Option<char>,
    pub quote: Option<char>,
    pub quote_disabled: bool,
    pub has_headers: bool,
    pub preserve_empty_strings: bool,
    pub utf8_bom: bool,
}

impl Default for CsvInputPolicy {
    fn default() -> Self {
        Self {
            delimiter: None,
            quote: None,
            quote_disabled: false,
            has_headers: true,
            preserve_empty_strings: false,
            utf8_bom: false,
        }
    }
}

impl CsvInputPolicy {
    /// Preserve the established CSV/X12 factory and its exact typed causes.
    pub fn from_format_options(
        options: &FormatOptions,
    ) -> Result<Self, crate::CsvX12BoundaryError> {
        Self::from_input_format_options(options).map_err(Into::into)
    }

    /// Capture only understood CSV settings without copying foreign metadata.
    pub fn from_input_format_options(
        options: &FormatOptions,
    ) -> Result<Self, CsvInputBoundaryError> {
        if options.csv_text_repair_dependency.is_some() {
            return Err(CsvInputBoundaryError::SourceRepairRequired);
        }
        let accepted = FormatOptions {
            tabular_kind: options
                .tabular_kind
                .filter(|kind| *kind == TabularBoundaryKind::Csv),
            delimiter: options.delimiter,
            csv_quote: options.csv_quote,
            csv_quote_disabled: options.csv_quote_disabled,
            has_header_row: options.has_header_row,
            csv_preserve_empty_strings: options.csv_preserve_empty_strings,
            csv_utf8_bom: options.csv_utf8_bom,
            ..FormatOptions::default()
        };
        if *options != accepted {
            return Err(CsvInputBoundaryError::SourceFormatOptions);
        }
        let policy = Self {
            delimiter: options.delimiter,
            quote: options.csv_quote,
            quote_disabled: options.csv_quote_disabled,
            has_headers: options.has_header_row.unwrap_or(true),
            preserve_empty_strings: options.csv_preserve_empty_strings,
            utf8_bom: options.csv_utf8_bom,
        };
        policy.validate_dialect()?;
        Ok(policy)
    }

    pub(crate) fn validate_dialect(&self) -> Result<(), CsvInputBoundaryError> {
        let delimiter = self.delimiter.unwrap_or(',');
        if !delimiter.is_ascii() || matches!(delimiter, '\0' | '\r' | '\n') {
            return Err(CsvInputBoundaryError::BadDelimiter(delimiter));
        }
        if self.quote_disabled {
            if self.quote.is_some() {
                return Err(CsvInputBoundaryError::ConflictingQuoteSettings);
            }
            return Ok(());
        }
        let quote = self.quote.unwrap_or('"');
        if !('!'..='~').contains(&quote) {
            return Err(CsvInputBoundaryError::BadQuote(quote));
        }
        if delimiter == quote {
            return Err(CsvInputBoundaryError::DelimiterQuoteConflict);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvInputField {
    pub name: String,
    pub ty: ScalarType,
}

#[derive(Debug)]
pub enum CsvInputBoundaryError {
    SourceSchema {
        field: Option<usize>,
        reason: &'static str,
    },
    SourceNameBytes {
        maximum: usize,
        observed: usize,
    },
    SourceFormatOptions,
    SourceRepairRequired,
    BadDelimiter(char),
    BadQuote(char),
    ConflictingQuoteSettings,
    DelimiterQuoteConflict,
    EmbeddedSourceSchema(codegen_schema::CodecError),
}

impl fmt::Display for CsvInputBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceSchema { field, reason } => write!(
                formatter,
                "generated CSV source schema at column {field:?}: {reason}"
            ),
            Self::SourceNameBytes { maximum, observed } => write!(
                formatter,
                "generated CSV source names use {observed} UTF-8 bytes; maximum is {maximum}"
            ),
            Self::SourceFormatOptions => {
                formatter.write_str("generated CSV input cannot use conflicting format options")
            }
            Self::SourceRepairRequired => formatter
                .write_str("generated CSV input requires repair of the stored text settings"),
            Self::BadDelimiter(character) => write!(
                formatter,
                "generated CSV input requires one ASCII delimiter other than NUL, CR or LF, got {character:?}"
            ),
            Self::BadQuote(character) => write!(
                formatter,
                "generated CSV input requires one visible ASCII quote, got {character:?}"
            ),
            Self::ConflictingQuoteSettings => formatter.write_str(
                "generated CSV input cannot combine a quote character with disabled quoting",
            ),
            Self::DelimiterQuoteConflict => {
                formatter.write_str("generated CSV input delimiter and quote must differ")
            }
            Self::EmbeddedSourceSchema(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CsvInputBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EmbeddedSourceSchema(error) => Some(error),
            _ => None,
        }
    }
}

fn canonical_metadata(node: &SchemaNode) -> bool {
    // Exhaustive borrowed destructuring keeps new metadata conservative without
    // building a recursive baseline or cloning potentially unbounded values.
    let SchemaNode {
        name: _,
        kind: _,
        repeating: _,
        xml_namespace,
        xml_name_alternatives,
        xml_wildcard_namespace,
        xml_wildcard_process_contents,
        recursive_ref,
        attribute,
        text,
        nillable,
        xml_optional,
        xml_attribute_required,
        nullable,
        container_nullable,
        json_any,
        fixed,
        json_allowed_values,
        numeric_range,
        json_multiple_of,
        item_count_range,
        json_contains,
        json_dependent_schemas,
        property_count_range,
        json_property_dependencies,
        json_pattern_property_names,
        json_property_names,
        json_unique_items,
        string_length_range,
        json_patterns,
        json_formats,
        default,
        value_generation,
        alternative_mode,
        xml_alternative_kind,
        xml_type_alternatives,
        xml_default_type,
        xml_repeating_sequences,
        xml_repeating_choices,
        database_relation,
    } = node;
    xml_namespace.is_none()
        && xml_name_alternatives.is_empty()
        && xml_wildcard_namespace.is_none()
        && *xml_wildcard_process_contents == XmlWildcardProcessContents::Skip
        && recursive_ref.is_none()
        && !attribute
        && !text
        && !nillable
        && !xml_optional
        && !xml_attribute_required
        && !nullable
        && !container_nullable
        && !json_any
        && fixed.is_none()
        && json_allowed_values.is_none()
        && numeric_range.is_none()
        && json_multiple_of.is_none()
        && item_count_range.is_none()
        && json_contains.is_none()
        && json_dependent_schemas.is_none()
        && property_count_range.is_none()
        && json_property_dependencies.is_none()
        && json_pattern_property_names.is_none()
        && json_property_names.is_none()
        && !json_unique_items
        && string_length_range.is_none()
        && json_patterns.is_none()
        && json_formats.is_empty()
        && default.is_none()
        && value_generation.is_none()
        && *alternative_mode == GroupAlternativeMode::Exclusive
        && *xml_alternative_kind == XmlAlternativeKind::XsiType
        && !xml_type_alternatives
        && xml_default_type.is_none()
        && xml_repeating_sequences.is_empty()
        && xml_repeating_choices.is_empty()
        && database_relation.is_none()
}

pub(crate) fn source_fields(schema: &SchemaNode) -> Result<&[SchemaNode], CsvInputBoundaryError> {
    let bad = |field, reason| CsvInputBoundaryError::SourceSchema { field, reason };
    let SchemaKind::Group {
        children,
        alternatives,
        required,
        xml_restricted_alternatives,
        dynamic,
    } = &schema.kind
    else {
        return Err(bad(None, "a flat group row schema is required"));
    };
    if schema.repeating
        || !alternatives.is_empty()
        || !required.is_empty()
        || !xml_restricted_alternatives.is_empty()
        || dynamic.is_some()
    {
        return Err(bad(
            None,
            "a closed non-repeating group without alternatives or required fields is required",
        ));
    }
    if children.len() > 256 {
        return Err(bad(None, "CSV row schema exceeds 256 columns"));
    }
    if !canonical_metadata(schema) {
        return Err(bad(None, "row metadata must use canonical defaults"));
    }
    let mut names = BTreeSet::new();
    let mut bytes = schema.name.len();
    let check_bytes = |observed| {
        if observed > MAX_EMBEDDED_JSON_SCHEMA_BYTES {
            Err(CsvInputBoundaryError::SourceNameBytes {
                maximum: MAX_EMBEDDED_JSON_SCHEMA_BYTES,
                observed,
            })
        } else {
            Ok(())
        }
    };
    check_bytes(bytes)?;
    for (index, child) in children.iter().enumerate() {
        if child.repeating || !matches!(child.kind, SchemaKind::Scalar { .. }) {
            return Err(bad(
                Some(index),
                "each column must have one non-repeating scalar type",
            ));
        }
        if !canonical_metadata(child) {
            return Err(bad(
                Some(index),
                "column metadata must use canonical defaults",
            ));
        }
        if child.name.is_empty() {
            return Err(bad(Some(index), "column names must be nonempty"));
        }
        bytes = bytes.saturating_add(child.name.len());
        check_bytes(bytes)?;
        if !names.insert(child.name.as_str()) {
            return Err(bad(Some(index), "column names must be unique"));
        }
    }
    Ok(children)
}
