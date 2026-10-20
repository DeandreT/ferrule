//! Legacy and owned adjacency-tree construction validation. The two forms
//! differ only in where the row collection is resolved: the legacy path uses
//! the parent source schema, while an owned collection uses its exact endpoint.

use ir::{SchemaKind, SchemaNode};
use mapping::{NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use super::ValidationIssue;
use super::graph::validate_adjacency_string_field;
use super::schema::{display_path, follow_schema, owned_source_root, source_schema_at};

/// Row-relative fields and generated target structure shared by both forms.
struct Construction<'a> {
    key: &'a [String],
    parent: &'a [String],
    target_key: &'a str,
    target_children: &'a str,
    root: Option<NodeId>,
}

pub(super) fn validate(
    project: &Project,
    scope: &Scope,
    parent_source: Option<&SchemaNode>,
    target: Option<&SchemaNode>,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let (collection, construction) = match &scope.construction {
        ScopeConstruction::AdjacencyTree { plan } => {
            let collection = source_schema_at(project, parent_source, plan.collection());
            if collection.is_none_or(|node| !repeating_group(node)) {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "adjacency-tree collection `{}` must be a repeating group",
                        display_path(plan.collection())
                    ),
                ));
            }
            (
                collection,
                Construction {
                    key: plan.key(),
                    parent: plan.parent(),
                    target_key: plan.target_key(),
                    target_children: plan.target_children(),
                    root: plan.root(),
                },
            )
        }
        ScopeConstruction::OwnedAdjacencyTree { plan } => {
            // The rows belong to their exact endpoint; the parent source schema
            // and legacy aliases never participate.
            let source = plan.collection();
            let collection = owned_source_root(project, &source.owner, location, issues)
                .map(|root| follow_schema(root, &source.path));
            if let Some(collection) = collection
                && collection.is_none_or(|node| !repeating_group(node))
            {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "owned adjacency-tree collection `{}` must be a repeating group in its owner",
                        source.path.join("/")
                    ),
                ));
            }
            (
                collection.flatten(),
                Construction {
                    key: plan.key(),
                    parent: plan.parent(),
                    target_key: plan.target_key(),
                    target_children: plan.target_children(),
                    root: plan.root(),
                },
            )
        }
        _ => return,
    };
    if let Some(collection) = collection {
        validate_adjacency_string_field(location, collection, construction.key, "key", issues);
        validate_adjacency_string_field(
            location,
            collection,
            construction.parent,
            "parent",
            issues,
        );
    }
    validate_output(project, scope, target, construction, location, issues);
}

fn repeating_group(node: &SchemaNode) -> bool {
    node.repeating && matches!(node.kind, SchemaKind::Group { .. })
}

fn validate_output(
    project: &Project,
    scope: &Scope,
    target: Option<&SchemaNode>,
    construction: Construction<'_>,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Construction {
        target_key,
        target_children,
        root,
        ..
    } = construction;
    if target.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
        issues.push(ValidationIssue::new(
            location,
            "adjacency-tree construction requires a group target schema",
        ));
    }
    if let Some(target) = target {
        if target.child(target_key).is_none_or(|key| {
            key.repeating
                || !matches!(
                    key.kind,
                    SchemaKind::Scalar {
                        ty: ir::ScalarType::String
                    }
                )
        }) {
            issues.push(ValidationIssue::new(
                location,
                format!("adjacency-tree target key `{target_key}` must be a non-repeating string"),
            ));
        }
        if target.child(target_children).is_none_or(|children| {
            !children.repeating
                || !matches!(children.kind, SchemaKind::Group { .. })
                || children.recursive_ref.as_deref() != Some(target.name.as_str())
        }) {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "adjacency-tree child field `{target_children}` must recursively reference `{}`",
                    target.name
                ),
            ));
        }
    }
    if let Some(root) = root
        && !project.graph.nodes.contains_key(&root)
    {
        issues.push(ValidationIssue::new(
            location,
            format!("adjacency-tree root references missing node {root}"),
        ));
    }
    if !(scope.bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields)
    {
        issues.push(ValidationIssue::new(
            location,
            "adjacency-tree construction cannot contain bindings, child scopes, or dynamic target content",
        ));
    }
    if scope.filter.is_some()
        || scope.post_group_filter.is_some()
        || scope.has_grouping()
        || scope.has_sort()
        || !scope.windows.is_empty()
    {
        issues.push(ValidationIssue::new(
            location,
            "adjacency-tree construction cannot use scope controls",
        ));
    }
    if !matches!(&scope.iteration, ScopeIteration::None) {
        issues.push(ValidationIssue::new(
            location,
            "adjacency-tree construction cannot use scope iteration",
        ));
    }
}
