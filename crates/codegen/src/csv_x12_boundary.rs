//! Explicit flat CSV input and singular X12 output admission.

use std::collections::BTreeSet;
use std::fmt;

use ir::{
    GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode, XmlAlternativeKind,
    XmlWildcardProcessContents,
};
use mapping::{FormatOptions, Project, Scope, ScopeIteration, TabularBoundaryKind};

use crate::{
    MAX_EMBEDDED_JSON_SCHEMA_BYTES, Program, ProgramValidationError, TargetConstruction,
    X12BoundaryOptions, X12BoundaryPolicyError, X12BoundarySide,
};

#[cfg(test)]
mod tests;

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
    /// Capture only understood CSV settings without copying foreign metadata.
    pub fn from_format_options(options: &FormatOptions) -> Result<Self, CsvX12BoundaryError> {
        if options.csv_text_repair_dependency.is_some() {
            return Err(CsvX12BoundaryError::SourceRepairRequired);
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
            return Err(CsvX12BoundaryError::SourceFormatOptions);
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

    fn validate_dialect(&self) -> Result<(), CsvX12BoundaryError> {
        let delimiter = self.delimiter.unwrap_or(',');
        if !delimiter.is_ascii() || matches!(delimiter, '\0' | '\r' | '\n') {
            return Err(CsvX12BoundaryError::BadDelimiter(delimiter));
        }
        if self.quote_disabled {
            if self.quote.is_some() {
                return Err(CsvX12BoundaryError::ConflictingQuoteSettings);
            }
            return Ok(());
        }
        let quote = self.quote.unwrap_or('"');
        if !('!'..='~').contains(&quote) {
            return Err(CsvX12BoundaryError::BadQuote(quote));
        }
        if delimiter == quote {
            return Err(CsvX12BoundaryError::DelimiterQuoteConflict);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvInputField {
    pub name: String,
    pub ty: ScalarType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvX12BoundaryPolicy {
    pub source: CsvInputPolicy,
    pub target: X12BoundaryOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvX12BoundaryProfile {
    pub fields: Vec<CsvInputField>,
    pub source: CsvInputPolicy,
    pub target_descriptor: String,
}

#[derive(Debug)]
pub enum CsvX12BoundaryError {
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
    ProgramField {
        field: &'static str,
    },
    EmbeddedSourceSchema(codegen_schema::CodecError),
    TargetX12(Box<X12BoundaryPolicyError>),
    Validation(ProgramValidationError),
}

impl fmt::Display for CsvX12BoundaryError {
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
            Self::ProgramField { field } => write!(
                formatter,
                "generated CSV-to-X12 program field {field} is unsupported"
            ),
            Self::EmbeddedSourceSchema(error) => error.fmt(formatter),
            Self::TargetX12(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CsvX12BoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EmbeddedSourceSchema(error) => Some(error),
            Self::TargetX12(error) => Some(error.as_ref()),
            Self::Validation(error) => Some(error),
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

fn source_fields(schema: &SchemaNode) -> Result<&[SchemaNode], CsvX12BoundaryError> {
    let bad = |field, reason| CsvX12BoundaryError::SourceSchema { field, reason };
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
            Err(CsvX12BoundaryError::SourceNameBytes {
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

fn prepare(
    source: &SchemaNode,
    target: &SchemaNode,
    policy: &CsvX12BoundaryPolicy,
) -> Result<CsvX12BoundaryProfile, CsvX12BoundaryError> {
    policy.source.validate_dialect()?;
    let children = source_fields(source)?;
    // Exact escaped descriptor size is separate from the borrowed name budget.
    codegen_schema::encode(source, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(CsvX12BoundaryError::EmbeddedSourceSchema)?;
    crate::x12_boundary::preflight_schema_bounds(target, X12BoundarySide::Target)
        .map_err(|error| CsvX12BoundaryError::TargetX12(Box::new(error)))?;
    let target_descriptor =
        crate::x12_boundary::descriptor(target, Some(&policy.target), X12BoundarySide::Target)
            .map_err(|error| CsvX12BoundaryError::TargetX12(Box::new(error)))?;
    Ok(CsvX12BoundaryProfile {
        fields: children
            .iter()
            .map(|child| {
                let SchemaKind::Scalar { ty } = child.kind else {
                    unreachable!("borrowed source scalar proof")
                };
                CsvInputField {
                    name: child.name.clone(),
                    ty,
                }
            })
            .collect(),
        source: policy.source.clone(),
        target_descriptor,
    })
}

fn program_field(field: &'static str) -> CsvX12BoundaryError {
    CsvX12BoundaryError::ProgramField { field }
}

fn single_output(
    target: &SchemaNode,
    repeating: bool,
    iterating: bool,
) -> Result<(), CsvX12BoundaryError> {
    if target.repeating || repeating || iterating {
        return Err(program_field("primary_output"));
    }
    Ok(())
}

fn project_dynamic_documents(root: &Scope) -> Result<(), CsvX12BoundaryError> {
    let mut pending = vec![root];
    while let Some(scope) = pending.pop() {
        if matches!(scope.iteration, ScopeIteration::DynamicDocuments { .. }) {
            return Err(program_field("dynamic_target"));
        }
        if let Some(sequence) = scope.concatenated() {
            pending.extend(sequence.iter());
        }
        pending.extend(scope.children.iter());
        pending.extend(scope.dynamic_children.iter().map(|child| &child.scope));
    }
    Ok(())
}

/// Bound both borrowed schemas before ordinary Project lowering and copies.
pub fn prepare_csv_x12_project_boundary(
    project: &Project,
    policy: &CsvX12BoundaryPolicy,
) -> Result<CsvX12BoundaryProfile, CsvX12BoundaryError> {
    if !project.extra_sources.is_empty() {
        return Err(program_field("extra_sources"));
    }
    if !project.extra_targets.is_empty() {
        return Err(program_field("extra_targets"));
    }
    single_output(
        &project.target,
        false,
        !matches!(project.root.iteration, ScopeIteration::None),
    )?;
    let profile = prepare(&project.source, &project.target, policy)?;
    project_dynamic_documents(&project.root)?;
    Ok(profile)
}

/// Admit the complete document before ordinary mapping validation or emission.
pub fn prepare_csv_x12_boundary(
    program: &Program,
    policy: &CsvX12BoundaryPolicy,
) -> Result<CsvX12BoundaryProfile, CsvX12BoundaryError> {
    if program.xml_boundary.is_some() {
        return Err(program_field("xml_boundary"));
    }
    if !program.extra_sources.is_empty() {
        return Err(program_field("extra_sources"));
    }
    if !program.extra_targets.is_empty() {
        return Err(program_field("extra_targets"));
    }
    single_output(
        &program.target,
        program.root.repeating,
        program.root.iteration.is_some(),
    )?;
    let profile = prepare(&program.source, &program.target, policy)?;
    let mut pending = vec![&program.root];
    while let Some(scope) = pending.pop() {
        if let Some(iteration) = &scope.iteration {
            if iteration.dynamic_document_iteration().is_some() {
                return Err(program_field("dynamic_target"));
            }
            if let Some(sequence) = iteration.concatenated() {
                pending.extend(sequence.iter());
            }
        }
        pending.extend(scope.children.iter());
        if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
            pending.extend(children.iter().map(|child| &child.scope));
        }
    }
    crate::validate_program(program).map_err(CsvX12BoundaryError::Validation)?;
    Ok(profile)
}
