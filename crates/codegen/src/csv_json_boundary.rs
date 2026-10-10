//! Explicit flat CSV input and selected singular strict JSON output admission.

use std::collections::BTreeSet;
use std::fmt;

use ir::{SchemaKind, SchemaNode};
use mapping::{Project, Scope, ScopeIteration};

use crate::csv_input_boundary::source_fields;
use crate::{
    CsvInputBoundaryError, CsvInputField, CsvInputPolicy, MAX_EMBEDDED_JSON_SCHEMA_BYTES, Program,
    ProgramValidationError, TargetConstruction, TargetScope, X12BoundaryPolicyError,
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvJsonBoundaryPolicy {
    pub source: CsvInputPolicy,
    pub extra_target_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedJsonTargetDescriptor {
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvJsonBoundaryProfile {
    pub fields: Vec<CsvInputField>,
    pub source: CsvInputPolicy,
    pub target_descriptor: String,
    pub extra_target_descriptors: Vec<NamedJsonTargetDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvJsonBoundaryOwner {
    Source,
    Target,
    NamedTarget { index: usize, name: String },
}

#[derive(Debug)]
pub enum CsvJsonBoundaryError {
    Source(CsvInputBoundaryError),
    PolicyNames {
        field: &'static str,
    },
    ProgramField {
        field: &'static str,
    },
    TargetField {
        owner: CsvJsonBoundaryOwner,
        field: &'static str,
    },
    EmbeddedSchema {
        owner: CsvJsonBoundaryOwner,
        error: codegen_schema::CodecError,
    },
    JsonFormatOptions {
        owner: CsvJsonBoundaryOwner,
        error: Box<X12BoundaryPolicyError>,
    },
    Validation(Box<ProgramValidationError>),
}

impl fmt::Display for CsvJsonBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(formatter),
            Self::PolicyNames { field } => write!(
                formatter,
                "CSV-to-JSON {field} policy names must exactly match declaration order"
            ),
            Self::ProgramField { field } => write!(
                formatter,
                "generated CSV-to-JSON program field {field} is unsupported"
            ),
            Self::TargetField { owner, field } => write!(
                formatter,
                "generated CSV-to-JSON {owner:?} field {field} is unsupported"
            ),
            Self::EmbeddedSchema { owner, error } => write!(
                formatter,
                "generated CSV-to-JSON {owner:?} descriptor: {error}"
            ),
            Self::JsonFormatOptions { owner, .. } => write!(
                formatter,
                "generated CSV-to-JSON {owner:?} requires plain strict JSON format options"
            ),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CsvJsonBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::EmbeddedSchema { error, .. } => Some(error),
            Self::JsonFormatOptions { error, .. } => Some(error.as_ref()),
            Self::Validation(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<CsvInputBoundaryError> for CsvJsonBoundaryError {
    fn from(error: CsvInputBoundaryError) -> Self {
        Self::Source(error)
    }
}

fn names<'a>(
    declared: impl Iterator<Item = &'a str>,
    policy: &CsvJsonBoundaryPolicy,
) -> Result<(), CsvJsonBoundaryError> {
    let declared = declared.collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    if declared.len() != policy.extra_target_names.len()
        || declared
            .iter()
            .zip(&policy.extra_target_names)
            .any(|(name, expected)| *name != expected.as_str() || !seen.insert(expected.as_str()))
    {
        return Err(CsvJsonBoundaryError::PolicyNames {
            field: "extra_targets",
        });
    }
    Ok(())
}

fn named_owner(index: usize, name: &str) -> CsvJsonBoundaryOwner {
    CsvJsonBoundaryOwner::NamedTarget {
        index,
        name: name.into(),
    }
}

fn prepare<'a>(
    source: &SchemaNode,
    target: &SchemaNode,
    targets: impl Iterator<Item = (&'a str, &'a SchemaNode)>,
    policy: &CsvJsonBoundaryPolicy,
) -> Result<CsvJsonBoundaryProfile, CsvJsonBoundaryError> {
    policy.source.validate_dialect()?;
    // Borrow and encode every endpoint before recursive ordinary validation,
    // lowering or schema copies, retaining the original typed codec causes.
    codegen_schema::encode(source, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(CsvInputBoundaryError::EmbeddedSourceSchema)?;
    let target_descriptor = codegen_schema::encode(target, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(|error| CsvJsonBoundaryError::EmbeddedSchema {
            owner: CsvJsonBoundaryOwner::Target,
            error,
        })?;
    let mut extra_target_descriptors = Vec::new();
    for (index, (name, schema)) in targets.enumerate() {
        let descriptor =
            codegen_schema::encode(schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).map_err(|error| {
                CsvJsonBoundaryError::EmbeddedSchema {
                    owner: named_owner(index, name),
                    error,
                }
            })?;
        extra_target_descriptors.push(NamedJsonTargetDescriptor {
            name: name.into(),
            descriptor,
        });
    }
    let children = source_fields(source)?;
    Ok(CsvJsonBoundaryProfile {
        fields: children
            .iter()
            .map(|child| {
                let SchemaKind::Scalar { ty } = child.kind else {
                    unreachable!("borrowed CSV scalar proof")
                };
                CsvInputField {
                    name: child.name.clone(),
                    ty,
                }
            })
            .collect(),
        source: policy.source.clone(),
        target_descriptor,
        extra_target_descriptors,
    })
}

fn single_output(
    schema: &SchemaNode,
    repeating: bool,
    iterating: bool,
    owner: CsvJsonBoundaryOwner,
) -> Result<(), CsvJsonBoundaryError> {
    if schema.repeating || repeating || iterating {
        return Err(CsvJsonBoundaryError::TargetField {
            owner,
            field: "document_iteration",
        });
    }
    Ok(())
}

fn project_dynamic_documents(
    root: &Scope,
    owner: CsvJsonBoundaryOwner,
) -> Result<(), CsvJsonBoundaryError> {
    let mut pending = vec![root];
    while let Some(scope) = pending.pop() {
        if matches!(scope.iteration, ScopeIteration::DynamicDocuments { .. }) {
            return Err(CsvJsonBoundaryError::TargetField {
                owner,
                field: "dynamic_target",
            });
        }
        if let Some(sequence) = scope.concatenated() {
            pending.extend(sequence.iter());
        }
        pending.extend(scope.children.iter());
        pending.extend(scope.dynamic_children.iter().map(|child| &child.scope));
    }
    Ok(())
}

fn program_dynamic_documents(
    root: &TargetScope,
    owner: CsvJsonBoundaryOwner,
) -> Result<(), CsvJsonBoundaryError> {
    let mut pending = vec![root];
    while let Some(scope) = pending.pop() {
        if let Some(iteration) = &scope.iteration {
            if iteration.dynamic_document_iteration().is_some() {
                return Err(CsvJsonBoundaryError::TargetField {
                    owner,
                    field: "dynamic_target",
                });
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
    Ok(())
}

/// Bound complete borrowed endpoints before ordinary lowering; executes no mapping.
pub fn prepare_csv_json_project_boundary(
    project: &Project,
    policy: &CsvJsonBoundaryPolicy,
) -> Result<CsvJsonBoundaryProfile, CsvJsonBoundaryError> {
    if !project.extra_sources.is_empty() {
        return Err(CsvJsonBoundaryError::ProgramField {
            field: "extra_sources",
        });
    }
    names(
        project
            .extra_targets
            .iter()
            .map(|target| target.name.as_str()),
        policy,
    )?;
    let profile = prepare(
        &project.source,
        &project.target,
        project
            .extra_targets
            .iter()
            .map(|target| (target.name.as_str(), &target.schema)),
        policy,
    )?;
    single_output(
        &project.target,
        false,
        !matches!(project.root.iteration, ScopeIteration::None),
        CsvJsonBoundaryOwner::Target,
    )?;
    for (index, target) in project.extra_targets.iter().enumerate() {
        single_output(
            &target.schema,
            false,
            !matches!(target.root.iteration, ScopeIteration::None),
            named_owner(index, &target.name),
        )?;
    }
    project_dynamic_documents(&project.root, CsvJsonBoundaryOwner::Target)?;
    for (index, target) in project.extra_targets.iter().enumerate() {
        project_dynamic_documents(&target.root, named_owner(index, &target.name))?;
    }
    Ok(profile)
}

/// Admit every endpoint, then prove ordinary complete Program semantics.
pub fn prepare_csv_json_boundary(
    program: &Program,
    policy: &CsvJsonBoundaryPolicy,
) -> Result<CsvJsonBoundaryProfile, CsvJsonBoundaryError> {
    if program.xml_boundary.is_some() {
        return Err(CsvJsonBoundaryError::ProgramField {
            field: "xml_boundary",
        });
    }
    if !program.extra_sources.is_empty() {
        return Err(CsvJsonBoundaryError::ProgramField {
            field: "extra_sources",
        });
    }
    names(
        program
            .extra_targets
            .iter()
            .map(|target| target.name.as_str()),
        policy,
    )?;
    let profile = prepare(
        &program.source,
        &program.target,
        program
            .extra_targets
            .iter()
            .map(|target| (target.name.as_str(), &target.target)),
        policy,
    )?;
    single_output(
        &program.target,
        program.root.repeating,
        program.root.iteration.is_some(),
        CsvJsonBoundaryOwner::Target,
    )?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        single_output(
            &target.target,
            target.root.repeating,
            target.root.iteration.is_some(),
            named_owner(index, &target.name),
        )?;
    }
    program_dynamic_documents(&program.root, CsvJsonBoundaryOwner::Target)?;
    for (index, target) in program.extra_targets.iter().enumerate() {
        program_dynamic_documents(&target.root, named_owner(index, &target.name))?;
    }
    crate::validate_program(program)
        .map_err(|error| CsvJsonBoundaryError::Validation(Box::new(error)))?;
    Ok(profile)
}
