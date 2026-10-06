use std::collections::BTreeSet;

use mapping::{Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration};

use super::graph::all_dependencies;
use super::{ValidationIssue, ValidationOwner};

pub(super) fn validate_primary_root_primitives(
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    let primitives: BTreeSet<_> = project
        .graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| {
            matches!(
                node,
                Node::SourceRootXmlTypeEquals { .. } | Node::SourceRootField { .. }
            )
            .then_some(id)
        })
        .collect();
    if primitives.is_empty() {
        return;
    }
    for &id in &primitives {
        let supported = !project.source_options.local_xml_file_set
            && match &project.graph.nodes[&id] {
                Node::SourceRootXmlTypeEquals {
                    canonical_expanded_type,
                } => ir::primary_root_schema_has_type(&project.source, canonical_expanded_type),
                Node::SourceRootField { path, .. } => ir::primary_root_schema_has_scalar(
                    &project.source,
                    &path.iter().map(String::as_str).collect::<Vec<_>>(),
                ),
                _ => unreachable!(),
            };
        if !supported {
            issue(
                id,
                "requires a supported singular primary XsiType root and an exact schema-known ordinary scalar or canonical type",
                issues,
            );
        }
    }
    for (&id, node) in &project.graph.nodes {
        if matches!(
            node,
            Node::SourceRootXmlTypeEquals { .. }
                | Node::SourceRootField { .. }
                | Node::Call { .. }
                | Node::If { .. }
                | Node::ValueMap { .. }
        ) {
            continue;
        }
        reject_dependencies(
            project,
            &primitives,
            node.dependencies(),
            &format!("graph node {id} changes or isolates evaluation ownership"),
            issues,
        );
    }
    validate_scope(project, &primitives, &project.root, true, issues);
    let named_root_allowed = observed_static_named_root_is_supported(project);
    for target in &project.extra_targets {
        validate_scope(
            project,
            &primitives,
            &target.root,
            named_root_allowed,
            issues,
        );
    }
    for rule in &project.failure_rules {
        let roots = rule
            .message
            .into_iter()
            .chain(rule.selection.predicate())
            .chain(match &rule.iteration {
                mapping::FailureIteration::Sequence { sequence } => sequence.inputs(),
                mapping::FailureIteration::Source { .. } => Vec::new(),
            });
        reject_dependencies(
            project,
            &primitives,
            roots,
            "failure-rule evaluation",
            issues,
        );
    }
    for source in &project.extra_sources {
        if let Some(path) = &source.dynamic_path {
            reject_dependencies(
                project,
                &primitives,
                [path.node],
                "dynamic named-source path evaluation",
                issues,
            );
        }
    }
}

// Both flat outputs evaluate with the same explicit primary owner. This does
// not grant ownership to descendants, controls or dynamic boundaries.
fn observed_static_named_root_is_supported(project: &Project) -> bool {
    project.source_options
        == (mapping::FormatOptions {
            xml_document: true,
            xml_allow_inactive_root_type_members: true,
            xml_root_view_read_policy: true,
            ..Default::default()
        })
        && ir::xml_root_view_read_policy_is_supported(&project.source)
        && project.extra_sources.is_empty()
        && project.extra_targets.len() == 1
        && flat_static_group_root(&project.target, &project.root)
        && xml_document_output_options(&project.target_options)
        && project.extra_targets.iter().all(|target| {
            flat_static_group_root(&target.schema, &target.root)
                && xml_document_output_options(&target.options)
        })
}

fn xml_document_output_options(options: &mapping::FormatOptions) -> bool {
    *options
        == mapping::FormatOptions {
            xml_document: true,
            xml_schema_hints: options.xml_schema_hints.clone(),
            ..Default::default()
        }
}

fn flat_static_group_root(schema: &ir::SchemaNode, scope: &Scope) -> bool {
    !schema.repeating
        && matches!(schema.kind, ir::SchemaKind::Group { .. })
        && (scope.target_field.is_empty() || scope.target_field == schema.name)
        && scope.dynamic_bindings.is_empty()
        && flat_static_scope(scope)
}

fn flat_static_scope(scope: &Scope) -> bool {
    matches!(scope.iteration, ScopeIteration::None)
        && matches!(scope.construction, ScopeConstruction::Constructed)
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && !scope.merge_dynamic_fields
        && scope.children.is_empty()
        && scope.dynamic_children.is_empty()
        && scope.concatenated().is_none()
}

fn validate_scope(
    project: &Project,
    primitives: &BTreeSet<NodeId>,
    scope: &Scope,
    root_binding_allowed: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    let allowed = root_binding_allowed
        && !project.target.repeating
        && matches!(project.target.kind, ir::SchemaKind::Group { .. })
        && flat_static_scope(scope)
        && project
            .extra_sources
            .iter()
            .all(|source| source.dynamic_path.is_none());
    if !allowed {
        reject_dependencies(
            project,
            primitives,
            scope.bindings.iter().map(|binding| binding.node),
            "non-flat, descendant or named-target binding",
            issues,
        );
    }
    let mut controls: Vec<_> = scope
        .filter
        .into_iter()
        .chain(scope.post_group_filter)
        .chain(scope.grouping_nodes())
        .chain(scope.sort_keys().map(|key| key.node))
        .chain(scope.output_path())
        .chain(
            scope
                .windows
                .iter()
                .copied()
                .flat_map(|window| window.nodes()),
        )
        .collect();
    if let Some(sequence) = scope.sequence() {
        controls.extend(sequence.inputs());
    }
    match &scope.construction {
        ScopeConstruction::Scalar { value } => controls.push(*value),
        ScopeConstruction::RecursiveFilter { plan } => controls.push(plan.predicate()),
        ScopeConstruction::AdjacencyTree { plan } => controls.extend(plan.root()),
        ScopeConstruction::Constructed
        | ScopeConstruction::CopyCurrentSource
        | ScopeConstruction::XmlMixedContent { .. }
        | ScopeConstruction::PathHierarchy { .. } => {}
    }
    reject_dependencies(
        project,
        primitives,
        controls,
        "scope control or construction expression",
        issues,
    );
    for binding in &scope.dynamic_bindings {
        reject_dependencies(
            project,
            primitives,
            [binding.key, binding.value],
            "dynamic binding",
            issues,
        );
    }
    for child in &scope.children {
        validate_scope(project, primitives, child, false, issues);
    }
    for child in &scope.dynamic_children {
        reject_dependencies(
            project,
            primitives,
            [child.key],
            "dynamic child name",
            issues,
        );
        validate_scope(project, primitives, &child.scope, false, issues);
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            validate_scope(project, primitives, segment, false, issues);
        }
    }
}

fn reject_dependencies(
    project: &Project,
    primitives: &BTreeSet<NodeId>,
    roots: impl IntoIterator<Item = NodeId>,
    context: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    for id in all_dependencies(&project.graph, roots).intersection(primitives) {
        issue(
            *id,
            &format!(
                "is unavailable in {context}; only flat primary target root static bindings are proved"
            ),
            issues,
        );
    }
}

fn issue(id: NodeId, message: &str, issues: &mut Vec<ValidationIssue>) {
    issues.push(
        ValidationIssue::new(
            format!("graph node {id}"),
            format!("primary-root primitive {message}"),
        )
        .with_owner(ValidationOwner::GraphNode {
            function: None,
            node: id,
        }),
    );
}
