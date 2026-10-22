//! Owned dynamic drivers: a strict driver endpoint and alias-free references.
//!
//! An owned driver selects one collection of exactly one endpoint. Its loaded
//! documents are reachable only through owned selectors and reads, so a
//! legacy path whose first segment names such a source is refused rather than
//! resolved through name-based fallback.

use mapping::{
    DynamicSourcePath, FailureIteration, NamedSource, Node, Project, Scope, SequenceExpr,
    SourceOwner, SourceSelector, SourceSelectorRef,
};

use super::ValidationIssue;
use super::schema::{display_path, source_path_matches, validate_owned_collection};

/// The dynamic source named exactly `name` when its driver is owned.
pub(crate) fn owned_dynamic<'a>(project: &'a Project, name: &str) -> Option<&'a NamedSource> {
    project.extra_sources.iter().find(|source| {
        source.name == name
            && source
                .dynamic_path
                .as_ref()
                .is_some_and(|dynamic| dynamic.owned_driver().is_some())
    })
}

pub(super) fn validate_driver(
    project: &Project,
    source: &NamedSource,
    dynamic: &DynamicSourcePath,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match &dynamic.iteration {
        SourceSelector::Legacy(path) => {
            if !source_path_matches(project, path, |_| true) {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "dynamic path iteration `{}` matches no source path",
                        display_path(path)
                    ),
                ));
            } else if let Some(owned) = path.first().and_then(|name| owned_dynamic(project, name)) {
                issues.push(owned_alias(location, &owned.name, path));
            }
        }
        SourceSelector::Owned(driver) => {
            validate_owned_collection(project, driver, location, issues);
            let SourceOwner::Named { name } = &driver.owner else {
                return;
            };
            if *name == source.name {
                issues.push(ValidationIssue::new(
                    location,
                    "an owned dynamic driver cannot select its own endpoint",
                ));
            } else if project.extra_sources.iter().any(|candidate| {
                candidate.name == *name
                    && candidate
                        .dynamic_path
                        .as_ref()
                        .is_some_and(|dynamic| dynamic.legacy_driver().is_some())
            }) {
                issues.push(ValidationIssue::new(
                    location,
                    format!("an owned dynamic driver cannot select legacy dynamic source `{name}`"),
                ));
            }
        }
    }
}

fn owned_alias(location: &str, name: &str, path: &[String]) -> ValidationIssue {
    ValidationIssue::new(
        location,
        format!(
            "legacy path `{}` names owned dynamic source `{name}`; use an owned selector",
            display_path(path)
        ),
    )
}

/// Scope selectors, constructions and generated producers of one scope.
pub(super) fn validate_scope(
    project: &Project,
    scope: &Scope,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut legacy = Vec::new();
    match scope.source_selector() {
        Some(SourceSelectorRef::Legacy(path)) => legacy.push(path),
        Some(SourceSelectorRef::Owned(source)) => {
            if let SourceOwner::Named { name } = &source.owner
                && owned_dynamic(project, name).is_some()
                && scope.output_path().is_some()
            {
                issues.push(ValidationIssue::new(
                    location,
                    format!("owned dynamic source `{name}` cannot drive per-item document outputs"),
                ));
            }
        }
        None => {}
    }
    if let Some((_, join)) = scope.join() {
        legacy.extend(join.sources().map(|source| source.collection()));
    }
    match &scope.construction {
        mapping::ScopeConstruction::PathHierarchy { plan } => legacy.push(plan.collection()),
        mapping::ScopeConstruction::AdjacencyTree { plan } => legacy.push(plan.collection()),
        _ => {}
    }
    if let Some(SequenceExpr::RecursiveCollect { collection, .. }) = scope.sequence() {
        legacy.push(collection);
    }
    for path in legacy {
        if let Some(owned) = path.first().and_then(|name| owned_dynamic(project, name)) {
            issues.push(owned_alias(location, &owned.name, path));
        }
    }
}

/// Graph expressions and failure rules naming an owned dynamic source by alias.
pub(super) fn validate_references(project: &Project, issues: &mut Vec<ValidationIssue>) {
    if !project.extra_sources.iter().any(|source| {
        source
            .dynamic_path
            .as_ref()
            .is_some_and(|dynamic| dynamic.owned_driver().is_some())
    }) {
        return;
    }
    for (id, node) in &project.graph.nodes {
        for path in legacy_paths(node) {
            if let Some(owned) = path.first().and_then(|name| owned_dynamic(project, name)) {
                issues.push(owned_alias(&format!("node {id}"), &owned.name, path));
            }
        }
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let path = match &rule.iteration {
            FailureIteration::Source { collection } => Some(collection.as_slice()),
            FailureIteration::Sequence {
                sequence: SequenceExpr::RecursiveCollect { collection, .. },
            } => Some(collection.as_slice()),
            _ => None,
        };
        if let Some(path) = path
            && let Some(owned) = path.first().and_then(|name| owned_dynamic(project, name))
        {
            issues.push(owned_alias(
                &format!("failure rule {}", index + 1),
                &owned.name,
                path,
            ));
        }
    }
}

/// Every legacy endpoint-selecting path of one node.
fn legacy_paths(node: &Node) -> Vec<&[String]> {
    match node {
        Node::SourceField { path, frame }
        | Node::DynamicSourceField {
            object: path,
            frame,
            ..
        }
        | Node::XmlSerialize { path, frame, .. } => std::iter::once(path.as_slice())
            .chain(frame.as_deref())
            .collect(),
        Node::Position { collection }
        | Node::JoinField { collection, .. }
        | Node::Lookup { collection, .. }
        | Node::CollectionFind { collection, .. }
        | Node::Aggregate { collection, .. } => vec![collection],
        Node::XmlMixedContent {
            path,
            frame,
            replacements,
        } => std::iter::once(path.as_slice())
            .chain(frame.as_deref())
            .chain(
                replacements
                    .iter()
                    .map(|replacement| replacement.collection.as_slice()),
            )
            .collect(),
        Node::SequenceExists { sequence, .. }
        | Node::SequenceItemAt { sequence, .. }
        | Node::SequenceAggregate { sequence, .. } => match sequence {
            SequenceExpr::RecursiveCollect { collection, .. } => vec![collection],
            _ => Vec::new(),
        },
        Node::JoinAggregate { plan, .. } => {
            plan.sources().map(|source| source.collection()).collect()
        }
        _ => Vec::new(),
    }
}
