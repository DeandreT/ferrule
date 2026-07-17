use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ir::{SchemaKind, SchemaNode, XML_TYPE_FIELD};
use mapping::{
    FormatOptions, Graph, IterationOutput, JoinId, Node, NodeId, Project, Scope, ScopeConstruction,
    ScopeIteration, XbrlBoundaryMode,
};

use super::validate_join::{
    validate_plan as validate_join_plan, validate_roots as validate_join_roots,
    validate_scope_nodes as validate_scope_join_nodes,
};

/// One actionable problem found before a mapping is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub location: String,
    pub message: String,
}

impl ValidationIssue {
    pub(super) fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Checks graph integrity, source/target paths, scope references, builtin
/// names, and cycles without reading input data or evaluating expressions.
pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    validate_xbrl_options(
        "source format options",
        &project.source_options,
        XbrlBoundaryMode::ExternalSource,
        &mut issues,
    );
    validate_external_source_options(
        "source format options",
        &project.source_options,
        true,
        &mut issues,
    );
    validate_structured_edi_source_options(
        "source format options",
        &project.source_options,
        &mut issues,
    );
    validate_target_options(
        "target format options",
        &project.target_options,
        &mut issues,
    );
    if let Some(layout) = &project.source_options.pdf
        && layout.schema() != project.source
    {
        issues.push(ValidationIssue::new(
            "source format options",
            "PDF extraction layout does not match the source schema",
        ));
    }
    validate_schema(
        "source schema",
        &project.source,
        &mut Vec::new(),
        &mut issues,
    );
    validate_schema(
        "target schema",
        &project.target,
        &mut Vec::new(),
        &mut issues,
    );
    let mut target_names = BTreeSet::new();
    for target in &project.extra_targets {
        let name = target.name.trim();
        if name.is_empty() {
            issues.push(ValidationIssue::new(
                "extra target",
                "extra target name cannot be empty",
            ));
        } else if !target_names.insert(name) {
            issues.push(ValidationIssue::new(
                format!("extra target `{name}`"),
                "extra target name is duplicated",
            ));
        }
        validate_target_options(
            &format!("extra target `{name}` format options"),
            &target.options,
            &mut issues,
        );
        validate_schema(
            &format!("extra target `{name}` schema"),
            &target.schema,
            &mut Vec::new(),
            &mut issues,
        );
    }
    let mut source_names = BTreeSet::new();
    for source in &project.extra_sources {
        let name = source.name.trim();
        let location = format!("extra source `{name}`");
        if name.is_empty() {
            issues.push(ValidationIssue::new(
                "extra source",
                "extra source name cannot be empty",
            ));
        } else if !source_names.insert(name) {
            issues.push(ValidationIssue::new(
                &location,
                "extra source name is duplicated",
            ));
        }
        validate_xbrl_options(
            &format!("{location} format options"),
            &source.options,
            XbrlBoundaryMode::ExternalSource,
            &mut issues,
        );
        validate_external_source_options(
            &format!("{location} format options"),
            &source.options,
            true,
            &mut issues,
        );
        validate_structured_edi_source_options(
            &format!("{location} format options"),
            &source.options,
            &mut issues,
        );
        if let Some(layout) = &source.options.pdf
            && layout.schema() != source.schema
        {
            issues.push(ValidationIssue::new(
                format!("{location} format options"),
                "PDF extraction layout does not match the extra-source schema",
            ));
        }
        validate_schema(
            &format!("{location} schema"),
            &source.schema,
            &mut Vec::new(),
            &mut issues,
        );
        if let Some(dynamic) = &source.dynamic_path {
            if !project.graph.nodes.contains_key(&dynamic.node) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "dynamic path expression references missing node {}",
                        dynamic.node
                    ),
                ));
            }
            if !source_path_matches(project, &dynamic.iteration, |_| true) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "dynamic path iteration `{}` matches no source path",
                        display_path(&dynamic.iteration)
                    ),
                ));
            }
        }
    }
    validate_graph(project, &mut issues);
    validate_cycles(&project.graph, &mut issues);
    validate_scope(
        project,
        &project.root,
        ScopeSchemas {
            target: Some(&project.target),
            parent_source: Some(&project.source),
        },
        &mut Vec::new(),
        &[],
        &mut BTreeMap::new(),
        &mut issues,
    );
    for target in &project.extra_targets {
        validate_scope(
            project,
            &target.root,
            ScopeSchemas {
                target: Some(&target.schema),
                parent_source: Some(&project.source),
            },
            &mut Vec::new(),
            &[],
            &mut BTreeMap::new(),
            &mut issues,
        );
    }
    issues
}

fn validate_target_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_xbrl_options(location, options, XbrlBoundaryMode::ExternalTarget, issues);
    validate_external_source_options(location, options, false, issues);
    if options.http_get.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "HTTP GET transport is valid only for mapping sources",
        ));
    }
    if options.pdf.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "PDF extraction is valid only for mapping sources",
        ));
    }
    if options.idoc.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "SAP IDoc layouts are valid only for mapping sources",
        ));
    }
    if options.swift_mt.is_some() {
        issues.push(ValidationIssue::new(
            location,
            "SWIFT MT layouts are valid only for mapping sources",
        ));
    }
}

fn validate_structured_edi_source_options(
    location: &str,
    options: &FormatOptions,
    issues: &mut Vec<ValidationIssue>,
) {
    if options.idoc.is_some() && has_non_idoc_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`idoc` cannot be combined with another format's options",
        ));
    }
    if options.swift_mt.is_some() && has_non_swift_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`swift_mt` cannot be combined with another format's options",
        ));
    }
}

fn has_non_idoc_format_options(options: &FormatOptions) -> bool {
    options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_xlsx_format_options(options)
}

fn has_non_swift_format_options(options: &FormatOptions) -> bool {
    options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_xlsx_format_options(options)
}

fn has_xlsx_format_options(options: &FormatOptions) -> bool {
    options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn validate_external_source_options(
    location: &str,
    options: &FormatOptions,
    source_side: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    if options.external_source.is_none() {
        return;
    }
    if !source_side {
        issues.push(ValidationIssue::new(
            location,
            "captured external responses are valid only for mapping sources",
        ));
    }
    if has_non_external_source_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`external_source` cannot be combined with another format's options",
        ));
    }
}

fn has_non_external_source_format_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn validate_xbrl_options(
    location: &str,
    options: &FormatOptions,
    expected_mode: XbrlBoundaryMode,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(xbrl) = &options.xbrl else {
        return;
    };
    if xbrl.mode() != expected_mode {
        let actual_mode = match xbrl.mode() {
            XbrlBoundaryMode::ExternalSource => "external source",
            XbrlBoundaryMode::ExternalTarget => "external target",
        };
        let expected_side = match expected_mode {
            XbrlBoundaryMode::ExternalSource => "source",
            XbrlBoundaryMode::ExternalTarget => "target",
        };
        issues.push(ValidationIssue::new(
            location,
            format!("XBRL boundary mode `{actual_mode}` is invalid on a mapping {expected_side}"),
        ));
    }
    if has_non_xbrl_format_options(options) {
        issues.push(ValidationIssue::new(
            location,
            "`xbrl` cannot be combined with another format's options",
        ));
    }
}

fn has_non_xbrl_format_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn validate_schema(
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

fn validate_graph(project: &Project, issues: &mut Vec<ValidationIssue>) {
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
        if let Node::SequenceExists { sequence, .. } = node {
            claim_sequence_item(
                sequence.item(),
                format!("graph node {id}"),
                &mut sequence_item_scopes,
                issues,
            );
        }
    }
    let sequence_items: BTreeSet<_> = sequence_item_scopes.keys().copied().collect();
    validate_sequence_exists_contexts(project, &sequence_items, issues);
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
            Node::CollectionFind { collection, .. } => {
                validate_collection_path(project, &location, collection, "collection find", issues);
            }
            Node::SequenceExists { sequence, .. } => {
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
                super::validate_join::validate_plan(project, *join, plan, &location, issues)
            }
            _ => {}
        }
    }
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
    roots.extend(
        [
            scope.filter,
            scope.group_by,
            scope.group_starting_with,
            scope.group_into_blocks,
            scope.sort_by,
            scope.take,
        ]
        .into_iter()
        .flatten(),
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

fn validate_collection_path(
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

fn validate_adjacency_string_field(
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
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
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

fn validate_cycles(graph: &Graph, issues: &mut Vec<ValidationIssue>) {
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

#[derive(Clone, Copy)]
struct ScopeSchemas<'a> {
    target: Option<&'a SchemaNode>,
    parent_source: Option<&'a SchemaNode>,
}

fn validate_scope(
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
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || scope.take.is_some()
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
        if scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
        {
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
            ScopeIteration::None | ScopeIteration::Source(_) => {}
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
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || scope.take.is_some()
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
            ScopeIteration::None | ScopeIteration::Source(_) => {}
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
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || scope.take.is_some()
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
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || scope.take.is_some()
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
    let mut parent_roots = scope.take.into_iter().collect::<Vec<_>>();
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
        if scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
        {
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
        ("group-by key", scope.group_by),
        ("group-starting-with predicate", scope.group_starting_with),
        ("group block size", scope.group_into_blocks),
        ("sort key", scope.sort_by),
        ("take count", scope.take),
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
    if !iterates && scope.group_by.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-by key has no iterated source",
        ));
    }
    if !iterates && scope.group_starting_with.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group-starting-with predicate has no iterated source",
        ));
    }
    if !iterates && scope.group_into_blocks.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "group block size has no iterated source",
        ));
    }
    if [
        scope.group_by,
        scope.group_starting_with,
        scope.group_into_blocks,
    ]
    .into_iter()
    .flatten()
    .count()
        > 1
    {
        issues.push(ValidationIssue::new(
            &location,
            "scope grouping modes are mutually exclusive",
        ));
    }
    if !iterates && scope.sort_by.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "sort key has no iterated source",
        ));
    }
    if !iterates && scope.take.is_some() {
        issues.push(ValidationIssue::new(
            &location,
            "take count has no iterated source",
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

fn current_source_schema<'a>(
    project: &'a Project,
    parent: Option<&'a SchemaNode>,
    iteration: &ScopeIteration,
) -> Option<&'a SchemaNode> {
    match iteration {
        ScopeIteration::None => parent,
        ScopeIteration::Source(path) => source_schema_at(project, parent, path),
        ScopeIteration::Sequence(_)
        | ScopeIteration::InnerJoin { .. }
        | ScopeIteration::Concatenate(_) => None,
    }
}

fn source_schema_at<'a>(
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

fn follow_schema<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
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

#[cfg(test)]
mod tests;
