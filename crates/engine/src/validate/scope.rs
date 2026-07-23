use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, XML_TYPE_FIELD};
use mapping::{IterationOutput, JoinId, Node, Project, Scope, ScopeConstruction, ScopeIteration};

use super::ValidationIssue;
use super::graph::validate_adjacency_string_field;
use super::join::{
    validate_plan as validate_join_plan, validate_roots as validate_join_roots,
    validate_scope_nodes as validate_scope_join_nodes,
};
use super::schema::{current_source_schema, display_path, source_path_matches, source_schema_at};

#[derive(Clone, Copy)]
pub(super) struct ScopeSchemas<'a> {
    pub(super) target: Option<&'a SchemaNode>,
    pub(super) parent_source: Option<&'a SchemaNode>,
}

pub(super) fn validate_scope(
    project: &Project,
    scope: &Scope,
    schemas: ScopeSchemas<'_>,
    path: &mut Vec<String>,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    join_owners: &mut BTreeMap<JoinId, String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let target = schemas.target;
    let location = if path.is_empty() {
        "root scope".to_string()
    } else {
        format!("scope `{}`", path.join("/"))
    };

    if let Some(node) = scope.output_path() {
        if !path.is_empty() {
            issues.push(ValidationIssue::new(
                &location,
                "dynamic target paths are valid only on a project root scope",
            ));
        }
        if !project.graph.nodes.contains_key(&node) {
            issues.push(ValidationIssue::new(
                &location,
                format!("dynamic target path references missing node {node}"),
            ));
        }
        if !scope.iterates() {
            issues.push(ValidationIssue::new(
                &location,
                "dynamic target paths require an iterating scope",
            ));
        }
        if scope.iteration_output != IterationOutput::Repeated {
            issues.push(ValidationIssue::new(
                &location,
                "dynamic target paths require repeated iteration output",
            ));
        }
        if scope.concatenated().is_some() {
            issues.push(ValidationIssue::new(
                &location,
                "dynamic target paths cannot be combined with concatenated scope segments",
            ));
        }
    }

    if let Some(segments) = scope.concatenated() {
        if schemas
            .target
            .is_none_or(|target| !matches!(target.kind, SchemaKind::Group { .. }))
        {
            issues.push(ValidationIssue::new(
                &location,
                "concatenated scope requires a group target schema",
            ));
        }
        if scope.iteration_output == IterationOutput::MappedSequence
            && (path.is_empty() || schemas.target.is_some_and(|target| target.repeating))
        {
            issues.push(ValidationIssue::new(
                &location,
                "concatenated mapped-sequence output requires a non-root, non-repeating target group",
            ));
        }
        if scope.construction != ScopeConstruction::Constructed
            || scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.has_grouping()
            || scope.sort_by.is_some()
            || !scope.windows.is_empty()
            || scope.iteration_output == IterationOutput::First
            || !scope.bindings.is_empty()
            || !scope.dynamic_bindings.is_empty()
            || !scope.children.is_empty()
            || !scope.dynamic_children.is_empty()
            || scope.merge_dynamic_fields
        {
            issues.push(ValidationIssue::new(
                &location,
                "concatenated scope wrapper cannot contain construction, controls, bindings, or child content",
            ));
        }
        for (index, segment) in segments.iter().enumerate() {
            if !segment.target_field.is_empty() {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "concatenated scope segment {} must have an empty target field",
                        index + 1
                    ),
                ));
            }
            if segment.iteration_output != scope.iteration_output {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "concatenated scope segment {} output kind does not match its wrapper",
                        index + 1
                    ),
                ));
            }
            path.push(format!("<segment {}>", index + 1));
            validate_scope(
                project,
                segment,
                ScopeSchemas {
                    target: schemas.target,
                    parent_source: schemas.parent_source,
                },
                path,
                active_joins,
                join_owners,
                issues,
            );
            path.pop();
        }
        return;
    }
    let current_source = current_source_schema(project, schemas.parent_source, &scope.iteration);

    if let ScopeConstruction::Scalar { value } = &scope.construction {
        if target.is_none_or(|node| !matches!(node.kind, SchemaKind::Scalar { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "scalar construction requires a scalar target schema",
            ));
        }
        if !project.graph.nodes.contains_key(value) {
            issues.push(ValidationIssue::new(
                &location,
                format!("scalar construction references missing node {value}"),
            ));
        }
        if !(scope.bindings.is_empty()
            && scope.children.is_empty()
            && scope.dynamic_bindings.is_empty()
            && scope.dynamic_children.is_empty()
            && !scope.merge_dynamic_fields)
        {
            issues.push(ValidationIssue::new(
                &location,
                "scalar construction cannot contain bindings, child scopes, or dynamic target content",
            ));
        }
    }

    if let ScopeConstruction::XmlMixedContent { elements } = &scope.construction {
        if target.is_none_or(|node| {
            !matches!(node.kind, SchemaKind::Group { .. }) || node.text_child().is_none()
        }) {
            issues.push(ValidationIssue::new(
                &location,
                "XML mixed-content construction requires a group target with a text field",
            ));
        }
        if current_source.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "XML mixed-content construction requires a group source item",
            ));
        }
        if elements.is_empty() {
            issues.push(ValidationIssue::new(
                &location,
                "XML mixed-content construction requires at least one child mapping",
            ));
        }
        let mut source_names = BTreeSet::new();
        for element in elements {
            if element.source.is_empty()
                || element.target.is_empty()
                || !source_names.insert(&element.source)
            {
                issues.push(ValidationIssue::new(
                    &location,
                    "XML mixed-content child mappings require unique non-empty source names and non-empty target names",
                ));
                break;
            }
            if target
                .and_then(|node| node.child(&element.target))
                .is_none_or(|node| {
                    !node.repeating || !matches!(node.kind, SchemaKind::Scalar { .. })
                })
            {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "XML mixed-content target `{}` must be a repeating scalar field",
                        element.target
                    ),
                ));
            }
        }
    }

    if matches!(&scope.construction, ScopeConstruction::CopyCurrentSource) {
        if target.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction requires a group target schema",
            ));
        }
        if current_source.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction requires a group source item",
            ));
        }
        if let (Some(source), Some(target)) = (current_source, target)
            && matches!(source.kind, SchemaKind::Group { .. })
            && matches!(target.kind, SchemaKind::Group { .. })
            && source.kind != target.kind
        {
            issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction requires matching source and target group fields",
            ));
        }
        if !(scope.bindings.is_empty()
            && scope.children.is_empty()
            && scope.dynamic_bindings.is_empty()
            && scope.dynamic_children.is_empty()
            && !scope.merge_dynamic_fields)
        {
            issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction cannot contain bindings, child scopes, or dynamic target content",
            ));
        }
        if scope.has_grouping() {
            issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction cannot use grouping controls",
            ));
        }
        match &scope.iteration {
            ScopeIteration::Sequence(_) => issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction cannot iterate a generated sequence",
            )),
            ScopeIteration::InnerJoin { .. } => issues.push(ValidationIssue::new(
                &location,
                "copy-current-source construction cannot iterate an inner join",
            )),
            ScopeIteration::Concatenate(_) => unreachable!("handled above"),
            ScopeIteration::None
            | ScopeIteration::Source(_)
            | ScopeIteration::DynamicDocuments { .. } => {}
        }
    }

    if let ScopeConstruction::RecursiveFilter { plan } = &scope.construction {
        if target.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction requires a group target schema",
            ));
        }
        if current_source.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction requires a group source item",
            ));
        }
        if let (Some(source), Some(target)) = (current_source, target)
            && matches!(source.kind, SchemaKind::Group { .. })
            && matches!(target.kind, SchemaKind::Group { .. })
            && source.kind != target.kind
        {
            issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction requires matching source and target group fields",
            ));
        }
        if let Some(source) = current_source {
            if source.child(plan.children()).is_none_or(|child| {
                !child.repeating
                    || child.recursive_ref.is_none()
                    || !matches!(child.kind, SchemaKind::Group { .. })
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "recursive-filter child field `{}` must be a repeating recursive group",
                        plan.children()
                    ),
                ));
            }
            if source.child(plan.items()).is_none_or(|item| {
                !item.repeating || !matches!(item.kind, SchemaKind::Group { .. })
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "recursive-filter item field `{}` must be a repeating group",
                        plan.items()
                    ),
                ));
            }
        }
        if !project.graph.nodes.contains_key(&plan.predicate()) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "recursive-filter predicate references missing node {}",
                    plan.predicate()
                ),
            ));
        }
        if !(scope.bindings.is_empty()
            && scope.children.is_empty()
            && scope.dynamic_bindings.is_empty()
            && scope.dynamic_children.is_empty()
            && !scope.merge_dynamic_fields)
        {
            issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction cannot contain bindings, child scopes, or dynamic target content",
            ));
        }
        if scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.has_grouping()
            || scope.sort_by.is_some()
            || !scope.windows.is_empty()
        {
            issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction cannot use scope controls",
            ));
        }
        match &scope.iteration {
            ScopeIteration::Sequence(_) => issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction cannot iterate a generated sequence",
            )),
            ScopeIteration::InnerJoin { .. } => issues.push(ValidationIssue::new(
                &location,
                "recursive-filter construction cannot iterate an inner join",
            )),
            ScopeIteration::Concatenate(_) => unreachable!("handled above"),
            ScopeIteration::None
            | ScopeIteration::Source(_)
            | ScopeIteration::DynamicDocuments { .. } => {}
        }
    }

    if let ScopeConstruction::PathHierarchy { plan } = &scope.construction {
        let collection = source_schema_at(project, schemas.parent_source, plan.collection());
        if collection
            .is_none_or(|node| !node.repeating || !matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "path-hierarchy collection `{}` must be a repeating scalar",
                    display_path(plan.collection())
                ),
            ));
        }
        if target.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "path-hierarchy construction requires a group target schema",
            ));
        }
        if let Some(target) = target {
            if target.child(plan.name()).is_none_or(|name| {
                name.repeating || !matches!(name.kind, SchemaKind::Scalar { .. })
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "path-hierarchy name field `{}` must be a non-repeating scalar",
                        plan.name()
                    ),
                ));
            }
            if target.child(plan.files()).is_none_or(|files| {
                !files.repeating
                    || !matches!(files.kind, SchemaKind::Group { .. })
                    || files.child(plan.name()).is_none_or(|name| {
                        name.repeating || !matches!(name.kind, SchemaKind::Scalar { .. })
                    })
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "path-hierarchy file field `{}` must be a repeating group with scalar `{}`",
                        plan.files(),
                        plan.name()
                    ),
                ));
            }
            if target.child(plan.directories()).is_none_or(|directories| {
                !directories.repeating
                    || directories.recursive_ref.as_deref() != Some(target.name.as_str())
                    || !matches!(directories.kind, SchemaKind::Group { .. })
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "path-hierarchy directory field `{}` must recursively reference `{}`",
                        plan.directories(),
                        target.name
                    ),
                ));
            }
        }
        if !(scope.bindings.is_empty()
            && scope.children.is_empty()
            && scope.dynamic_bindings.is_empty()
            && scope.dynamic_children.is_empty()
            && !scope.merge_dynamic_fields)
        {
            issues.push(ValidationIssue::new(
                &location,
                "path-hierarchy construction cannot contain bindings, child scopes, or dynamic target content",
            ));
        }
        if scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.has_grouping()
            || scope.sort_by.is_some()
            || !scope.windows.is_empty()
        {
            issues.push(ValidationIssue::new(
                &location,
                "path-hierarchy construction cannot use scope controls",
            ));
        }
        if !matches!(&scope.iteration, ScopeIteration::None) {
            issues.push(ValidationIssue::new(
                &location,
                "path-hierarchy construction cannot use scope iteration",
            ));
        }
    }

    if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction {
        let collection = source_schema_at(project, schemas.parent_source, plan.collection());
        if collection
            .is_none_or(|node| !node.repeating || !matches!(node.kind, SchemaKind::Group { .. }))
        {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "adjacency-tree collection `{}` must be a repeating group",
                    display_path(plan.collection())
                ),
            ));
        }
        if let Some(collection) = collection {
            validate_adjacency_string_field(&location, collection, plan.key(), "key", issues);
            validate_adjacency_string_field(&location, collection, plan.parent(), "parent", issues);
        }
        if target.is_none_or(|node| !matches!(node.kind, SchemaKind::Group { .. })) {
            issues.push(ValidationIssue::new(
                &location,
                "adjacency-tree construction requires a group target schema",
            ));
        }
        if let Some(target) = target {
            if target.child(plan.target_key()).is_none_or(|key| {
                key.repeating
                    || !matches!(
                        key.kind,
                        SchemaKind::Scalar {
                            ty: ir::ScalarType::String
                        }
                    )
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "adjacency-tree target key `{}` must be a non-repeating string",
                        plan.target_key()
                    ),
                ));
            }
            if target.child(plan.target_children()).is_none_or(|children| {
                !children.repeating
                    || !matches!(children.kind, SchemaKind::Group { .. })
                    || children.recursive_ref.as_deref() != Some(target.name.as_str())
            }) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "adjacency-tree child field `{}` must recursively reference `{}`",
                        plan.target_children(),
                        target.name
                    ),
                ));
            }
        }
        if let Some(root) = plan.root()
            && !project.graph.nodes.contains_key(&root)
        {
            issues.push(ValidationIssue::new(
                &location,
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
                &location,
                "adjacency-tree construction cannot contain bindings, child scopes, or dynamic target content",
            ));
        }
        if scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.has_grouping()
            || scope.sort_by.is_some()
            || !scope.windows.is_empty()
        {
            issues.push(ValidationIssue::new(
                &location,
                "adjacency-tree construction cannot use scope controls",
            ));
        }
        if !matches!(&scope.iteration, ScopeIteration::None) {
            issues.push(ValidationIssue::new(
                &location,
                "adjacency-tree construction cannot use scope iteration",
            ));
        }
    }

    if let Some(source) = scope.source()
        && !source_path_matches(project, source, |_| true)
    {
        issues.push(ValidationIssue::new(
            &location,
            format!("source path `{}` does not exist", display_path(source)),
        ));
    }
    if let Some(sequence) = scope.sequence() {
        let mut references = match sequence {
            mapping::SequenceExpr::Tokenize {
                input, delimiter, ..
            } => vec![
                ("sequence input", *input),
                ("sequence parameter", *delimiter),
            ],
            mapping::SequenceExpr::TokenizeByLength { input, length, .. } => {
                vec![("sequence input", *input), ("sequence parameter", *length)]
            }
            mapping::SequenceExpr::TokenizeRegex {
                input,
                pattern,
                flags,
                ..
            } => [
                Some(("sequence input", *input)),
                Some(("sequence pattern", *pattern)),
                flags.map(|node| ("sequence flags", node)),
            ]
            .into_iter()
            .flatten()
            .collect(),
            mapping::SequenceExpr::Generate { from, to, .. } => from
                .iter()
                .map(|&node| ("sequence lower boundary", node))
                .chain([("sequence upper boundary", *to)])
                .collect(),
            mapping::SequenceExpr::RecursiveCollect {
                prefix, separator, ..
            } => vec![
                ("recursive sequence prefix", *prefix),
                ("recursive sequence separator", *separator),
            ],
        };
        references.push(("sequence item", sequence.item()));
        for (label, node) in references {
            if !project.graph.nodes.contains_key(&node) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!("{label} references missing node {node}"),
                ));
            }
        }
        if let Some(node) = project.graph.nodes.get(&sequence.item())
            && !matches!(node, Node::SourceField { path, frame: None } if path.is_empty())
        {
            issues.push(ValidationIssue::new(
                &location,
                "sequence item must reference an unframed empty-path source field",
            ));
        }
    }
    let mut parent_roots = scope
        .windows
        .iter()
        .copied()
        .flat_map(|window| window.nodes())
        .collect::<Vec<_>>();
    if let Some(sequence) = scope.sequence() {
        parent_roots.extend(sequence.inputs());
    }
    validate_join_roots(
        &project.graph,
        parent_roots,
        active_joins,
        &location,
        project,
        issues,
    );
    let mut active_joins = active_joins.to_vec();
    if let Some((join, plan)) = scope.join() {
        validate_join_plan(project, join, plan, &location, issues);
        if let Some(first) = join_owners.insert(join, location.clone()) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "join id {} is already owned by {first}; each join scope requires a unique id",
                    join.get()
                ),
            ));
        }
        active_joins.push((
            join,
            plan.sources()
                .map(|source| source.collection().to_vec())
                .collect(),
        ));
        if scope.has_grouping() {
            issues.push(ValidationIssue::new(
                &location,
                "inner join iteration cannot be combined with grouping controls",
            ));
        }
    }
    validate_scope_join_nodes(
        &project.graph,
        scope,
        &active_joins,
        &location,
        project,
        issues,
    );
    for (label, node) in [
        ("filter", scope.filter),
        ("post-group filter", scope.post_group_filter),
        ("group-by key", scope.group_by),
        ("group-adjacent-by key", scope.group_adjacent_by),
        ("group-starting-with predicate", scope.group_starting_with),
        ("group-ending-with predicate", scope.group_ending_with),
        ("group block size", scope.group_into_blocks),
        ("sort key", scope.sort_by),
        ("dynamic target path", scope.output_path()),
    ] {
        if let Some(node) = node
            && !project.graph.nodes.contains_key(&node)
        {
            issues.push(ValidationIssue::new(
                &location,
                format!("{label} references missing node {node}"),
            ));
        }
    }
    for (index, window) in scope.windows.iter().copied().enumerate() {
        for node in window.nodes() {
            if !project.graph.nodes.contains_key(&node) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "sequence window {} references missing bound node {node}",
                        index + 1
                    ),
                ));
            }
        }
    }
    for (index, key) in scope.sort_then_by.iter().enumerate() {
        if !project.graph.nodes.contains_key(&key.node) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "secondary sort key {} references missing node {}",
                    index + 1,
                    key.node
                ),
            ));
        }
    }
    if scope.sort_by.is_none() && !scope.sort_then_by.is_empty() {
        issues.push(ValidationIssue::new(
            &location,
            "secondary sort keys require a primary sort key",
        ));
    }
    let iterates = scope.iterates();
    if scope.iteration_output == IterationOutput::First && !iterates {
        issues.push(ValidationIssue::new(
            &location,
            "first-item output requires an iterated source",
        ));
    }
    if scope.iteration_output == IterationOutput::First && scope.merge_dynamic_fields {
        issues.push(ValidationIssue::new(
            &location,
            "first-item output cannot be combined with dynamic object merge",
        ));
    }
    if scope.iteration_output == IterationOutput::MappedSequence && !iterates {
        issues.push(ValidationIssue::new(
            &location,
            "mapped-sequence output requires an iterated source",
        ));
    }
    if scope.iteration_output == IterationOutput::MappedSequence && path.is_empty() {
        issues.push(ValidationIssue::new(
            &location,
            "mapped-sequence output is not valid for the project root scope",
        ));
    }
    if scope.iteration_output == IterationOutput::MappedSequence && scope.merge_dynamic_fields {
        issues.push(ValidationIssue::new(
            &location,
            "mapped-sequence output cannot be combined with dynamic object merge",
        ));
    }
    if scope.iteration_output == IterationOutput::MappedSequence
        && target
            .is_some_and(|node| node.repeating || !matches!(node.kind, SchemaKind::Group { .. }))
    {
        issues.push(ValidationIssue::new(
            &location,
            "mapped-sequence output requires a non-repeating target group schema",
        ));
    }
    if scope.iteration_output == IterationOutput::First
        && target
            .is_some_and(|node| node.repeating || !matches!(node.kind, SchemaKind::Group { .. }))
    {
        issues.push(ValidationIssue::new(
            &location,
            "first-item output requires a non-repeating target group schema",
        ));
    }
    if !iterates && scope.filter.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "filter has no iterated source",
        ));
    }
    if scope.post_group_filter.is_some() && !scope.has_grouping() {
        issues.push(ValidationIssue::new(
            &location,
            "post-group filter requires one grouping control",
        ));
    }
    if !iterates && scope.group_by.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-by key has no iterated source",
        ));
    }
    if !iterates && scope.group_adjacent_by.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-adjacent-by key has no iterated source",
        ));
    }
    if !iterates && scope.group_starting_with.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-starting-with predicate has no iterated source",
        ));
    }
    if !iterates && scope.group_ending_with.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-ending-with predicate has no iterated source",
        ));
    }
    if !iterates && scope.group_into_blocks.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group block size has no iterated source",
        ));
    }
    if scope.has_conflicting_grouping() {
        issues.push(ValidationIssue::new(
            &location,
            "scope grouping modes are mutually exclusive",
        ));
    }
    if !iterates && scope.has_sort() {
        issues.push(ValidationIssue::new(
            &location,
            "sort key has no iterated source",
        ));
    }
    if !iterates && !scope.windows.is_empty() {
        issues.push(ValidationIssue::new(
            &location,
            "sequence window has no iterated source",
        ));
    }
    if scope.merge_dynamic_fields && !iterates {
        issues.push(ValidationIssue::new(
            &location,
            "dynamic object merge requires an iterated source",
        ));
    }
    if scope.merge_dynamic_fields && !(scope.bindings.is_empty() && scope.children.is_empty()) {
        issues.push(ValidationIssue::new(
            &location,
            "dynamic object merge accepts only computed properties",
        ));
    }
    if scope.merge_dynamic_fields
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
    {
        issues.push(ValidationIssue::new(
            &location,
            "dynamic object merge requires at least one computed property",
        ));
    }
    if (scope.merge_dynamic_fields
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty())
        && target.and_then(SchemaNode::dynamic_fields).is_none()
    {
        issues.push(ValidationIssue::new(
            &location,
            "computed target properties require an open target group schema",
        ));
    }

    let mut bound_fields = BTreeSet::new();
    for binding in &scope.bindings {
        let duplicate = !bound_fields.insert(&binding.target_field);
        let repeating_scalar = target
            .and_then(|target| target.child(&binding.target_field))
            .is_some_and(|field| {
                field.repeating && matches!(field.kind, SchemaKind::Scalar { .. })
            });
        if duplicate && !repeating_scalar {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "target field `{}` is bound more than once",
                    binding.target_field
                ),
            ));
        }
        if !project.graph.nodes.contains_key(&binding.node) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "binding for `{}` references missing node {}",
                    binding.target_field, binding.node
                ),
            ));
        }
        if let Some(target) = target {
            if binding.target_field == XML_TYPE_FIELD && !target.alternatives().is_empty() {
                continue;
            }
            match target.child(&binding.target_field) {
                Some(field) if matches!(field.kind, SchemaKind::Scalar { .. }) => {}
                Some(_) => issues.push(ValidationIssue::new(
                    &location,
                    format!("binding target `{}` is not a scalar", binding.target_field),
                )),
                None => issues.push(ValidationIssue::new(
                    &location,
                    format!("binding target `{}` does not exist", binding.target_field),
                )),
            }
        }
    }

    let mut child_fields = BTreeSet::new();
    for child in &scope.children {
        if !child_fields.insert(&child.target_field) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "target child scope `{}` occurs more than once",
                    child.target_field
                ),
            ));
        }
        path.push(child.target_field.clone());
        let child_target = target.and_then(|target| target.child(&child.target_field));
        match child_target {
            Some(node)
                if matches!(node.kind, SchemaKind::Group { .. })
                    || matches!(
                        (&node.kind, &child.construction),
                        (SchemaKind::Scalar { .. }, ScopeConstruction::Scalar { .. })
                    ) => {}
            Some(_) => issues.push(ValidationIssue::new(
                format!("scope `{}`", path.join("/")),
                "target scope is not a group",
            )),
            None => issues.push(ValidationIssue::new(
                format!("scope `{}`", path.join("/")),
                "target scope does not exist",
            )),
        }
        validate_scope(
            project,
            child,
            ScopeSchemas {
                target: child_target,
                parent_source: current_source,
            },
            path,
            &active_joins,
            join_owners,
            issues,
        );
        path.pop();
    }
    let dynamic_target = target.and_then(SchemaNode::dynamic_fields);
    for binding in &scope.dynamic_bindings {
        for (label, node) in [
            ("dynamic property key", binding.key),
            ("dynamic property value", binding.value),
        ] {
            if !project.graph.nodes.contains_key(&node) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!("{label} references missing node {node}"),
                ));
            }
        }
        if dynamic_target
            .is_some_and(|node| node.repeating || !matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            issues.push(ValidationIssue::new(
                &location,
                "computed scalar binding requires a non-repeating scalar dynamic field schema",
            ));
        }
    }
    for child in &scope.dynamic_children {
        if !project.graph.nodes.contains_key(&child.key) {
            issues.push(ValidationIssue::new(
                &location,
                format!("dynamic child key references missing node {}", child.key),
            ));
        }
        if child.scope.iteration_output == IterationOutput::MappedSequence {
            issues.push(ValidationIssue::new(
                &location,
                "mapped-sequence output cannot populate a computed target property",
            ));
        }
        if let Some(dynamic_target) = dynamic_target {
            if !matches!(dynamic_target.kind, SchemaKind::Group { .. }) {
                issues.push(ValidationIssue::new(
                    &location,
                    "computed child scope requires a group dynamic field schema",
                ));
            }
            let child_iterates = child.scope.iterates();
            let child_repeats = child_iterates
                && child.scope.iteration_output == IterationOutput::Repeated
                && !child.scope.merge_dynamic_fields;
            if child_repeats != dynamic_target.repeating {
                issues.push(ValidationIssue::new(
                    &location,
                    "computed child scope cardinality does not match the dynamic field schema",
                ));
            }
        }
        path.push("*".to_string());
        validate_scope(
            project,
            &child.scope,
            ScopeSchemas {
                target: dynamic_target,
                parent_source: current_source,
            },
            path,
            &active_joins,
            join_owners,
            issues,
        );
        path.pop();
    }
}
