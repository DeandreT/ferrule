use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD, XML_TYPE_FIELD};
use mapping::{
    FailureIteration, FailureRule, Graph, Node, NodeId, Project, Scope, ScopeConstruction,
};

use super::ValidationIssue;
use super::schema::{
    display_path, follow_schema, source_path_matches, source_path_matches_resolved,
};

pub(super) fn validate_graph(project: &Project, issues: &mut Vec<ValidationIssue>) {
    let mut sequence_item_scopes = BTreeMap::new();
    collect_sequence_items(
        &project.root,
        &mut Vec::new(),
        &mut sequence_item_scopes,
        issues,
    );
    for target in &project.extra_targets {
        collect_sequence_items(
            &target.root,
            &mut Vec::new(),
            &mut sequence_item_scopes,
            issues,
        );
    }
    for (&id, node) in &project.graph.nodes {
        if let Node::SequenceExists { sequence, .. } | Node::SequenceItemAt { sequence, .. } = node
        {
            claim_sequence_item(
                sequence.item(),
                format!("graph node {id}"),
                &mut sequence_item_scopes,
                issues,
            );
        }
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        if let FailureIteration::Sequence { sequence } = &rule.iteration {
            claim_sequence_item(
                sequence.item(),
                format!("failure rule {}", index + 1),
                &mut sequence_item_scopes,
                issues,
            );
        }
    }
    let sequence_items: BTreeSet<_> = sequence_item_scopes.keys().copied().collect();
    validate_sequence_exists_contexts(project, &sequence_items, issues);
    validate_sequence_item_at_contexts(project, &sequence_items, issues);
    validate_failure_rules(project, &sequence_items, issues);
    for (&id, node) in &project.graph.nodes {
        let location = format!("graph node {id}");
        for (input, referenced) in node_inputs(node) {
            if !project.graph.nodes.contains_key(&referenced) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!("{input} references missing node {referenced}"),
                ));
            }
        }

        match node {
            Node::SourceField { .. } if sequence_items.contains(&id) => {}
            Node::SourceField { path, frame } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                let xml_type_marker = absolute
                    .strip_suffix(&[XML_TYPE_FIELD.to_string()])
                    .is_some_and(|owner| {
                        source_path_matches(project, owner, |node| !node.alternatives().is_empty())
                    });
                if !xml_type_marker
                    && !source_path_matches(project, &absolute, |node| {
                        matches!(node.kind, SchemaKind::Scalar { .. })
                    })
                {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "source field `{}` matches no scalar",
                            display_path(&absolute)
                        ),
                    ));
                }
            }
            Node::SourceDocumentPath => {
                if !project.source_options.local_xml_file_set
                    && !project
                        .extra_sources
                        .iter()
                        .any(|source| source.options.local_xml_file_set)
                {
                    issues.push(ValidationIssue::new(
                        &location,
                        "source document path requires a local XML file-set boundary",
                    ));
                }
            }
            Node::Position { collection } if !collection.is_empty() => {
                validate_collection_path(project, &location, collection, "position", issues);
            }
            Node::Call { function, .. } if !functions::is_known(function) => {
                issues.push(ValidationIssue::new(
                    &location,
                    format!("unknown function `{function}`"),
                ));
            }
            Node::Lookup {
                collection,
                key,
                value,
                ..
            } => {
                validate_collection_path(project, &location, collection, "lookup", issues);
                validate_collection_value(
                    project,
                    &location,
                    collection,
                    key,
                    "lookup key",
                    issues,
                );
                validate_collection_value(
                    project,
                    &location,
                    collection,
                    value,
                    "lookup value",
                    issues,
                );
            }
            Node::DynamicSourceField { object, frame, .. } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(object.iter().cloned());
                if !source_path_matches(project, &absolute, |node| {
                    node.dynamic_fields().is_some_and(|dynamic| {
                        !dynamic.repeating && matches!(dynamic.kind, SchemaKind::Scalar { .. })
                    })
                }) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "dynamic source object `{}` matches no open scalar object",
                            display_path(&absolute)
                        ),
                    ));
                }
            }
            Node::XmlMixedContent {
                path,
                frame,
                replacements,
            } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                if !source_path_matches_resolved(project, &absolute, |node| {
                    matches!(node.kind, SchemaKind::Group { .. })
                        && node.child(XML_TEXT_FIELD).is_some_and(|text| text.text)
                }) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "XML mixed-content source `{}` matches no mixed group",
                            display_path(&absolute)
                        ),
                    ));
                }
                let mut replacement_elements = BTreeSet::new();
                for replacement in replacements {
                    if replacement.element.is_empty() {
                        issues.push(ValidationIssue::new(
                            &location,
                            "XML mixed-content replacement element cannot be empty",
                        ));
                    } else if !replacement_elements.insert(replacement.element.as_str()) {
                        issues.push(ValidationIssue::new(
                            &location,
                            format!(
                                "XML mixed-content element `{}` has more than one replacement",
                                replacement.element
                            ),
                        ));
                    }
                    if !replacement.collection.is_empty() {
                        validate_collection_path(
                            project,
                            &location,
                            &replacement.collection,
                            "XML mixed-content replacement",
                            issues,
                        );
                    }
                }
            }
            Node::XmlSerialize {
                path,
                frame,
                schema,
                namespace,
                ..
            } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                let expected_group = matches!(schema.kind, SchemaKind::Group { .. });
                if !source_path_matches_resolved(project, &absolute, |source| {
                    source.name == schema.name
                        && matches!(source.kind, SchemaKind::Group { .. }) == expected_group
                }) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "XML serializer source `{}` does not match its `{}` schema",
                            display_path(&absolute),
                            schema.name
                        ),
                    ));
                }
                if schema.repeating {
                    issues.push(ValidationIssue::new(
                        &location,
                        "XML serializer schema must describe one document element",
                    ));
                }
                if namespace.as_ref().is_some_and(String::is_empty) {
                    issues.push(ValidationIssue::new(
                        &location,
                        "XML serializer namespace cannot be empty",
                    ));
                }
            }
            Node::CollectionFind { collection, .. } => {
                validate_collection_path(project, &location, collection, "collection find", issues);
            }
            Node::SequenceExists { sequence, .. } | Node::SequenceItemAt { sequence, .. } => {
                match project.graph.nodes.get(&sequence.item()) {
                    Some(Node::SourceField { path, frame: None }) if path.is_empty() => {}
                    Some(_) => issues.push(ValidationIssue::new(
                        &location,
                        "sequence item must reference an unframed empty-path source field",
                    )),
                    None => issues.push(ValidationIssue::new(
                        &location,
                        format!("sequence item references missing node {}", sequence.item()),
                    )),
                }
            }
            Node::Aggregate {
                collection,
                value,
                expression,
                ..
            } => {
                validate_collection_path(project, &location, collection, "aggregate", issues);
                if expression.is_none() && !value.is_empty() {
                    validate_collection_value(
                        project,
                        &location,
                        collection,
                        value,
                        "aggregate value",
                        issues,
                    );
                }
            }
            Node::JoinAggregate { join, plan, .. } => {
                super::join::validate_plan(project, *join, plan, &location, issues)
            }
            _ => {}
        }
    }
}

fn validate_failure_rules(
    project: &Project,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let location = format!("failure rule {}", index + 1);
        if let Some(message) = rule.message
            && !project.graph.nodes.contains_key(&message)
        {
            issues.push(ValidationIssue::new(
                &location,
                format!("message references missing node {message}"),
            ));
        }
        if let Some(predicate) = rule.selection.predicate()
            && !project.graph.nodes.contains_key(&predicate)
        {
            issues.push(ValidationIssue::new(
                &location,
                format!("selection predicate references missing node {predicate}"),
            ));
        }
        super::join::validate_roots(
            &project.graph,
            rule_roots(rule).into_iter().chain(match &rule.iteration {
                FailureIteration::Sequence { sequence } => sequence.inputs(),
                FailureIteration::Source { .. } => Vec::new(),
            }),
            &[],
            &location,
            project,
            issues,
        );
        validate_failure_dynamic_sources(project, &location, rule, issues);
        match &rule.iteration {
            FailureIteration::Source { collection } => {
                if !collection.is_empty()
                    && !source_path_matches(project, collection, |node| node.repeating)
                {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "source collection `{}` matches no repeating source path",
                            display_path(collection)
                        ),
                    ));
                }
            }
            FailureIteration::Sequence { sequence } => {
                match project.graph.nodes.get(&sequence.item()) {
                    Some(Node::SourceField { path, frame: None }) if path.is_empty() => {}
                    Some(_) => issues.push(ValidationIssue::new(
                        &location,
                        "sequence item must reference an unframed empty-path source field",
                    )),
                    None => issues.push(ValidationIssue::new(
                        &location,
                        format!("sequence item references missing node {}", sequence.item()),
                    )),
                }
                for (argument, node) in sequence.inputs().into_iter().enumerate() {
                    if !project.graph.nodes.contains_key(&node) {
                        issues.push(ValidationIssue::new(
                            &location,
                            format!("sequence argument {argument} references missing node {node}"),
                        ));
                    }
                }
                validate_failure_sequence_context(project, index, rule, sequence_items, issues);
            }
        }
    }
}

fn validate_failure_sequence_context(
    project: &Project,
    index: usize,
    rule: &FailureRule,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    let FailureIteration::Sequence { sequence } = &rule.iteration else {
        return;
    };
    let item = sequence.item();
    let location = format!("failure rule {}", index + 1);
    let roots = rule_roots(rule);
    let allowed = context_dependencies(&project.graph, roots.iter().copied());
    for foreign in allowed.intersection(sequence_items) {
        if *foreign != item {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "selection or message references sequence item node {foreign} owned by another generated context"
                ),
            ));
        }
    }
    for argument in sequence.inputs() {
        let dependencies = context_dependencies(&project.graph, [argument]);
        if dependencies.contains(&item) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "sequence argument depends on its own item node {item} before that item exists"
                ),
            ));
        }
        for foreign in dependencies.intersection(sequence_items) {
            if *foreign != item {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "sequence argument references item node {foreign} owned by another generated context"
                    ),
                ));
            }
        }
    }

    let mut dependent: BTreeSet<_> = allowed
        .iter()
        .copied()
        .filter(|&node| context_dependencies(&project.graph, [node]).contains(&item))
        .collect();
    // Ownership applies even when the rule itself does not read its item.
    // Otherwise that private empty-path source node could leak into a target
    // or another host-owned graph context unnoticed.
    dependent.insert(item);
    for (&consumer, node) in &project.graph.nodes {
        for input in context_node_inputs(node) {
            if dependent.contains(&input) && !allowed.contains(&consumer) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "item-dependent node {input} is also consumed by graph node {consumer} outside this failure rule"
                    ),
                ));
            }
        }
        if let Node::SequenceExists { predicate, .. } = node
            && dependent.contains(predicate)
        {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "item-dependent node {predicate} is reused as graph node {consumer}'s sequence-exists predicate"
                ),
            ));
        }
    }
    let mut scope_roots = BTreeSet::new();
    collect_scope_graph_roots(&project.root, &mut scope_roots);
    for target in &project.extra_targets {
        collect_scope_graph_roots(&target.root, &mut scope_roots);
    }
    for root in scope_roots.intersection(&dependent) {
        issues.push(ValidationIssue::new(
            &location,
            format!(
                "item-dependent node {root} is also referenced by a scope outside this failure rule"
            ),
        ));
    }
    for source in &project.extra_sources {
        let Some(dynamic) = &source.dynamic_path else {
            continue;
        };
        if dependent.contains(&dynamic.node) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "item-dependent node {} is also referenced by dynamic extra source `{}`",
                    dynamic.node, source.name
                ),
            ));
        }
    }
    for (other_index, other) in project.failure_rules.iter().enumerate() {
        if other_index == index {
            continue;
        }
        for root in rule_roots(other) {
            if dependent.contains(&root) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "item-dependent node {root} is also referenced by failure rule {}",
                        other_index + 1
                    ),
                ));
            }
        }
    }
}

fn rule_roots(rule: &FailureRule) -> Vec<NodeId> {
    rule.message
        .into_iter()
        .chain(rule.selection.predicate())
        .collect()
}

fn validate_failure_dynamic_sources(
    project: &Project,
    location: &str,
    rule: &FailureRule,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut dynamic_sources = BTreeSet::new();
    match &rule.iteration {
        FailureIteration::Source { collection } => {
            if let Some(source) = dynamic_source_for_path(project, collection) {
                dynamic_sources.insert(source.to_string());
            }
        }
        FailureIteration::Sequence { sequence } => {
            if let Some(source) = sequence_dynamic_source(project, sequence) {
                dynamic_sources.insert(source.to_string());
            }
        }
    }

    let roots = rule_roots(rule).into_iter().chain(match &rule.iteration {
        FailureIteration::Sequence { sequence } => sequence.inputs(),
        FailureIteration::Source { .. } => Vec::new(),
    });
    for node in all_dependencies(&project.graph, roots) {
        let Some(expression) = project.graph.nodes.get(&node) else {
            continue;
        };
        dynamic_sources.extend(node_dynamic_sources(project, expression).map(str::to_string));
    }

    for source in dynamic_sources {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "depends on dynamic extra source `{source}`, which is unavailable before target evaluation"
            ),
        ));
    }
}

fn all_dependencies(graph: &Graph, roots: impl IntoIterator<Item = NodeId>) -> BTreeSet<NodeId> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some(expression) = graph.nodes.get(&node) {
            pending.extend(node_inputs(expression).into_iter().map(|(_, input)| input));
        }
    }
    visited
}

fn node_dynamic_sources<'a>(project: &'a Project, node: &'a Node) -> impl Iterator<Item = &'a str> {
    let mut sources = BTreeSet::new();
    let mut inspect = |path: &[String]| {
        if let Some(source) = dynamic_source_for_path(project, path) {
            sources.insert(source);
        }
    };
    match node {
        Node::SourceField { path, frame }
        | Node::DynamicSourceField {
            object: path,
            frame,
            ..
        } => {
            inspect(frame.as_deref().unwrap_or(path));
        }
        Node::Position { collection }
        | Node::JoinField { collection, .. }
        | Node::Lookup { collection, .. }
        | Node::CollectionFind { collection, .. }
        | Node::Aggregate { collection, .. } => inspect(collection),
        Node::XmlMixedContent {
            path,
            frame,
            replacements,
        } => {
            inspect(frame.as_deref().unwrap_or(path));
            for replacement in replacements {
                inspect(&replacement.collection);
            }
        }
        Node::XmlSerialize { path, frame, .. } => {
            inspect(frame.as_deref().unwrap_or(path));
        }
        Node::SequenceExists { sequence, .. } | Node::SequenceItemAt { sequence, .. } => {
            if let Some(source) = sequence_dynamic_source(project, sequence) {
                sources.insert(source);
            }
        }
        Node::JoinAggregate { plan, .. } => {
            for source in plan.sources() {
                inspect(source.collection());
            }
        }
        Node::SourceDocumentPath
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. }
        | Node::Call { .. }
        | Node::If { .. }
        | Node::ValueMap { .. } => {}
    }
    sources.into_iter()
}

fn sequence_dynamic_source<'a>(
    project: &'a Project,
    sequence: &mapping::SequenceExpr,
) -> Option<&'a str> {
    match sequence {
        mapping::SequenceExpr::RecursiveCollect { collection, .. } => {
            dynamic_source_for_path(project, collection)
        }
        mapping::SequenceExpr::Tokenize { .. }
        | mapping::SequenceExpr::TokenizeByLength { .. }
        | mapping::SequenceExpr::TokenizeRegex { .. }
        | mapping::SequenceExpr::Generate { .. } => None,
    }
}

fn dynamic_source_for_path<'a>(project: &'a Project, path: &[String]) -> Option<&'a str> {
    let first = path.first()?;
    project
        .extra_sources
        .iter()
        .find(|source| source.name == *first && source.dynamic_path.is_some())
        .map(|source| source.name.as_str())
}

fn collect_sequence_items(
    scope: &Scope,
    path: &mut Vec<String>,
    items: &mut BTreeMap<NodeId, String>,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(sequence) = scope.sequence() {
        let location = if path.is_empty() {
            "root scope".to_string()
        } else {
            format!("scope `{}`", path.join("/"))
        };
        claim_sequence_item(sequence.item(), location, items, issues);
    }
    if let Some(segments) = scope.concatenated() {
        for (index, segment) in segments.iter().enumerate() {
            path.push(format!("<segment {}>", index + 1));
            collect_sequence_items(segment, path, items, issues);
            path.pop();
        }
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        collect_sequence_items(child, path, items, issues);
        path.pop();
    }
    for child in &scope.dynamic_children {
        path.push("*".to_string());
        collect_sequence_items(&child.scope, path, items, issues);
        path.pop();
    }
}

fn claim_sequence_item(
    item: NodeId,
    location: String,
    items: &mut BTreeMap<NodeId, String>,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(first) = items.get(&item) {
        issues.push(ValidationIssue::new(
            &location,
            format!(
                "sequence item node {item} is already owned by {first}; each generated sequence requires a unique item node"
            ),
        ));
    } else {
        items.insert(item, location);
    }
}

fn validate_sequence_exists_contexts(
    project: &Project,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut scope_roots = BTreeSet::new();
    collect_scope_graph_roots(&project.root, &mut scope_roots);
    for target in &project.extra_targets {
        collect_scope_graph_roots(&target.root, &mut scope_roots);
    }
    for (&owner, node) in &project.graph.nodes {
        let Node::SequenceExists {
            sequence,
            predicate,
        } = node
        else {
            continue;
        };
        let item = sequence.item();
        let location = format!("graph node {owner}");
        let allowed = context_dependencies(&project.graph, [*predicate]);

        for foreign in allowed.intersection(sequence_items) {
            if *foreign != item {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "predicate references sequence item node {foreign} owned by another generated context"
                    ),
                ));
            }
        }
        for argument in sequence.inputs() {
            if context_dependencies(&project.graph, [argument]).contains(&item) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "sequence argument depends on its own item node {item} before that item exists"
                    ),
                ));
            }
        }

        let dependent: BTreeSet<_> = allowed
            .iter()
            .copied()
            .filter(|&id| context_dependencies(&project.graph, [id]).contains(&item))
            .collect();
        if dependent.is_empty() {
            continue;
        }
        for (&consumer, consumer_node) in &project.graph.nodes {
            for input in context_node_inputs(consumer_node) {
                if dependent.contains(&input) && !allowed.contains(&consumer) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "item-dependent node {input} is also consumed by graph node {consumer} outside this predicate"
                        ),
                    ));
                }
            }
            if let Node::SequenceExists {
                predicate: nested_predicate,
                ..
            } = consumer_node
                && dependent.contains(nested_predicate)
                && consumer != owner
            {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "item-dependent node {nested_predicate} is reused as graph node {consumer}'s predicate"
                    ),
                ));
            }
        }
        for root in scope_roots.intersection(&dependent) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "item-dependent node {root} is also referenced by a scope outside this predicate"
                ),
            ));
        }
    }
}

fn validate_sequence_item_at_contexts(
    project: &Project,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (&owner, node) in &project.graph.nodes {
        let Node::SequenceItemAt { sequence, index } = node else {
            continue;
        };
        let item = sequence.item();
        let location = format!("graph node {owner}");
        for (label, input) in sequence
            .inputs()
            .into_iter()
            .enumerate()
            .map(|(index, input)| (format!("sequence argument {index}"), input))
            .chain([("index".to_string(), *index)])
        {
            let dependencies = context_dependencies(&project.graph, [input]);
            if dependencies.contains(&item) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "{label} depends on its own sequence item node {item} before that item exists"
                    ),
                ));
            }
            for foreign in dependencies.intersection(sequence_items) {
                if *foreign != item {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "{label} references sequence item node {foreign} owned by another generated context"
                        ),
                    ));
                }
            }
        }
    }
}

fn context_dependencies(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
) -> BTreeSet<NodeId> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(node) = graph.nodes.get(&id) {
            pending.extend(context_node_inputs(node));
        }
    }
    visited
}

fn context_node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SequenceExists { sequence, .. } => sequence.inputs(),
        _ => node_inputs(node)
            .into_iter()
            .map(|(_, referenced)| referenced)
            .collect(),
    }
}

fn collect_scope_graph_roots(scope: &Scope, roots: &mut BTreeSet<NodeId>) {
    roots.extend(scope.filter);
    roots.extend(scope.grouping_nodes());
    roots.extend(scope.sort_by);
    roots.extend(scope.output_path());
    roots.extend(
        scope
            .windows
            .iter()
            .copied()
            .flat_map(|window| window.nodes()),
    );
    if let Some(sequence) = scope.sequence() {
        roots.extend(sequence.inputs());
    }
    if let ScopeConstruction::RecursiveFilter { plan } = &scope.construction {
        roots.insert(plan.predicate());
    }
    if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction
        && let Some(root) = plan.root()
    {
        roots.insert(root);
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            collect_scope_graph_roots(segment, roots);
        }
    }
    roots.extend(scope.bindings.iter().map(|binding| binding.node));
    for binding in &scope.dynamic_bindings {
        roots.extend([binding.key, binding.value]);
    }
    for child in &scope.children {
        collect_scope_graph_roots(child, roots);
    }
    for child in &scope.dynamic_children {
        roots.insert(child.key);
        collect_scope_graph_roots(&child.scope, roots);
    }
}

pub(super) fn validate_collection_path(
    project: &Project,
    location: &str,
    collection: &[String],
    label: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if !source_path_matches(project, collection, |_| true) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "{label} collection `{}` matches no source path",
                display_path(collection)
            ),
        ));
    }
}

pub(super) fn validate_adjacency_string_field(
    location: &str,
    collection: &SchemaNode,
    path: &[String],
    role: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if follow_schema(collection, path).is_none_or(|field| {
        field.repeating
            || !matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ir::ScalarType::String
                }
            )
    }) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "adjacency-tree {role} field `{}` must be a non-repeating string",
                display_path(path)
            ),
        ));
    }
}

pub(super) fn validate_collection_value(
    project: &Project,
    location: &str,
    collection: &[String],
    value: &[String],
    label: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if !source_path_matches(project, collection, |node| {
        follow_schema(node, value)
            .is_some_and(|leaf| matches!(leaf.kind, SchemaKind::Scalar { .. }))
    }) {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "{label} `{}` is not a scalar under collection `{}`",
                display_path(value),
                display_path(collection)
            ),
        ));
    }
}

pub(super) fn node_inputs(node: &Node) -> Vec<(String, NodeId)> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::Call { args, .. } => args
            .iter()
            .enumerate()
            .map(|(index, &id)| (format!("argument {index}"), id))
            .collect(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![
            ("condition".into(), *condition),
            ("then branch".into(), *then),
            ("else branch".into(), *else_),
        ],
        Node::ValueMap { input, .. } => vec![("input".into(), *input)],
        Node::Lookup { matches, .. } => vec![("matches".into(), *matches)],
        Node::DynamicSourceField { key, .. } => vec![("property name".into(), *key)],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| {
                (
                    format!("replacement for `{}`", replacement.element),
                    replacement.expression,
                )
            })
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![
            ("predicate".into(), *predicate),
            ("value expression".into(), *value),
        ],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .enumerate()
            .map(|(index, id)| (format!("sequence argument {index}"), id))
            .chain([("predicate".to_string(), *predicate)])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .enumerate()
            .map(|(argument, id)| (format!("sequence argument {argument}"), id))
            .chain([("index".to_string(), *index)])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        } => expression
            .iter()
            .map(|&id| ("value expression".to_string(), id))
            .chain(arg.iter().map(|&id| ("argument".to_string(), id)))
            .collect(),
        Node::JoinAggregate {
            expression, arg, ..
        } => expression
            .iter()
            .map(|&id| ("value expression".to_string(), id))
            .chain(arg.iter().map(|&id| ("argument".to_string(), id)))
            .collect(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Active,
    Done,
}

pub(super) fn validate_cycles(graph: &Graph, issues: &mut Vec<ValidationIssue>) {
    fn visit(
        id: NodeId,
        graph: &Graph,
        visits: &mut BTreeMap<NodeId, Visit>,
        reported: &mut BTreeSet<NodeId>,
        issues: &mut Vec<ValidationIssue>,
    ) {
        visits.insert(id, Visit::Active);
        if let Some(node) = graph.nodes.get(&id) {
            for (_, referenced) in node_inputs(node) {
                match visits.get(&referenced) {
                    Some(Visit::Active) if reported.insert(referenced) => {
                        issues.push(ValidationIssue::new(
                            format!("graph node {id}"),
                            format!("cycle reaches node {referenced}"),
                        ));
                    }
                    Some(_) => {}
                    None if graph.nodes.contains_key(&referenced) => {
                        visit(referenced, graph, visits, reported, issues);
                    }
                    None => {}
                }
            }
        }
        visits.insert(id, Visit::Done);
    }

    let mut visits = BTreeMap::new();
    let mut reported = BTreeSet::new();
    for &id in graph.nodes.keys() {
        if !visits.contains_key(&id) {
            visit(id, graph, &mut visits, &mut reported, issues);
        }
    }
}
