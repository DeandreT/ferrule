//! Explicit flat CSV input and singular X12 output admission.

use std::fmt;

use ir::{SchemaKind, SchemaNode};
use mapping::{Project, Scope, ScopeIteration};

use crate::csv_input_boundary::source_fields;

use crate::{
    CsvInputField, CsvInputPolicy, MAX_EMBEDDED_JSON_SCHEMA_BYTES, Program, ProgramValidationError,
    TargetConstruction, X12BoundaryOptions, X12BoundaryPolicyError, X12BoundarySide,
};

#[cfg(test)]
mod tests;

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

impl From<crate::CsvInputBoundaryError> for CsvX12BoundaryError {
    fn from(error: crate::CsvInputBoundaryError) -> Self {
        use crate::CsvInputBoundaryError as Source;
        match error {
            Source::SourceSchema { field, reason } => Self::SourceSchema { field, reason },
            Source::SourceNameBytes { maximum, observed } => {
                Self::SourceNameBytes { maximum, observed }
            }
            Source::SourceFormatOptions => Self::SourceFormatOptions,
            Source::SourceRepairRequired => Self::SourceRepairRequired,
            Source::BadDelimiter(value) => Self::BadDelimiter(value),
            Source::BadQuote(value) => Self::BadQuote(value),
            Source::ConflictingQuoteSettings => Self::ConflictingQuoteSettings,
            Source::DelimiterQuoteConflict => Self::DelimiterQuoteConflict,
            Source::EmbeddedSourceSchema(error) => Self::EmbeddedSourceSchema(error),
        }
    }
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
