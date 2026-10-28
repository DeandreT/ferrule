use ir::{SchemaNode, flat_row_schema_is_supported};
use mapping::{IterationOutput, Project, Scope, ScopeIteration};

use super::{ValidationEndpoint, ValidationIssue, ValidationScopeLocation};

pub(super) fn validate(
    project: &Project,
    scope: &Scope,
    target: Option<&SchemaNode>,
    owner: &ValidationScopeLocation,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let valid = owner.target == ValidationEndpoint::Target
        && owner.path.is_empty()
        && scope.target_field.is_empty()
        && project.extra_sources.is_empty()
        && project.extra_targets.is_empty()
        && target.is_some_and(flat_row_schema_is_supported)
        && flat_row_schema_is_supported(&project.source)
        && target.is_some_and(|target| target.kind == project.source.kind)
        && matches!(&scope.iteration, ScopeIteration::DynamicDocuments { source, .. } if source.is_empty())
        && scope.iteration_output == IterationOutput::Repeated
        && scope.group_into_blocks.is_some()
        && !scope.has_conflicting_grouping()
        && scope.bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields;
    if !valid {
        issues.push(ValidationIssue::new(location,
            "copy-current-rows requires a primary fixed-block document producer, matching flat row schemas, and no named endpoints or mapped content"));
    }
}
