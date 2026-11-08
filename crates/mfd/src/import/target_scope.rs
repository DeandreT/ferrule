use std::collections::{BTreeMap, BTreeSet};

use super::{
    BindingOccurrence, ComponentFormat, GraphBuilder, ResourceResolver, SchemaComponent,
    SchemaKind, Scope, ScopeBuilder, ScopeIteration, ScopeSequence, TargetLeaf, aggregate,
    document_collections, dynamic_json, filtered_document_filename, generated_occurrence,
    group_projection, join, json_parser, json_parser_object, json_variable, materialize,
    protobuf_target, recursive, scalar_root, schema, schema_node_at, scope, target_float_cast,
    target_iteration, target_mixed_content, target_node_default, target_node_function,
    target_type_cast, transposed_target, udf,
};

pub(super) fn scope_source_hints(
    structure: &roxmltree::Node<'_, '_>,
) -> BTreeMap<u32, Vec<String>> {
    structure
        .children()
        .find(|node| {
            node.has_tag_name("ferrule-scope-sources") && node.attribute("version") == Some("1")
        })
        .into_iter()
        .flat_map(|metadata| {
            metadata
                .children()
                .filter(|node| node.has_tag_name("scope"))
        })
        .filter_map(|scope| {
            let target = scope.attribute("targetkey")?.parse().ok()?;
            let source = serde_json::from_str(scope.attribute("source")?).ok()?;
            Some((target, source))
        })
        .collect()
}

pub(super) fn restore_scope_source_hints(
    root: &mut Scope,
    target: &SchemaComponent,
    hints: &BTreeMap<u32, Vec<String>>,
) {
    fn scope_at_mut<'a>(mut scope: &'a mut Scope, path: &[String]) -> Option<&'a mut Scope> {
        for segment in path {
            scope = scope
                .children
                .iter_mut()
                .find(|child| child.target_field == *segment)?;
        }
        Some(scope)
    }

    for (target_key, source) in hints {
        let Some(path) = target.ports.get(target_key) else {
            continue;
        };
        let Some(scope) = scope_at_mut(root, path) else {
            continue;
        };
        if matches!(scope.iteration, ScopeIteration::Source(_)) {
            scope.iteration = ScopeIteration::Source(source.clone());
        }
    }
}

pub(super) fn build_target_scope(
    mapping: &roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    copy_all_targets: &BTreeSet<u32>,
    eager_functions: Option<&BTreeSet<usize>>,
    builder: &mut GraphBuilder<'_>,
) -> Scope {
    if let Some(recipe) = document_collections::recognize(target, structure, resources, builder) {
        let mut candidate = builder.clone();
        let warnings = candidate.warnings.len();
        if let Some(plan) = recipe.materialize(&mut candidate) {
            let root = build_target_scope_inner(
                mapping,
                target,
                structure,
                resources,
                copy_all_targets,
                eager_functions,
                Some(&plan),
                &mut candidate,
            );
            if candidate.warnings.len() == warnings
                && let Some(root) = plan.wrap(root)
            {
                *builder = candidate;
                return root;
            }
        }
    }
    build_target_scope_inner(
        mapping,
        target,
        structure,
        resources,
        copy_all_targets,
        eager_functions,
        None,
        builder,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_target_scope_inner(
    mapping: &roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    copy_all_targets: &BTreeSet<u32>,
    eager_functions: Option<&BTreeSet<usize>>,
    document_plan: Option<&document_collections::Plan>,
    builder: &mut GraphBuilder<'_>,
) -> Scope {
    let edge_from = builder.edge_from;
    builder.rejected_join_paths.clear();
    builder.scope_projection_feeds.clear();
    let mut scopes = ScopeBuilder::new(BTreeMap::new());
    if let Some(plan) = document_plan {
        plan.initialize(&mut scopes);
    }
    let filtered_filename = document_plan
        .is_none()
        .then(|| filtered_document_filename::prepare(target, structure, builder))
        .flatten();
    if let Some(plan) = &filtered_filename {
        plan.install(&mut scopes);
    }
    let dynamic_document = (document_plan.is_none() && filtered_filename.is_none()).then(|| target
        .ports
        .iter()
        .find(|(_, path)| path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT])
        .and_then(|(input, _)| edge_from.get(input).copied())
        .and_then(|feed| {
            if builder.static_string_feed(feed).is_some() {
                return None;
            }
            let driver = builder.computed_iteration_source(feed);
            match driver {
                Some(driver) => {
                    builder.note_framed_prefixes(&driver);
                    let context = builder.context_path(&driver);
                    scopes.add_iteration(
                        &[],
                        &context,
                        scope::IterationNodes::default(),
                        mapping::IterationOutput::Repeated,
                    );
                    Some(feed)
                }
                None => {
                    builder.warnings.push(format!(
                        "target `{}` has a connected FileInstance path without a supported document driver; dynamic document output was skipped",
                        target.name
                    ));
                    None
                }
            }
        })).flatten();
    let dynamic_target = dynamic_json::prepare_target(target, builder);
    transposed_target::install(target, builder, &mut scopes);
    let mut iterations = Vec::new();
    let mut bindings = Vec::new();
    let mut float_cast_witnesses = Vec::new();
    let mut parser_object_targets = Vec::new();
    let mut group_projections = Vec::new();
    let mut structured_udf_targets = Vec::new();
    let mut csv_singleton_bindings = BTreeMap::new();
    let array_parser_ports = json_parser::install_array_records(target, builder, &mut scopes);
    for (&inpkey, target_path) in &target.ports {
        if array_parser_ports.contains(&inpkey) {
            continue;
        }
        let Some(&from) = edge_from.get(&inpkey) else {
            continue;
        };
        if target_path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT]
            || transposed_target::is_row_index_port(target_path)
        {
            continue;
        }
        if let Some((position, field)) = schema::split_singleton_port(target_path) {
            csv_singleton_bindings
                .entry(position)
                .or_insert_with(Vec::new)
                .push((field.to_string(), from));
            continue;
        }
        let node_kind = schema_node_at(&target.schema, target_path);
        if builder.json_parser_declares_object(from) {
            if !matches!(
                node_kind.map(|node| &node.kind),
                Some(SchemaKind::Group { .. })
            ) {
                builder.warnings.push(format!(
                    "JSON string parser output port `{from}` requires an ordinary object target; skipped",
                ));
                continue;
            }
            parser_object_targets.push((
                target_path.clone(),
                inpkey,
                from,
                copy_all_targets.contains(&inpkey),
            ));
            continue;
        }
        if let Some(node) = node_kind
            && recursive::accept_target(target_path, node, from, builder, &mut scopes)
        {
            continue;
        }
        match node_kind {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {
                match json_variable::expand_record(
                    target,
                    target_path,
                    node,
                    inpkey,
                    from,
                    copy_all_targets.contains(&inpkey),
                    builder,
                ) {
                    Ok(Some(record)) => {
                        for (path, feed) in record {
                            if let Some(leaf) = TargetLeaf::from_path(&path) {
                                bindings.push((leaf, feed, inpkey));
                            }
                        }
                        continue;
                    }
                    Err(reason) => {
                        builder.warnings.push(format!(
                            "JSON variable record into `{}` is unsupported: {reason}; skipped",
                            target_path.join("/")
                        ));
                        continue;
                    }
                    Ok(None) => {}
                }
                if udf::structured::accept_target(target, target_path, node, inpkey, from, builder)
                {
                    structured_udf_targets.push((target_path.clone(), from));
                    continue;
                }
                group_projection::classify_target_connection(
                    target,
                    group_projection::TargetConnection {
                        target_path,
                        target_node: node,
                        input_key: inpkey,
                        feed: from,
                        copy_all_targets,
                    },
                    builder,
                    &mut iterations,
                    &mut group_projections,
                )
            }
            Some(_) => {
                if let Some(target) = TargetLeaf::from_path(target_path) {
                    bindings.push((target, from, inpkey));
                }
            }
            None => builder.warnings.push(format!(
                "target port path `{}` not found in schema",
                target_path.join("/")
            )),
        }
    }
    order_repeating_scalar_bindings(target, &mut bindings);
    udf::structured::prepare_target_frames(&structured_udf_targets, builder);
    generated_occurrence::infer(target, builder, &mut iterations);
    iterations.sort_by_key(|iteration| iteration.target_path.len());
    let explicit_iteration_paths = iterations
        .iter()
        .map(|iteration| iteration.target_path.clone())
        .collect::<BTreeSet<_>>();
    join::prepare_iterations(&iterations, builder, &mut scopes);
    for iteration in &iterations {
        for feed_key in std::iter::once(iteration.feed)
            .chain(iteration.additional_feeds.iter().map(|(feed, _, _)| *feed))
        {
            let feed = builder.resolve_iteration_feed(feed_key);
            if let Some(idx) = feed.sequence_component {
                builder.sequence_scope_components.insert(idx);
            }
            if let Some(source_path) = builder.iteration_source_path(&feed) {
                builder.note_framed_prefixes(&source_path);
            }
        }
    }
    materialize::eager_functions(builder, eager_functions);
    scalar_root::install(target, structure, resources, builder, &mut scopes);
    let document_path = document_plan
        .map(|plan| plan.filename)
        .or_else(|| filtered_filename.as_ref().map(|plan| plan.filename))
        .or_else(|| {
            dynamic_document.and_then(|feed| builder.binding_node_at_anchor(feed, &[], &[]))
        });
    if let Some(node) = document_path
        && !scopes.root.set_output_path(Some(node))
    {
        builder.warnings.push(
            "target FileInstance path conflicts with another root iteration; dynamic document output was skipped"
                .to_string(),
        );
    }
    let mut skipped_iteration_paths =
        target_iteration::build(iterations, target, &mut bindings, builder, &mut scopes);
    skipped_iteration_paths.extend(json_parser_object::install(
        &parser_object_targets,
        target,
        builder,
        &mut scopes,
    ));
    protobuf_target::infer_singleton_messages(
        target,
        &bindings,
        &explicit_iteration_paths,
        builder,
        &mut scopes,
        &mut skipped_iteration_paths,
    );
    let structured_udf_paths = structured_udf_targets
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    udf::structured::build_targets(
        structured_udf_targets,
        target,
        builder,
        &mut scopes,
        &mut skipped_iteration_paths,
    );
    group_projection::build(
        group_projections,
        target,
        &skipped_iteration_paths,
        builder,
        &mut scopes,
    );
    target_mixed_content::install(target, builder, &mut scopes);
    for (target_leaf, from, input) in bindings {
        let target_path = target_leaf.path();
        if builder.join_dependency_rejected(from) {
            continue;
        }
        if structured_udf_paths
            .iter()
            .any(|path| target_path.starts_with(path))
            && builder.is_structured_recipe(from)
        {
            continue;
        }
        if skipped_iteration_paths
            .iter()
            .any(|path| target_path.starts_with(path))
        {
            continue;
        }
        let node = match builder.filtered_count_binding(target, input, from, &target_path, &scopes)
        {
            aggregate::filtered_count::CountBinding::Handled(node) => Some(node),
            aggregate::filtered_count::CountBinding::Refused => continue,
            aggregate::filtered_count::CountBinding::Legacy => {
                if install_repeating_scalar_iteration(
                    target,
                    &target_leaf,
                    from,
                    builder,
                    &mut scopes,
                ) {
                    continue;
                }
                let active_anchor = scopes.enclosing_anchor(&target_path);
                match target.db_xml_columns.get(&input) {
                    Some(column) => builder.database_xml_column_node(from, column),
                    None => builder.binding_node_at_anchor(from, &target_path, &active_anchor),
                }
            }
        };
        let Some(node) = node else {
            continue;
        };
        let node = if scopes
            .scope(target_leaf.chain())
            .is_some_and(|scope| scope.post_group_filter.is_some())
        {
            builder.group_member_value(node)
        } else {
            node
        };
        let root_index = target_leaf
            .chain()
            .is_empty()
            .then_some(scopes.root.bindings.len());
        let cast_path = target_leaf.path();
        scopes.add_binding(
            target_leaf,
            node,
            BindingOccurrence::authored(target, input),
        );
        if let Some(index) = root_index
            && scopes.root.bindings.len() == index + 1
        {
            float_cast_witnesses.push(target_float_cast::Witness {
                index,
                input,
                feed: from,
                node,
                path: cast_path,
            });
        }
    }
    dynamic_json::build_target(dynamic_target, target, builder, &mut scopes);
    compose_csv_target_rows(csv_singleton_bindings, builder, &mut scopes);
    target_node_default::install(target, structure, builder, &mut scopes);
    target_node_function::install(mapping, target, structure, resources, builder, &mut scopes);
    target_type_cast::install(target, structure, resources, builder, &mut scopes);
    target_float_cast::install(
        target,
        structure,
        resources,
        builder,
        &mut scopes.root,
        &float_cast_witnesses,
    );
    group_projection::install_optional_text_occurrences(target, builder, &mut scopes);
    ensure_singleton_csv_row_set(target, &mut scopes);
    scopes.root
}

/// A record-per-row target without an iterated record writes one record.
fn ensure_singleton_csv_row_set(target: &SchemaComponent, scopes: &mut ScopeBuilder) {
    let has_row_port = target.ports.values().any(Vec::is_empty);
    let record_rows = target.format == ComponentFormat::Csv || transposed_target::is_target(target);
    if !record_rows || has_row_port || scopes.root.iterates() {
        return;
    }
    let row = std::mem::take(&mut scopes.root);
    scopes.root.iteration = ScopeIteration::Concatenate(ScopeSequence::new(row, Vec::new()));
}

fn install_repeating_scalar_iteration(
    target_component: &SchemaComponent,
    target: &TargetLeaf,
    feed: u32,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> bool {
    let target_path = target.path();
    if !schema_node_at(&target_component.schema, &target_path)
        .is_some_and(|node| node.repeating && node.is_scalar())
    {
        return false;
    }
    let Some(source_path) = builder
        .source_abs_path(feed)
        .map(|path| builder.source_value_path(path.source, path.path))
    else {
        return false;
    };
    if !builder
        .schema_node(&source_path)
        .is_some_and(|node| node.repeating && node.is_scalar())
    {
        return false;
    }
    let source_abs = builder.context_path(&source_path);
    builder.note_framed_prefixes(&source_path);
    let value = builder.source_field(Some(source_abs.clone()), Vec::new());
    scopes.add_iteration(
        &target_path,
        &source_abs,
        scope::IterationNodes::default(),
        mapping::IterationOutput::Repeated,
    );
    scopes.ensure_scope(&target_path).construction = mapping::ScopeConstruction::Scalar { value };
    true
}

/// Repeated scalar target entries can be cloned several times under the same
/// schema path. Their numeric pin keys are identifiers, not an occurrence
/// order, so preserve the entry-tree branch order recorded by the schema
/// reader before the bindings are distributed into concatenated scopes.
fn order_repeating_scalar_bindings(
    target: &SchemaComponent,
    bindings: &mut [(TargetLeaf, u32, u32)],
) {
    let mut positions = BTreeMap::<Vec<String>, Vec<usize>>::new();
    for (index, (binding, _, _)) in bindings.iter().enumerate() {
        let path = binding.path();
        if schema_node_at(&target.schema, &path)
            .is_some_and(|node| node.repeating && node.is_scalar())
        {
            positions.entry(path).or_default().push(index);
        }
    }
    for positions in positions.values().filter(|positions| positions.len() > 1) {
        let mut ordered = positions
            .iter()
            .map(|index| bindings[*index].clone())
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            target
                .input_ancestors
                .get(&left.2)
                .cmp(&target.input_ancestors.get(&right.2))
                .then_with(|| left.2.cmp(&right.2))
        });
        for (position, binding) in positions.iter().copied().zip(ordered) {
            bindings[position] = binding;
        }
    }
}

fn compose_csv_target_rows(
    singleton_bindings: BTreeMap<schema::CsvSingletonPosition, Vec<(String, u32)>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    if singleton_bindings.is_empty() {
        return;
    }
    let repeated = std::mem::take(&mut scopes.root);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (position, bindings) in singleton_bindings {
        let mut segment = Scope::default();
        for (field, feed) in bindings {
            let target_path = vec![field.clone()];
            let Some(node) = builder.binding_node(feed, &target_path) else {
                continue;
            };
            segment.bindings.push(mapping::Binding {
                target_field: field,
                node,
            });
        }
        match position {
            schema::CsvSingletonPosition::Before(index) => before.push((index, segment)),
            schema::CsvSingletonPosition::After(index) => after.push((index, segment)),
        }
    }
    before.sort_by_key(|(index, _)| *index);
    after.sort_by_key(|(index, _)| *index);
    let mut segments = before
        .into_iter()
        .map(|(_, scope)| scope)
        .chain(std::iter::once(repeated))
        .chain(after.into_iter().map(|(_, scope)| scope));
    let Some(first) = segments.next() else {
        return;
    };
    scopes.root.iteration =
        ScopeIteration::Concatenate(ScopeSequence::new(first, segments.collect()));
}
