use super::*;

/// Raw, typed operands in one ordinary physical JSON frame. This contract does
/// not reuse computed-producer presence proofs from the XML/EDI adapter.
pub(super) enum ScalarProof {
    Literal(ir::Value),
    Field {
        frame: Vec<String>,
        path: Vec<String>,
    },
}

impl ScalarProof {
    pub(super) fn matches_lowered(
        &self,
        node: mapping::NodeId,
        builder: &GraphBuilder<'_>,
    ) -> bool {
        match self {
            Self::Literal(value) => exact_constant_node(node, value, builder),
            Self::Field { frame, path } => matches!(
                builder.graph.nodes.get(&node),
                Some(mapping::Node::SourceField { path: actual, frame: Some(actual_frame) })
                    if actual == path && actual_frame == frame
            ),
        }
    }
}

pub(super) fn admit(target: &SchemaComponent, builder: &GraphBuilder<'_>) -> bool {
    let Some(primary) = builder.sources.first() else {
        return false;
    };
    let mut source_budget = MAX_BINDINGS;
    let mut target_budget = MAX_BINDINGS;
    ordinary(primary, true)
        && ordinary(target, false)
        && !primary.schema.repeating
        && !target.schema.repeating
        && plain_schema(&primary.schema, 0, &mut source_budget)
        && plain_schema(&target.schema, 0, &mut target_budget)
        && !primary
            .output_keys
            .iter()
            .any(|key| builder.has_source_node_functions(*key))
}

pub(super) fn ordinary(component: &SchemaComponent, source: bool) -> bool {
    component.format == ComponentFormat::Json
        && component.is_source == source
        && component.json_schema_authoritative
        && !component.is_variable
        && !component.is_pass_through
        && component.compute_when_key.is_none()
        && component.dynamic_json.is_none()
        && !component.has_target_node_controls
        && component.options
            == mapping::FormatOptions {
                json_document: true,
                ..Default::default()
            }
        && [&component.input_instance, &component.output_instance]
            .into_iter()
            .flatten()
            .all(|path| {
                !std::path::Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json5"))
            })
}

/// Comparing to a reconstructed plain node preserves required/repeating and
/// explicit nonnull item facts without discarding any extra schema metadata.
pub(in crate::import) fn plain_schema(
    node: &SchemaNode,
    depth: usize,
    remaining: &mut usize,
) -> bool {
    if depth > MAX_DEPTH || *remaining == 0 || !presence::json_non_null(node) {
        return false;
    }
    *remaining -= 1;
    let mut expected = match &node.kind {
        SchemaKind::Scalar { ty }
            if matches!(
                ty,
                ir::ScalarType::Bool | ir::ScalarType::Int | ir::ScalarType::String
            ) && !node.repeating =>
        {
            SchemaNode::scalar(node.name.clone(), *ty)
        }
        SchemaKind::Group {
            children, required, ..
        } if children
            .iter()
            .all(|child| plain_schema(child, depth + 1, remaining)) =>
        {
            let Some(group) = SchemaNode::group(node.name.clone(), children.clone())
                .with_required_fields(required.clone())
            else {
                return false;
            };
            group
        }
        _ => return false,
    };
    expected.repeating = node.repeating;
    if node.repeating && matches!(node.kind, SchemaKind::Group { .. }) {
        // Omitted legacy metadata and explicit false are separately retained.
        expected.json_object_item_nullable = node.json_object_item_nullable;
    }
    node == &expected
}

pub(super) fn plain_item(node: &SchemaNode) -> bool {
    fn descendants(node: &SchemaNode) -> bool {
        !node.repeating
            && match &node.kind {
                SchemaKind::Group { children, .. } => children.iter().all(descendants),
                _ => node.is_scalar(),
            }
    }
    node.repeating
        && matches!(&node.kind, SchemaKind::Group { children, .. }
            if children.iter().all(descendants))
}

pub(super) fn actual_array_frame(
    path: &[String],
    anchor: &[String],
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let Some(primary) = builder.sources.first() else {
        return false;
    };
    schema_node_at(&primary.schema, anchor)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        && (1..=anchor.len()).all(|source_length| {
            let source_prefix = &anchor[..source_length];
            schema_node_at(&primary.schema, source_prefix).is_some_and(|node| {
                !node.repeating
                    || (0..path.len()).any(|length| {
                        let prefix = &path[..length];
                        schema_node_at(&target.schema, prefix).is_some_and(|node| {
                            node.repeating && matches!(node.kind, SchemaKind::Group { .. })
                        }) && scopes.scope(prefix).is_some_and(|scope| {
                            matches!(scope.iteration, ScopeIteration::Source(_))
                        }) && scopes
                            .anchors
                            .get(prefix)
                            .is_some_and(|actual| actual == source_prefix)
                            && physical_frame_driver(prefix, source_prefix, target, builder)
                    })
            })
        })
}

/// The new adapter owns no previously lowered candidate subtree. Refuse a
/// binding, child, iteration or control that would be discarded by replacement.
pub(super) fn replaceable(scope: &mapping::Scope) -> bool {
    matches!(scope.iteration, ScopeIteration::None)
        && scope.construction == ScopeConstruction::Constructed
        && scope.iteration_output == IterationOutput::Repeated
        && scope.output_path().is_none()
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && !scope.sort_descending
        && scope.sort_filter_order == mapping::SortFilterOrder::SortThenFilter
        && scope.windows.is_empty()
        && scope.bindings.is_empty()
        && scope.children.is_empty()
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

pub(super) fn scalar_proof(
    feed: u32,
    anchor: &[String],
    target: &SchemaNode,
    builder: &GraphBuilder<'_>,
) -> Option<ScalarProof> {
    let SchemaKind::Scalar { ty } = target.kind else {
        return None;
    };
    if !matches!(
        ty,
        ir::ScalarType::Bool | ir::ScalarType::Int | ir::ScalarType::String
    ) || target.repeating
        || !unique_producer(feed, builder)
    {
        return None;
    }
    if let Some(value) = exact_literal(feed, builder) {
        let component = builder
            .fn_components
            .get(*builder.fn_by_output.get(&feed)?)?;
        return (component.raw_literal_valid && constant_has_type(&value, ty))
            .then_some(ScalarProof::Literal(value));
    }
    let primary = builder.sources.first()?;
    let absolute = physical_scalar(feed, anchor, builder)?;
    if !primary.json_scalar_port(feed, ty)
        || !guaranteed_scalar(absolute, anchor, &primary.schema)
        || !matches!(schema_node_at(&primary.schema, absolute)?.kind,
            SchemaKind::Scalar { ty: source_type } if source_type == ty)
    {
        return None;
    }
    Some(ScalarProof::Field {
        frame: anchor.to_vec(),
        path: absolute.strip_prefix(anchor)?.to_vec(),
    })
}

pub(super) fn required_assignments(
    node: &SchemaNode,
    path: &[String],
    assigned: &BTreeSet<Vec<String>>,
    present: &BTreeSet<Vec<String>>,
) -> bool {
    let SchemaKind::Group {
        children, required, ..
    } = &node.kind
    else {
        return false;
    };
    children.iter().all(|child| {
        let mut field = path.to_vec();
        field.push(child.name.clone());
        let required = required.contains(&child.name);
        if child.is_scalar() {
            !required || assigned.contains(&field)
        } else if present.contains(&field) {
            required_assignments(child, &field, assigned, present)
        } else {
            !required
        }
    })
}

pub(super) fn bindings_preserved(
    segment: &mapping::Scope,
    origins: &BindingOrigins,
    path: &[String],
    indices: &[usize],
) -> bool {
    fn collect(
        scope: &mapping::Scope,
        path: &mut Vec<String>,
        retained: &mut BTreeMap<(Vec<String>, mapping::NodeId), usize>,
    ) {
        for binding in &scope.bindings {
            let mut field = path.clone();
            field.push(binding.target_field.clone());
            *retained.entry((field, binding.node)).or_default() += 1;
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            collect(child, path, retained);
            path.pop();
        }
    }
    let mut retained = BTreeMap::new();
    collect(segment, &mut path.to_vec(), &mut retained);
    let expected = indices.iter().copied().collect::<BTreeSet<_>>();
    // Bindings are installed per authored input, so one interned expression
    // feeding several entries is retained once for each of them.
    origins.values().map(BTreeSet::len).sum::<usize>() == indices.len()
        && retained.len() == origins.len()
        && origins
            .iter()
            .all(|(binding, indices)| retained.get(binding) == Some(&indices.len()))
        && origins.values().flatten().copied().collect::<BTreeSet<_>>() == expected
}
