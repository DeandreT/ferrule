use thiserror::Error;

use crate::{
    BoundaryId, DisplayText, InspectionError, InspectionLimits, NativeLocation, Project,
    ProjectFileLayout, ValidationEndpoint, ValidationOwner, ValidationSchemaStep,
    ValidationScopeStep, inspect,
};

/// Limits for structural admission and the returned diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationLimits {
    pub inspection: InspectionLimits,
    /// Maximum number of issues accepted from the engine before producing a report.
    pub maximum_issues: usize,
    /// Maximum number of issues to include in a successful report.
    pub maximum_diagnostics: usize,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            inspection: InspectionLimits::default(),
            maximum_issues: 32_768,
            maximum_diagnostics: 128,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationError {
    #[error(transparent)]
    Inspection(#[from] InspectionError),
    #[error("The validation result exceeds the compiled issue-count limit.")]
    IssueLimitExceeded,
    #[error("A native owner identity exceeds its admitted text limit.")]
    OwnerIdentityTooLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: DisplayText,
    pub location: DisplayText,
    pub owner: Option<ValidationOwner>,
    pub native_location: NativeLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// Based on all issues, including those omitted from `diagnostics`.
    pub is_valid: bool,
    pub total_issue_count: usize,
    /// True if an issue was omitted or any diagnostic display text was shortened.
    pub truncated: bool,
    /// Emitted issues in engine order. Its length is the emitted issue count.
    pub diagnostics: Vec<Diagnostic>,
}

/// Admit the project's structure, then run validation in engine order.
///
/// The layout must describe the file whose native locations the caller will use.
pub fn validate(
    project: &Project,
    layout: ProjectFileLayout,
    limits: ValidationLimits,
) -> Result<ValidationReport, ValidationError> {
    inspect(project, layout, limits.inspection)?;
    let issues = engine::validate(project);
    let total_issue_count = issues.len();
    if total_issue_count > limits.maximum_issues {
        return Err(ValidationError::IssueLimitExceeded);
    }
    let maximum_text = limits.inspection.maximum_text_bytes;
    // Owners are identities. Check every one before selecting the emitted subset.
    if issues
        .iter()
        .filter_map(|issue| issue.owner.as_ref())
        .any(|owner| {
            matches!(
                endpoint_of(owner),
                Some(ValidationEndpoint::NamedSource { name, .. }
                    | ValidationEndpoint::NamedTarget { name, .. }) if name.len() > maximum_text
            )
        })
    {
        return Err(ValidationError::OwnerIdentityTooLong);
    }

    let diagnostics: Vec<_> = issues
        .into_iter()
        .take(limits.maximum_diagnostics)
        .map(|issue| Diagnostic {
            native_location: issue
                .owner
                .as_ref()
                .map_or(NativeLocation::Document, |owner| {
                    native_location(owner, layout, maximum_text)
                }),
            location: DisplayText::bounded(&issue.location, maximum_text),
            message: DisplayText::bounded(&issue.message, maximum_text),
            owner: issue.owner,
        })
        .collect();
    let truncated = diagnostics.len() < total_issue_count
        || diagnostics
            .iter()
            .any(|issue| issue.location.truncated || issue.message.truncated);
    Ok(ValidationReport {
        is_valid: total_issue_count == 0,
        total_issue_count,
        truncated,
        diagnostics,
    })
}

fn endpoint_of(owner: &ValidationOwner) -> Option<&ValidationEndpoint> {
    match owner {
        ValidationOwner::Endpoint(endpoint) => Some(endpoint),
        ValidationOwner::SchemaNode(location) => Some(&location.endpoint),
        ValidationOwner::Scope(location) => Some(&location.target),
        _ => None,
    }
}

fn native_location(
    owner: &ValidationOwner,
    layout: ProjectFileLayout,
    maximum_bytes: usize,
) -> NativeLocation {
    let path = match owner {
        ValidationOwner::GraphNode { function, node } => {
            return NativeLocation::GraphNode {
                node: *node,
                function: *function,
            };
        }
        ValidationOwner::UserFunction(function) => format!("/user_functions/{}", function.get()),
        ValidationOwner::Endpoint(endpoint) => BoundaryId::from_endpoint(endpoint).pointer(),
        ValidationOwner::FailureRule { index } => format!("/failure_rules/{index}"),
        ValidationOwner::SchemaNode(location) => {
            let boundary = BoundaryId::from_endpoint(&location.endpoint);
            let mut path = boundary.pointer();
            if matches!(
                boundary,
                BoundaryId::NamedSource(_) | BoundaryId::NamedTarget(_)
            ) {
                path.push_str("/schema");
            }
            for step in &location.path {
                match step {
                    ValidationSchemaStep::Child(index) => {
                        path.push_str(&format!("/kind/children/{index}"))
                    }
                    ValidationSchemaStep::DynamicField => path.push_str("/kind/dynamic"),
                    // Predicate steps have their own wire representation.
                    ValidationSchemaStep::Contains(_)
                    | ValidationSchemaStep::DependentSchema(_) => {
                        return NativeLocation::Document;
                    }
                }
            }
            path
        }
        ValidationOwner::Scope(location) => {
            let mut path = match location.target {
                ValidationEndpoint::Target => "/root".to_owned(),
                ValidationEndpoint::NamedTarget { index, .. } => {
                    format!("/extra_targets/{index}/root")
                }
                _ => return NativeLocation::Document,
            };
            for step in &location.path {
                path.push_str(&match step {
                    ValidationScopeStep::Child(index) => format!("/children/{index}"),
                    ValidationScopeStep::DynamicChild(index) => {
                        format!("/dynamic_children/{index}/scope")
                    }
                    ValidationScopeStep::Segment(index) => format!("/segments/{index}"),
                });
            }
            path
        }
    };
    NativeLocation::pointer(layout, &path, maximum_bytes)
}
