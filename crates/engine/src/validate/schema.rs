use ir::{SchemaKind, SchemaNode};
use mapping::{Project, ScopeIteration};

use super::ValidationIssue;

pub(super) fn validate_schema(
    root: &str,
    schema: &SchemaNode,
    path: &mut Vec<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    if !schema.alternatives_are_valid() {
        let suffix = if path.is_empty() {
            String::new()
        } else {
            format!(" at `{}`", path.join("/"))
        };
        issues.push(ValidationIssue::new(
            root,
            format!(
                "group alternative metadata{suffix} has duplicate or unknown names, members, or required fields"
            ),
        ));
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return;
    };
    for child in children {
        path.push(child.name.clone());
        validate_schema(root, child, path, issues);
        path.pop();
    }
    if let Some(dynamic) = schema.dynamic_fields() {
        path.push("*".to_string());
        validate_schema(root, dynamic, path, issues);
        path.pop();
    }
}

pub(super) fn current_source_schema<'a>(
    project: &'a Project,
    parent: Option<&'a SchemaNode>,
    iteration: &ScopeIteration,
) -> Option<&'a SchemaNode> {
    match iteration {
        ScopeIteration::None => parent,
        ScopeIteration::Source(path) | ScopeIteration::DynamicDocuments { source: path, .. } => {
            source_schema_at(project, parent, path)
        }
        ScopeIteration::Sequence(_)
        | ScopeIteration::InnerJoin { .. }
        | ScopeIteration::Concatenate(_) => None,
    }
}

pub(super) fn source_schema_at<'a>(
    project: &'a Project,
    parent: Option<&'a SchemaNode>,
    path: &[String],
) -> Option<&'a SchemaNode> {
    if let Some(node) = parent.and_then(|schema| follow_schema(schema, path)) {
        return Some(node);
    }
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && let Some(node) = follow_schema(&extra.schema, rest)
    {
        return Some(node);
    }
    find_schema_path(&project.source, path).or_else(|| {
        project
            .extra_sources
            .iter()
            .find_map(|source| find_schema_path(&source.schema, path))
    })
}

fn find_schema_path<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    find_schema_path_from(schema, schema, path)
}

fn find_schema_path_from<'a>(
    root: &'a SchemaNode,
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    follow_schema_from(root, schema, path).or_else(|| match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .find_map(|child| find_schema_path_from(root, child, path)),
        SchemaKind::Scalar { .. } => None,
    })
}

pub(super) fn source_path_matches(
    project: &Project,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && follow_schema(&extra.schema, rest).is_some_and(predicate)
    {
        return true;
    }

    any_schema_path(&project.source, path, predicate)
        || project
            .extra_sources
            .iter()
            .any(|source| any_schema_path(&source.schema, path, predicate))
}

pub(super) fn source_path_matches_resolved(
    project: &Project,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if let Some((name, rest)) = path.split_first()
        && let Some(extra) = project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
        && follow_schema_resolved(&extra.schema, rest).is_some_and(predicate)
    {
        return true;
    }
    any_schema_path_resolved(&project.source, path, predicate)
        || project
            .extra_sources
            .iter()
            .any(|source| any_schema_path_resolved(&source.schema, path, predicate))
}

fn any_schema_path_resolved(
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    fn visit(
        root: &SchemaNode,
        schema: &SchemaNode,
        path: &[String],
        predicate: impl Fn(&SchemaNode) -> bool + Copy,
    ) -> bool {
        if follow_schema_from(root, schema, path)
            .and_then(|node| {
                node.recursive_ref
                    .as_deref()
                    .and_then(|anchor| find_concrete_schema_group(root, anchor))
                    .or(Some(node))
            })
            .is_some_and(predicate)
        {
            return true;
        }
        match &schema.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .any(|child| visit(root, child, path, predicate)),
            SchemaKind::Scalar { .. } => false,
        }
    }
    visit(schema, schema, path, predicate)
}

fn follow_schema_resolved<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let node = follow_schema(schema, path)?;
    node.recursive_ref
        .as_deref()
        .and_then(|anchor| find_concrete_schema_group(schema, anchor))
        .or(Some(node))
}

/// SourceField paths are relative to the current scope frame, so a valid
/// path may start at any group in the source tree rather than only its root.
fn any_schema_path(
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    any_schema_path_from(schema, schema, path, predicate)
}

fn any_schema_path_from(
    root: &SchemaNode,
    schema: &SchemaNode,
    path: &[String],
    predicate: impl Fn(&SchemaNode) -> bool + Copy,
) -> bool {
    if follow_schema_from(root, schema, path).is_some_and(predicate) {
        return true;
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .any(|child| any_schema_path_from(root, child, path, predicate)),
        SchemaKind::Scalar { .. } => false,
    }
}

pub(super) fn follow_schema<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    follow_schema_from(schema, schema, path)
}

fn follow_schema_from<'a>(
    root: &'a SchemaNode,
    schema: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = schema;
    for segment in path {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_schema_group(root, anchor)?;
        }
        current = current.child(segment)?;
    }
    Some(current)
}

fn find_concrete_schema_group<'a>(schema: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if schema.recursive_ref.is_none()
        && schema.name == anchor
        && matches!(schema.kind, SchemaKind::Group { .. })
    {
        return Some(schema);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_schema_group(child, anchor))
}

pub(super) fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "<current>".to_string()
    } else {
        path.join("/")
    }
}
