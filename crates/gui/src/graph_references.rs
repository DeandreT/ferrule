use mapping::{
    FailureIteration, FailureRule, Graph, NamedSource, NamedTarget, Node, NodeId, Scope,
    ScopeConstruction,
};

/// A different output sharing the active canvas's graph. Only its name and
/// scope are borrowed; boundary schemas and format settings are not copied.
#[derive(Clone, Copy)]
pub(crate) struct InactiveTargetScope<'a> {
    name: Option<&'a str>,
    root: &'a Scope,
}

pub(crate) fn inactive_target_scopes<'a>(
    primary: &'a Scope,
    before: &'a [NamedTarget],
    after: &'a [NamedTarget],
) -> Vec<InactiveTargetScope<'a>> {
    std::iter::once(InactiveTargetScope {
        name: None,
        root: primary,
    })
    .chain(
        before
            .iter()
            .chain(after)
            .map(|target| InactiveTargetScope {
                name: Some(&target.name),
                root: &target.root,
            }),
    )
    .collect()
}

/// Project-level graph ownership is absent on isolated function canvases.
#[derive(Clone, Copy, Default)]
pub(crate) struct ProjectGraphReferences<'a> {
    failure_rules: &'a [FailureRule],
    extra_sources: &'a [NamedSource],
}

impl<'a> ProjectGraphReferences<'a> {
    pub(crate) fn new(failure_rules: &'a [FailureRule], extra_sources: &'a [NamedSource]) -> Self {
        Self {
            failure_rules,
            extra_sources,
        }
    }
}

pub(super) struct NodeReferences {
    pub all: Vec<String>,
    pub blocking: Vec<String>,
}

#[derive(Default)]
struct ReferenceCollector {
    all: std::collections::BTreeSet<String>,
    blocking: std::collections::BTreeSet<String>,
}

impl ReferenceCollector {
    fn add(&mut self, label: String, blocking: bool) {
        if blocking {
            self.blocking.insert(label.clone());
        }
        self.all.insert(label);
    }

    fn finish(self) -> NodeReferences {
        NodeReferences {
            all: self.all.into_iter().collect(),
            blocking: self.blocking.into_iter().collect(),
        }
    }
}

pub(super) fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

pub(super) fn remove_bindings_to(scope: &mut Scope, needle: NodeId) {
    scope.bindings.retain(|binding| binding.node != needle);
    if let Some(segments) = scope.concatenated_mut() {
        for segment in segments.iter_mut() {
            remove_bindings_to(segment, needle);
        }
    }
    for child in &mut scope.children {
        remove_bindings_to(child, needle);
    }
}

pub(super) fn references_to(
    graph: &Graph,
    root_scope: &Scope,
    extra_targets: &[NamedTarget],
    inactive_targets: &[InactiveTargetScope<'_>],
    project: ProjectGraphReferences<'_>,
    needle: NodeId,
) -> NodeReferences {
    fn scope_references(
        scope: &Scope,
        path: &mut Vec<String>,
        needle: NodeId,
        inactive: bool,
        found: &mut ReferenceCollector,
    ) {
        let label = if path.is_empty() {
            "root scope".to_string()
        } else {
            format!("scope {}", path.join("/"))
        };
        for (reference, description) in [
            (scope.filter, "filter"),
            (scope.post_group_filter, "post-group filter"),
            (scope.group_by, "group-by key"),
            (scope.group_adjacent_by, "group-adjacent key"),
            (scope.group_starting_with, "group-starting predicate"),
            (scope.group_ending_with, "group-ending predicate"),
            (scope.group_into_blocks, "group block size"),
            (scope.sort_by, "sort key"),
            (scope.output_path(), "dynamic target path"),
        ] {
            if reference == Some(needle) {
                found.add(format!("{label} {description}"), true);
            }
        }
        for (index, key) in scope.sort_then_by.iter().enumerate() {
            if key.node == needle {
                found.add(format!("{label} sort key {}", index + 2), true);
            }
        }
        for (index, window) in scope.windows.iter().copied().enumerate() {
            if window.nodes().any(|node| node == needle) {
                found.add(format!("{label} sequence window {}", index + 1), true);
            }
        }
        if let Some(sequence) = scope.sequence() {
            if sequence.inputs().contains(&needle) {
                found.add(format!("{label} sequence input"), true);
            }
            if sequence.item() == needle {
                found.add(format!("{label} sequence item"), true);
            }
        }
        if let ScopeConstruction::Scalar { value } = &scope.construction
            && *value == needle
        {
            found.add(format!("{label} scalar value"), true);
        }
        if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction
            && plan.root() == Some(needle)
        {
            found.add(format!("{label} adjacency-tree root"), true);
        }
        for binding in &scope.bindings {
            if binding.node == needle {
                // Reference identity is separate from its display label.
                // Inactive output bindings cannot be disconnected here,
                // even if an active scope uses an identical printable name.
                found.add(
                    format!("{label} binding {}", binding.target_field),
                    inactive,
                );
            }
        }
        for (index, binding) in scope.dynamic_bindings.iter().enumerate() {
            if binding.key == needle {
                found.add(format!("{label} dynamic binding {} key", index + 1), true);
            }
            if binding.value == needle {
                found.add(format!("{label} dynamic binding {} value", index + 1), true);
            }
        }
        if let Some(segments) = scope.concatenated() {
            for (index, segment) in segments.iter().enumerate() {
                path.push(format!("<segment {}>", index + 1));
                scope_references(segment, path, needle, inactive, found);
                path.pop();
            }
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            scope_references(child, path, needle, inactive, found);
            path.pop();
        }
        for (index, child) in scope.dynamic_children.iter().enumerate() {
            if child.key == needle {
                found.add(format!("{label} dynamic child {} key", index + 1), true);
            }
            path.push(format!("<dynamic child {}>", index + 1));
            // Deletion only disconnects bindings in ordinary/concatenated
            // active scopes. Computed child content retains its owner guard.
            scope_references(&child.scope, path, needle, true, found);
            path.pop();
        }
    }

    let mut found = ReferenceCollector::default();
    for (&owner, node) in &graph.nodes {
        if owner != needle && node_inputs(node).contains(&needle) {
            found.add(format!("graph node {owner}"), false);
        }
        if owner != needle
            && matches!(
                node,
                Node::SequenceExists { sequence, .. }
                    | Node::SequenceItemAt { sequence, .. }
                    | Node::SequenceAggregate { sequence, .. }
                    if sequence.item() == needle
            )
        {
            found.add(format!("graph node {owner} sequence item"), true);
        }
    }
    scope_references(root_scope, &mut Vec::new(), needle, false, &mut found);
    for target in extra_targets {
        let mut path = vec![format!("<target {}>", target.name)];
        scope_references(&target.root, &mut path, needle, true, &mut found);
    }
    for target in inactive_targets {
        let label = match target.name {
            Some(name) => format!("<target {name}>"),
            None => "<primary target>".to_string(),
        };
        scope_references(target.root, &mut vec![label], needle, true, &mut found);
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let label = format!("failure rule {}", index + 1);
        if rule.selection.predicate() == Some(needle) {
            found.add(format!("{label} predicate"), true);
        }
        if rule.message == Some(needle) {
            found.add(format!("{label} message"), true);
        }
        if let FailureIteration::Sequence { sequence } = &rule.iteration {
            if sequence.inputs().contains(&needle) {
                found.add(format!("{label} sequence input"), true);
            }
            if sequence.item() == needle {
                found.add(format!("{label} sequence item"), true);
            }
        }
    }
    for source in project.extra_sources {
        if source
            .dynamic_path
            .as_ref()
            .is_some_and(|path| path.node == needle)
        {
            found.add(format!("source {} dynamic path", source.name), true);
        }
    }
    found.finish()
}
