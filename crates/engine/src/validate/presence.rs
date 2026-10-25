use ir::{SchemaKind, SchemaNode};
use mapping::{IterationOutput, Project, Scope, ScopeConstruction};

use super::ValidationIssue;

/// Optional presence constructs zero or one group in the unchanged parent
/// item. It owns no candidate sequence, so item controls, alternate
/// constructions, and computed target content are refused.
pub(super) fn validate(
    project: &Project,
    scope: &Scope,
    target: Option<&SchemaNode>,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(condition) = scope.presence() else {
        return;
    };
    if !project.graph.nodes.contains_key(&condition) {
        issues.push(ValidationIssue::new(
            location,
            format!("optional presence condition references missing node {condition}"),
        ));
    }
    if target.is_none_or(|node| !node.repeating || !matches!(node.kind, SchemaKind::Group { .. })) {
        issues.push(ValidationIssue::new(
            location,
            "optional presence requires a repeating target group schema",
        ));
    }
    if scope.construction != ScopeConstruction::Constructed
        || scope.iteration_output != IterationOutput::Repeated
    {
        issues.push(ValidationIssue::new(
            location,
            "optional presence requires constructed repeated output",
        ));
    }
    if scope.filter.is_some()
        || scope.post_group_filter.is_some()
        || scope.has_grouping()
        || scope.has_sort()
        || !scope.windows.is_empty()
        || scope.merge_dynamic_fields
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
    {
        issues.push(ValidationIssue::new(
            location,
            "optional presence cannot use item controls or computed target properties",
        ));
    }
}
