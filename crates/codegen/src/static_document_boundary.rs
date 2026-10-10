//! Complete static JSON/X12 endpoint admission for selected physical output.

use std::collections::BTreeSet;
use std::fmt;

use codegen_schema::CodecError;
use ir::SchemaNode;
use mapping::{Project, ScopeIteration};

use crate::{
    MAX_EMBEDDED_JSON_SCHEMA_BYTES, Program, ProgramValidationError, TargetConstruction,
    TargetScope, X12BoundaryOptions, X12BoundaryPolicyError, X12BoundarySide,
};

#[cfg(test)]
mod tests;

/// A codec selected from one endpoint's explicit physical identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentBoundaryFormat {
    Json,
    X12,
}

/// Retained physical policy for exactly one declared endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentBoundaryOptions {
    Json,
    X12(X12BoundaryOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedDocumentBoundaryOptions {
    pub name: String,
    pub options: DocumentBoundaryOptions,
}

/// Names must equal the complete Program declarations in declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDocumentBoundaryPolicy {
    pub source: DocumentBoundaryOptions,
    pub target: DocumentBoundaryOptions,
    pub extra_sources: Vec<NamedDocumentBoundaryOptions>,
    pub extra_targets: Vec<NamedDocumentBoundaryOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentBoundaryDescriptor {
    pub format: DocumentBoundaryFormat,
    pub descriptor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedDocumentBoundaryDescriptor {
    pub name: String,
    pub boundary: DocumentBoundaryDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDocumentBoundaryProfile {
    pub source: DocumentBoundaryDescriptor,
    pub target: DocumentBoundaryDescriptor,
    pub extra_sources: Vec<NamedDocumentBoundaryDescriptor>,
    pub extra_targets: Vec<NamedDocumentBoundaryDescriptor>,
}

/// Exact endpoint ownership retained around existing codec and X12 causes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticDocumentBoundaryOwner {
    Source,
    NamedSource { index: usize, name: String },
    Target,
    NamedTarget { index: usize, name: String },
}

impl StaticDocumentBoundaryOwner {
    fn side(&self) -> X12BoundarySide {
        match self {
            Self::Source | Self::NamedSource { .. } => X12BoundarySide::Source,
            Self::Target | Self::NamedTarget { .. } => X12BoundarySide::Target,
        }
    }
}

#[derive(Debug)]
pub enum StaticDocumentBoundaryError {
    NoX12Boundary,
    PolicyNames {
        field: &'static str,
    },
    ProgramField {
        field: &'static str,
    },
    EmbeddedSchema {
        owner: StaticDocumentBoundaryOwner,
        error: CodecError,
    },
    X12 {
        owner: StaticDocumentBoundaryOwner,
        error: X12BoundaryPolicyError,
    },
    Validation(ProgramValidationError),
}

impl fmt::Display for StaticDocumentBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoX12Boundary => formatter
                .write_str("static document adapters require at least one declared X12 endpoint"),
            Self::PolicyNames { field } => write!(
                formatter,
                "static document {field} policy names must exactly match declaration order"
            ),
            Self::ProgramField { field } => write!(
                formatter,
                "static document program field {field} is unsupported"
            ),
            Self::EmbeddedSchema { owner, error } => {
                write!(formatter, "static document {owner:?} descriptor: {error}")
            }
            Self::X12 { owner, error } => {
                write!(formatter, "static document {owner:?} X12 boundary: {error}")
            }
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StaticDocumentBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EmbeddedSchema { error, .. } => Some(error),
            Self::X12 { error, .. } => Some(error),
            Self::Validation(error) => Some(error),
            _ => None,
        }
    }
}

struct Endpoint<'a> {
    owner: StaticDocumentBoundaryOwner,
    schema: &'a SchemaNode,
    options: &'a DocumentBoundaryOptions,
}

fn names<'a>(
    declared: impl Iterator<Item = &'a str>,
    policies: &[NamedDocumentBoundaryOptions],
    field: &'static str,
) -> Result<(), StaticDocumentBoundaryError> {
    let declared = declared.collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    if declared.len() != policies.len()
        || declared
            .iter()
            .zip(policies)
            .any(|(name, policy)| *name != policy.name || !seen.insert(policy.name.as_str()))
    {
        return Err(StaticDocumentBoundaryError::PolicyNames { field });
    }
    Ok(())
}

fn endpoints<'a>(
    source: &'a SchemaNode,
    sources: impl Iterator<Item = (&'a str, &'a SchemaNode)>,
    target: &'a SchemaNode,
    targets: impl Iterator<Item = (&'a str, &'a SchemaNode)>,
    policy: &'a StaticDocumentBoundaryPolicy,
) -> Result<Vec<Endpoint<'a>>, StaticDocumentBoundaryError> {
    let sources = sources.collect::<Vec<_>>();
    let targets = targets.collect::<Vec<_>>();
    names(
        sources.iter().map(|(name, _)| *name),
        &policy.extra_sources,
        "extra_sources",
    )?;
    names(
        targets.iter().map(|(name, _)| *name),
        &policy.extra_targets,
        "extra_targets",
    )?;
    let mut result = vec![Endpoint {
        owner: StaticDocumentBoundaryOwner::Source,
        schema: source,
        options: &policy.source,
    }];
    result.extend(
        sources
            .into_iter()
            .zip(&policy.extra_sources)
            .enumerate()
            .map(|(index, ((name, schema), options))| Endpoint {
                owner: StaticDocumentBoundaryOwner::NamedSource {
                    index,
                    name: name.into(),
                },
                schema,
                options: &options.options,
            }),
    );
    result.push(Endpoint {
        owner: StaticDocumentBoundaryOwner::Target,
        schema: target,
        options: &policy.target,
    });
    result.extend(
        targets
            .into_iter()
            .zip(&policy.extra_targets)
            .enumerate()
            .map(|(index, ((name, schema), options))| Endpoint {
                owner: StaticDocumentBoundaryOwner::NamedTarget {
                    index,
                    name: name.into(),
                },
                schema,
                options: &options.options,
            }),
    );
    if !result
        .iter()
        .any(|endpoint| matches!(endpoint.options, DocumentBoundaryOptions::X12(_)))
    {
        return Err(StaticDocumentBoundaryError::NoX12Boundary);
    }
    Ok(result)
}

fn prepare(
    endpoints: Vec<Endpoint<'_>>,
) -> Result<StaticDocumentBoundaryProfile, StaticDocumentBoundaryError> {
    // Every schema is borrowed and bounded before ordinary validation or any
    // recursive schema copy. The codec's exact depth/size causes stay intact.
    let mut encoded = Vec::with_capacity(endpoints.len());
    for endpoint in &endpoints {
        if matches!(endpoint.options, DocumentBoundaryOptions::X12(_)) {
            crate::x12_boundary::preflight_schema_bounds(endpoint.schema, endpoint.owner.side())
                .map_err(|error| StaticDocumentBoundaryError::X12 {
                    owner: endpoint.owner.clone(),
                    error,
                })?;
        }
        encoded.push(
            codegen_schema::encode(endpoint.schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).map_err(
                |error| StaticDocumentBoundaryError::EmbeddedSchema {
                    owner: endpoint.owner.clone(),
                    error,
                },
            )?,
        );
    }
    let mut source = None;
    let mut target = None;
    let mut extra_sources = Vec::new();
    let mut extra_targets = Vec::new();
    for (endpoint, descriptor) in endpoints.into_iter().zip(encoded) {
        let boundary = match endpoint.options {
            DocumentBoundaryOptions::Json => DocumentBoundaryDescriptor {
                format: DocumentBoundaryFormat::Json,
                descriptor,
            },
            DocumentBoundaryOptions::X12(options) => DocumentBoundaryDescriptor {
                format: DocumentBoundaryFormat::X12,
                descriptor: crate::x12_boundary::descriptor(
                    endpoint.schema,
                    Some(options),
                    endpoint.owner.side(),
                )
                .map_err(|error| StaticDocumentBoundaryError::X12 {
                    owner: endpoint.owner.clone(),
                    error,
                })?,
            },
        };
        match endpoint.owner {
            StaticDocumentBoundaryOwner::Source => source = Some(boundary),
            StaticDocumentBoundaryOwner::Target => target = Some(boundary),
            StaticDocumentBoundaryOwner::NamedSource { name, .. } => {
                extra_sources.push(NamedDocumentBoundaryDescriptor { name, boundary })
            }
            StaticDocumentBoundaryOwner::NamedTarget { name, .. } => {
                extra_targets.push(NamedDocumentBoundaryDescriptor { name, boundary })
            }
        }
    }
    Ok(StaticDocumentBoundaryProfile {
        source: source.expect("the complete borrowed endpoint table contains its primary source"),
        target: target.expect("the complete borrowed endpoint table contains its primary target"),
        extra_sources,
        extra_targets,
    })
}

fn single_output(
    schema: &SchemaNode,
    repeating: bool,
    iterating: bool,
) -> Result<(), StaticDocumentBoundaryError> {
    if schema.repeating || repeating || iterating {
        return Err(StaticDocumentBoundaryError::ProgramField {
            field: "document_iteration",
        });
    }
    Ok(())
}

/// Borrow and admit complete Project descriptors before ordinary lowering can
/// recursively validate or copy caller-constructed schemas. Graph validation
/// still belongs to lowering; this function executes no mapping.
pub fn prepare_static_document_project_boundaries(
    project: &Project,
    policy: &StaticDocumentBoundaryPolicy,
) -> Result<StaticDocumentBoundaryProfile, StaticDocumentBoundaryError> {
    let table = endpoints(
        &project.source,
        project
            .extra_sources
            .iter()
            .map(|side| (side.name.as_str(), &side.schema)),
        &project.target,
        project
            .extra_targets
            .iter()
            .map(|side| (side.name.as_str(), &side.schema)),
        policy,
    )?;
    if project
        .extra_sources
        .iter()
        .any(|source| source.dynamic_path.is_some())
    {
        return Err(StaticDocumentBoundaryError::ProgramField {
            field: "dynamic_source",
        });
    }
    single_output(
        &project.target,
        false,
        !matches!(project.root.iteration, ScopeIteration::None),
    )?;
    for target in &project.extra_targets {
        single_output(
            &target.schema,
            false,
            !matches!(target.root.iteration, ScopeIteration::None),
        )?;
    }
    prepare(table)
}

fn no_dynamic_documents(root: &TargetScope) -> Result<(), StaticDocumentBoundaryError> {
    let mut pending = vec![root];
    while let Some(scope) = pending.pop() {
        if let Some(iteration) = &scope.iteration {
            if iteration.dynamic_document_iteration().is_some() {
                return Err(StaticDocumentBoundaryError::ProgramField {
                    field: "dynamic_target",
                });
            }
            if let Some(segments) = iteration.concatenated() {
                pending.extend(segments.iter());
            }
        }
        pending.extend(scope.children.iter());
        if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
            pending.extend(children.iter().map(|child| &child.scope));
        }
    }
    Ok(())
}

/// Prepare every own descriptor and then prove complete ordinary mapping
/// semantics. Unused sources and unselected targets remain mandatory endpoints.
pub fn prepare_static_document_boundary(
    program: &Program,
    policy: &StaticDocumentBoundaryPolicy,
) -> Result<StaticDocumentBoundaryProfile, StaticDocumentBoundaryError> {
    let table = endpoints(
        &program.source,
        program
            .extra_sources
            .iter()
            .map(|side| (side.name.as_str(), &side.source)),
        &program.target,
        program
            .extra_targets
            .iter()
            .map(|side| (side.name.as_str(), &side.target)),
        policy,
    )?;
    if program.xml_boundary.is_some() {
        return Err(StaticDocumentBoundaryError::ProgramField {
            field: "xml_boundary",
        });
    }
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some())
    {
        return Err(StaticDocumentBoundaryError::ProgramField {
            field: "dynamic_source",
        });
    }
    single_output(
        &program.target,
        program.root.repeating,
        program.root.iteration.is_some(),
    )?;
    for target in &program.extra_targets {
        single_output(
            &target.target,
            target.root.repeating,
            target.root.iteration.is_some(),
        )?;
    }
    let profile = prepare(table)?;
    no_dynamic_documents(&program.root)?;
    for target in &program.extra_targets {
        no_dynamic_documents(&target.root)?;
    }
    crate::validate_program(program).map_err(StaticDocumentBoundaryError::Validation)?;
    Ok(profile)
}
