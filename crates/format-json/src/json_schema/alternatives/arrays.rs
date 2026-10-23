//! Array `anyOf` alternatives reduced to one scalar-item domain.

use super::*;

pub(in crate::json_schema) fn parse_scalar_domain_array_any_of(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let Some(alternatives) = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
    else {
        return Ok(None);
    };
    let mut arrays = Vec::new();
    let mut nullable = false;
    for alternative in alternatives {
        match classify_exact_array_alternative(name, alternative, doc, active_refs)? {
            ArrayAlternative::Null => nullable = true,
            ArrayAlternative::Array(candidate) => arrays.push(*candidate),
            ArrayAlternative::Other => return Ok(None),
        }
    }
    if arrays.is_empty() {
        return Ok(None);
    }
    let mut node = reduce_array_any_of(name, arrays)?;
    ensure_annotation_or_range_only(name, schema, "anyOf")?;
    set_container_nullable(&mut node, nullable)?;
    ranges::validate_ignored(name, schema)?;
    item_counts::apply(name, schema, &mut node, false)?;
    contains::apply(name, schema, &mut node, doc, active_refs, false)?;
    patterns::validate_ignored(name, schema)?;
    Ok(Some(node))
}

pub(super) fn set_container_nullable(
    node: &mut SchemaNode,
    nullable: bool,
) -> Result<(), JsonFormatError> {
    // Each object item keeps its own null admission when the container's
    // changes.
    let object_item_nullable = if node.repeating
        && matches!(node.kind, ir::SchemaKind::Group { .. })
        && !node.json_array_wrapper
    {
        Some(crate::null_layers::single_null(node)?)
    } else {
        None
    };
    node.container_nullable = nullable;
    if let Some(item_nullable) = object_item_nullable {
        node.json_object_item_nullable = (nullable || item_nullable).then_some(item_nullable);
    }
    Ok(())
}

fn scalar_array_domain_contains(superset: &SchemaNode, subset: &SchemaNode) -> bool {
    // A shape without an effective JSON view contains nothing.
    let (Ok(superset_null), Ok(subset_null)) = (
        crate::null_layers::layers(superset),
        crate::null_layers::layers(subset),
    ) else {
        return false;
    };
    if !superset.repeating
        || !subset.repeating
        || (subset_null.value_null && !superset_null.value_null)
        || superset_null.container_null != subset_null.container_null
        || (superset.json_unique_items && !subset.json_unique_items)
        || !item_count_domain_contains(superset.item_count_range, subset.item_count_range)
        || !contains_domain_contains(
            superset.json_contains.as_ref(),
            subset.json_contains.as_ref(),
        )
    {
        return false;
    }
    if subset.json_any {
        return superset.json_any;
    }
    if superset.json_any {
        return true;
    }
    if array_item_shapes_equal(superset, subset) {
        return true;
    }
    if constrained_scalar_item_domain_contains(superset, subset) {
        return true;
    }
    if !has_unconstrained_scalar_item_domain(superset) {
        return false;
    }
    [
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ]
    .into_iter()
    .all(|ty| !scalar_domain_contains(subset, ty) || scalar_domain_contains(superset, ty))
}

fn contains_domain_contains(
    superset: Option<&ir::JsonContainsConstraints>,
    subset: Option<&ir::JsonContainsConstraints>,
) -> bool {
    match (superset, subset) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(superset), Some(subset)) => superset == subset,
    }
}

fn reduce_array_any_of(
    name: &str,
    mut arrays: Vec<SchemaNode>,
) -> Result<SchemaNode, JsonFormatError> {
    let mut annotations = ir::JsonFormatAnnotations::default();
    for array in &mut arrays {
        annotations
            .extend(core::mem::take(&mut array.json_formats).into_vec())
            .map_err(|error| unsupported_union(name, &error.to_string()))?;
    }
    if let Some(superset) = arrays.iter().find(|candidate| {
        arrays
            .iter()
            .all(|other| scalar_array_domain_contains(candidate, other))
    }) {
        return Ok(with_array_formats(superset.clone(), annotations));
    }

    let mut groups: Vec<Vec<SchemaNode>> = Vec::new();
    for candidate in arrays {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| array_item_shapes_equal(&group[0], &candidate))
        {
            group.push(candidate);
        } else {
            groups.push(vec![candidate]);
        }
    }
    let mut merged = Vec::with_capacity(groups.len());
    for mut group in groups {
        let mut node = group.remove(0);
        node.item_count_range = union_item_count_range_set(
            name,
            core::iter::once(node.item_count_range)
                .chain(group.iter().map(|candidate| candidate.item_count_range)),
        )?;
        merged.push(node);
    }
    if let Some(superset) = merged.iter().find(|candidate| {
        merged
            .iter()
            .all(|other| scalar_array_domain_contains(candidate, other))
    }) {
        return Ok(with_array_formats(superset.clone(), annotations));
    }
    Err(unsupported_union(
        name,
        "anyOf array alternatives must have identical item schemas with contiguous count ranges, or one exact item/count domain that contains every other branch",
    ))
}

fn with_array_formats(mut node: SchemaNode, annotations: ir::JsonFormatAnnotations) -> SchemaNode {
    if !node.json_any && node.accepts_scalar_type(ScalarType::String) {
        node.json_formats = annotations;
    }
    node
}

fn array_item_shapes_equal(left: &SchemaNode, right: &SchemaNode) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.item_count_range = None;
    right.item_count_range = None;
    left.json_formats = Default::default();
    right.json_formats = Default::default();
    left == right
}

fn constrained_scalar_item_domain_contains(superset: &SchemaNode, subset: &SchemaNode) -> bool {
    if !superset.is_scalar() || !subset.is_scalar() {
        return false;
    }
    let mut normalized_superset = superset.clone();
    let mut normalized_subset = subset.clone();
    for node in [&mut normalized_superset, &mut normalized_subset] {
        node.repeating = false;
        node.container_nullable = false;
        node.nullable = false;
        node.item_count_range = None;
        node.json_formats = Default::default();
        node.string_length_range = None;
        node.json_patterns = None;
    }
    if normalized_superset != normalized_subset {
        return false;
    }
    string_length_domain_contains(superset.string_length_range, subset.string_length_range)
        && patterns::domain_contains(
            superset.json_patterns.as_ref(),
            subset.json_patterns.as_ref(),
        )
}

fn has_unconstrained_scalar_item_domain(node: &SchemaNode) -> bool {
    let mut actual = node.clone();
    actual.repeating = false;
    actual.container_nullable = false;
    actual.item_count_range = None;
    actual.json_formats = Default::default();
    let mut expected = match &actual.kind {
        SchemaKind::Scalar { ty } => SchemaNode::scalar(&actual.name, *ty),
        SchemaKind::ScalarUnion { types } => SchemaNode::scalar_union(&actual.name, *types),
        SchemaKind::Group { .. } => return false,
    };
    expected.nullable = actual.nullable;
    actual == expected
}

fn item_count_domain_contains(
    superset: Option<ir::ItemCountRange>,
    subset: Option<ir::ItemCountRange>,
) -> bool {
    match (superset, subset) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(superset), Some(subset)) => superset.contains_range(subset),
    }
}

fn string_length_domain_contains(
    superset: Option<ir::StringLengthRange>,
    subset: Option<ir::StringLengthRange>,
) -> bool {
    match (superset, subset) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(superset), Some(subset)) => superset.contains_range(subset),
    }
}

pub(super) fn union_string_length_range_set(
    name: &str,
    ranges: impl IntoIterator<Item = Option<ir::StringLengthRange>>,
) -> Result<Option<ir::StringLengthRange>, JsonFormatError> {
    let mut ranges = ranges.into_iter().collect::<Vec<_>>();
    if ranges.iter().any(Option::is_none) {
        return Ok(None);
    }
    let mut ranges = ranges
        .drain(..)
        .flatten()
        .collect::<Vec<ir::StringLengthRange>>();
    ranges.sort_by_key(|range| (range.minimum(), range.maximum()));
    let Some(mut merged) = ranges.first().copied() else {
        return Ok(None);
    };
    for range in ranges.into_iter().skip(1) {
        let Some(union) = merged.contiguous_union(range) else {
            return Err(unsupported_union(
                name,
                "anyOf string-length ranges are disjoint and cannot be represented as one interval",
            ));
        };
        let Some(union) = union else {
            return Ok(None);
        };
        merged = union;
    }
    Ok(Some(merged))
}

fn union_item_count_range_set(
    name: &str,
    ranges: impl IntoIterator<Item = Option<ir::ItemCountRange>>,
) -> Result<Option<ir::ItemCountRange>, JsonFormatError> {
    let mut ranges = ranges.into_iter().collect::<Vec<_>>();
    if ranges.iter().any(Option::is_none) {
        return Ok(None);
    }
    let mut ranges = ranges
        .drain(..)
        .flatten()
        .collect::<Vec<ir::ItemCountRange>>();
    ranges.sort_by_key(|range| (range.minimum(), range.maximum()));
    let Some(mut merged) = ranges.first().copied() else {
        return Ok(None);
    };
    for range in ranges.into_iter().skip(1) {
        let Some(union) = merged.contiguous_union(range) else {
            return Err(unsupported_union(
                name,
                "anyOf array count ranges are disjoint and cannot be represented as one interval",
            ));
        };
        let Some(union) = union else {
            return Ok(None);
        };
        merged = union;
    }
    Ok(Some(merged))
}

fn scalar_domain_contains(node: &SchemaNode, ty: ScalarType) -> bool {
    match node.kind {
        SchemaKind::Scalar { ty: declared } => declared == ty,
        SchemaKind::ScalarUnion { types } => types.contains(ty),
        SchemaKind::Group { .. } => false,
    }
}
