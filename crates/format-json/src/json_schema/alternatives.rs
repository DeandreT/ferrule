use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    GroupAlternativeMode, ScalarType, ScalarTypeSet, SchemaKind, SchemaNode,
};

use super::{
    files, formats, item_counts, parse, ranges, reject_unsupported_ref_siblings, resolve_ref,
    string_lengths, unsupported_union,
};
use crate::JsonFormatError;

enum WrapperAdditional {
    Open,
    Closed,
    Typed(Box<SchemaNode>),
}

enum ScalarAlternative {
    Null,
    Scalar(ScalarType),
    Other,
}

enum ExactScalarAlternative {
    Null,
    Scalar {
        ty: ScalarType,
        formats: ir::JsonFormatAnnotations,
        string_length: Option<ir::StringLengthRange>,
    },
    Other,
}

enum ArrayAlternative {
    Null,
    Array(Box<SchemaNode>),
    Other,
}

/// Imports an inclusive union made entirely from exact scalar type branches.
/// Duplicate branches collapse, while distinct types become a first-class
/// scalar union and retain their runtime value tags.
pub(super) fn parse_scalar_any_of(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    parse_scalar_composition(name, schema, alternatives, "anyOf", false, doc, active_refs)
}

/// Imports an exclusive union made entirely from exact, pairwise-disjoint
/// scalar type branches. JSON Schema `integer` overlaps `number`, and repeated
/// scalar or null branches overlap themselves, so those shapes cannot be
/// represented as an exact ferrule scalar union.
pub(super) fn parse_scalar_one_of(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    parse_scalar_composition(name, schema, alternatives, "oneOf", true, doc, active_refs)
}

fn parse_scalar_composition(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    keyword: &str,
    exclusive: bool,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let Some(alternatives) = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
    else {
        return Ok(None);
    };
    let mut scalar_types = Vec::new();
    let mut nullable = false;
    let mut format_annotations = ir::JsonFormatAnnotations::default();
    let mut string_lengths = Vec::new();
    if let Some(format) = formats::validate(name, schema)? {
        formats::accumulate(
            name,
            &mut format_annotations,
            core::iter::once(format.to_string()),
        )?;
    }
    for alternative in alternatives {
        match classify_exact_scalar_alternative(name, alternative, doc, active_refs)? {
            ExactScalarAlternative::Null => {
                if exclusive && nullable {
                    return Err(overlapping_scalar_one_of(name));
                }
                nullable = true;
            }
            ExactScalarAlternative::Scalar {
                ty,
                formats: branch_formats,
                string_length,
            } => {
                if exclusive
                    && scalar_types
                        .iter()
                        .copied()
                        .any(|existing| scalar_types_overlap(existing, ty))
                {
                    return Err(overlapping_scalar_one_of(name));
                }
                if !scalar_types.contains(&ty) {
                    scalar_types.push(ty);
                }
                formats::accumulate(name, &mut format_annotations, branch_formats.into_vec())?;
                if ty == ScalarType::String {
                    string_lengths.push(string_length);
                }
            }
            ExactScalarAlternative::Other => return Ok(None),
        }
    }
    let Some(first) = scalar_types.first().copied() else {
        return Ok(None);
    };
    ensure_annotation_or_format_only(name, schema, keyword)?;
    let mut node = if scalar_types.len() == 1 {
        SchemaNode::scalar(name, first)
    } else {
        let Some(types) = ScalarTypeSet::new(scalar_types) else {
            return Err(unsupported_union(
                name,
                &format!("scalar {keyword} contains an invalid type set"),
            ));
        };
        SchemaNode::scalar_union(name, types)
    };
    node.nullable = nullable;
    if node.accepts_scalar_type(ScalarType::String) {
        node.json_formats = format_annotations;
        node.string_length_range = union_string_length_range_set(name, string_lengths)?;
    }
    Ok(Some(node))
}

fn scalar_types_overlap(left: ScalarType, right: ScalarType) -> bool {
    left == right
        || matches!(
            (left, right),
            (ScalarType::Int, ScalarType::Float) | (ScalarType::Float, ScalarType::Int)
        )
}

fn overlapping_scalar_one_of(name: &str) -> JsonFormatError {
    unsupported_union(
        name,
        "scalar oneOf branches overlap and are not mutually exclusive",
    )
}

/// Collapses inclusive array branches when one imported scalar item domain
/// contains every other branch. This preserves the exact union: an array whose
/// item schema already accepts strings and integers subsumes a string-only
/// array, including mixed arrays admitted by the broader branch itself.
///
/// Incomparable item domains remain unsupported because merging them would
/// incorrectly admit mixed arrays that match none of the original branches.
pub(super) fn parse_scalar_domain_array_any_of(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    doc: &serde_json::Value,
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
    node.container_nullable = nullable;
    ranges::validate_ignored(name, schema)?;
    item_counts::apply(name, schema, &mut node, false)?;
    Ok(Some(node))
}

fn scalar_array_domain_contains(superset: &SchemaNode, subset: &SchemaNode) -> bool {
    if !superset.repeating
        || !subset.repeating
        || (subset.nullable && !superset.nullable)
        || superset.container_nullable != subset.container_nullable
        || !item_count_domain_contains(superset.item_count_range, subset.item_count_range)
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
    }
    if normalized_superset != normalized_subset {
        return false;
    }
    string_length_domain_contains(superset.string_length_range, subset.string_length_range)
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

fn union_string_length_range_set(
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

/// Canonicalizes the common nullable-scalar union spelling used by OpenAPI
/// and generated JSON Schemas. Structured nullability needs a distinct
/// instance variant, while scalar nullability maps exactly to
/// `SchemaNode::nullable`.
pub(super) fn parse_nullable_scalar_alternatives(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    keyword: &str,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let Some(alternatives) = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() == 2)
    else {
        return Ok(None);
    };
    let first = classify_scalar_alternative(name, &alternatives[0], doc, active_refs)?;
    let second = classify_scalar_alternative(name, &alternatives[1], doc, active_refs)?;
    let (ty, content) = match (first, second) {
        (ScalarAlternative::Null, ScalarAlternative::Scalar(ty)) => (ty, &alternatives[1]),
        (ScalarAlternative::Scalar(ty), ScalarAlternative::Null) => (ty, &alternatives[0]),
        _ => return Ok(None),
    };
    ensure_annotation_or_range_only(name, schema, keyword)?;
    let content = without_ignored_scalar_validation(content);
    let mut node = parse(name, &content, doc, active_refs)?;
    if node.repeating || !matches!(node.kind, SchemaKind::Scalar { ty: parsed } if parsed == ty) {
        return Err(unsupported_union(
            name,
            "nullable scalar alternative did not retain one concrete scalar type",
        ));
    }
    node.nullable = true;
    Ok(Some(node))
}

fn without_ignored_scalar_validation(schema: &serde_json::Value) -> serde_json::Value {
    let mut schema = schema.clone();
    if let Some(schema) = schema.as_object_mut() {
        for keyword in [
            "const",
            "enum",
            "pattern",
            "multipleOf",
            "contentEncoding",
            "contentMediaType",
            "contentSchema",
        ] {
            schema.remove(keyword);
        }
    }
    schema
}

/// Removes exact null branches, lowers the remaining composition through its
/// ordinary exact scalar/object/array path, then restores the composition's
/// null domain. This handles flat nullable unions with more than one content
/// branch as well as nested scalar-union wrappers.
pub(super) fn parse_nullable_composition(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    keyword: &str,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let Some(alternatives) = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
    else {
        return Ok(None);
    };
    let mut null_count = 0;
    let mut content = Vec::with_capacity(alternatives.len());
    for alternative in alternatives {
        if is_null_alternative(name, alternative, doc, active_refs)? {
            null_count += 1;
        } else {
            content.push(alternative.clone());
        }
    }
    if null_count == 0 || content.is_empty() {
        return Ok(None);
    }
    if keyword == "oneOf" && null_count != 1 {
        return Err(unsupported_union(
            name,
            "nullable oneOf null branches overlap",
        ));
    }
    ensure_annotation_or_range_only(name, schema, keyword)?;
    let mut node = if content.len() == 1 {
        parse(name, &content[0], doc, active_refs)?
    } else {
        let mut reduced = schema.clone();
        let object = reduced.as_object_mut().ok_or_else(|| {
            unsupported_union(name, "nullable composition must be a schema object")
        })?;
        object.insert(keyword.to_string(), serde_json::Value::Array(content));
        for constraint_keyword in [
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "minItems",
            "maxItems",
            "minLength",
            "maxLength",
        ] {
            object.remove(constraint_keyword);
        }
        parse(name, &reduced, doc, active_refs)?
    };
    if node.json_any {
        if keyword == "oneOf" {
            return Err(unsupported_union(
                name,
                "nullable oneOf overlaps an unconstrained branch on null",
            ));
        }
        return Ok(Some(node));
    }
    if keyword == "oneOf" && (node.nullable || node.container_nullable) {
        // The retained composition already accepts null, so the outer exact
        // null branch makes null match twice. Its non-null domain is unchanged.
        node.nullable = false;
        node.container_nullable = false;
        return Ok(Some(node));
    }
    if node.repeating || matches!(node.kind, ir::SchemaKind::Group { .. }) {
        node.container_nullable = true;
    } else if node.is_scalar() {
        node.nullable = true;
    } else {
        return Ok(None);
    }
    Ok(Some(node))
}

fn is_null_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<bool, JsonFormatError> {
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        if files::ref_siblings_apply(schema) {
            reject_unsupported_ref_siblings(union_name, schema)?;
        }
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported_union(
                union_name,
                "nullable container alternatives cannot use cyclic references",
            ));
        }
        let Some(resolved) = resolve_ref(doc, reference) else {
            return Ok(false);
        };
        active_refs.push(reference.to_string());
        let is_null = is_null_alternative(union_name, resolved, doc, active_refs);
        active_refs.pop();
        return is_null;
    }
    Ok(schema.get("type").and_then(serde_json::Value::as_str) == Some("null"))
}

fn classify_scalar_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<ScalarAlternative, JsonFormatError> {
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        if files::ref_siblings_apply(schema) {
            reject_unsupported_ref_siblings(union_name, schema)?;
        }
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported_union(
                union_name,
                "nullable scalar alternatives cannot use cyclic references",
            ));
        }
        let Some(resolved) = resolve_ref(doc, reference) else {
            return Err(unsupported_union(
                union_name,
                "nullable scalar alternatives require document-local references",
            ));
        };
        active_refs.push(reference.to_string());
        let classified = classify_scalar_alternative(union_name, resolved, doc, active_refs);
        active_refs.pop();
        return classified;
    }
    let Some(ty) = schema.get("type").and_then(serde_json::Value::as_str) else {
        if schema.get("const").is_some() {
            return Err(unsupported_union(
                union_name,
                "nullable scalar alternatives cannot preserve const validation",
            ));
        }
        return Ok(ScalarAlternative::Other);
    };
    let classified = match ty {
        "null" => ScalarAlternative::Null,
        "string" => ScalarAlternative::Scalar(ScalarType::String),
        "integer" => ScalarAlternative::Scalar(ScalarType::Int),
        "number" => ScalarAlternative::Scalar(ScalarType::Float),
        "boolean" => ScalarAlternative::Scalar(ScalarType::Bool),
        _ => return Ok(ScalarAlternative::Other),
    };
    ensure_scalar_shape_only(union_name, schema)?;
    Ok(classified)
}

fn classify_exact_scalar_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<ExactScalarAlternative, JsonFormatError> {
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        let apply_siblings = files::ref_siblings_apply(schema);
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported_union(
                union_name,
                "homogeneous scalar alternatives cannot use cyclic references",
            ));
        }
        let Some(resolved) = resolve_ref(doc, reference) else {
            return Err(unsupported_union(
                union_name,
                "homogeneous scalar alternatives require document-local references",
            ));
        };
        active_refs.push(reference.to_string());
        let classified = classify_exact_scalar_alternative(union_name, resolved, doc, active_refs);
        active_refs.pop();
        let mut classified = classified?;
        if apply_siblings && !matches!(classified, ExactScalarAlternative::Other) {
            ensure_annotation_or_format_only(union_name, schema, "$ref")?;
            let sibling_format = formats::validate(union_name, schema)?;
            if let ExactScalarAlternative::Scalar {
                ty: ScalarType::String,
                formats: annotations,
                string_length,
            } = &mut classified
            {
                if let Some(format) = sibling_format {
                    formats::accumulate(
                        union_name,
                        annotations,
                        core::iter::once(format.to_string()),
                    )?;
                }
                *string_length = string_lengths::intersect(
                    union_name,
                    *string_length,
                    string_lengths::parse(union_name, schema)?,
                )?;
            } else {
                string_lengths::validate_ignored(union_name, schema)?;
            }
        }
        return Ok(classified);
    }
    let Some(ty) = schema.get("type").and_then(serde_json::Value::as_str) else {
        return Ok(ExactScalarAlternative::Other);
    };
    let format = formats::validate(union_name, schema)?;
    let string_length = string_lengths::parse(union_name, schema)?;
    let classified = match ty {
        "null" => ExactScalarAlternative::Null,
        "string" => {
            let mut annotations = ir::JsonFormatAnnotations::default();
            if let Some(format) = format {
                formats::accumulate(
                    union_name,
                    &mut annotations,
                    core::iter::once(format.to_string()),
                )?;
            }
            ExactScalarAlternative::Scalar {
                ty: ScalarType::String,
                formats: annotations,
                string_length,
            }
        }
        "integer" => ExactScalarAlternative::Scalar {
            ty: ScalarType::Int,
            formats: Default::default(),
            string_length: None,
        },
        "number" => ExactScalarAlternative::Scalar {
            ty: ScalarType::Float,
            formats: Default::default(),
            string_length: None,
        },
        "boolean" => ExactScalarAlternative::Scalar {
            ty: ScalarType::Bool,
            formats: Default::default(),
            string_length: None,
        },
        _ => return Ok(ExactScalarAlternative::Other),
    };
    ensure_exact_scalar_shape(union_name, schema)?;
    Ok(classified)
}

fn classify_exact_array_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<ArrayAlternative, JsonFormatError> {
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        let apply_siblings = files::ref_siblings_apply(schema);
        if apply_siblings {
            reject_unsupported_ref_siblings(union_name, schema)?;
        }
        if active_refs.iter().any(|active| active == reference) {
            return Err(unsupported_union(
                union_name,
                "array anyOf alternatives cannot use cyclic references",
            ));
        }
        let Some(resolved) = resolve_ref(doc, reference) else {
            return Err(unsupported_union(
                union_name,
                "array anyOf alternatives require document-local references",
            ));
        };
        active_refs.push(reference.to_string());
        let classified = classify_exact_array_alternative(union_name, resolved, doc, active_refs);
        active_refs.pop();
        let classified = classified?;
        if apply_siblings && matches!(classified, ArrayAlternative::Array(_)) {
            return parse(union_name, schema, doc, active_refs)
                .map(Box::new)
                .map(ArrayAlternative::Array);
        }
        return Ok(classified);
    }
    match schema.get("type").and_then(serde_json::Value::as_str) {
        Some("null") => {
            ensure_exact_scalar_shape(union_name, schema)?;
            Ok(ArrayAlternative::Null)
        }
        Some("array") => {
            ensure_exact_array_shape(union_name, schema)?;
            Ok(ArrayAlternative::Array(Box::new(parse(
                union_name,
                schema,
                doc,
                active_refs,
            )?)))
        }
        _ => Ok(ArrayAlternative::Other),
    }
}

fn ensure_exact_scalar_shape(
    union_name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "homogeneous scalar alternatives must be schema objects",
        ));
    };
    if let Some(keyword) = object.keys().find(|keyword| {
        keyword.as_str() != "type"
            && keyword.as_str() != "format"
            && keyword.as_str() != "minLength"
            && keyword.as_str() != "maxLength"
            && !is_annotation_keyword(keyword.as_str())
    }) {
        return Err(unsupported_union(
            union_name,
            &format!("homogeneous scalar alternatives cannot preserve `{keyword}` validation"),
        ));
    }
    Ok(())
}

fn ensure_exact_array_shape(
    union_name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "array anyOf alternatives must be schema objects",
        ));
    };
    if !object.contains_key("items") {
        return Err(unsupported_union(
            union_name,
            "array anyOf alternatives require an explicit item schema",
        ));
    }
    if let Some(keyword) = object.keys().find(|keyword| {
        !matches!(keyword.as_str(), "type" | "items" | "minItems" | "maxItems")
            && !is_annotation_keyword(keyword.as_str())
    }) {
        return Err(unsupported_union(
            union_name,
            &format!("array anyOf alternatives cannot preserve `{keyword}` validation"),
        ));
    }
    Ok(())
}

fn ensure_scalar_shape_only(
    union_name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "nullable composition must be a schema object",
        ));
    };
    if let Some(keyword) = object.keys().find(|keyword| {
        keyword.as_str() != "type"
            && !is_annotation_keyword(keyword.as_str())
            && !is_scalar_validation_keyword(keyword.as_str())
    }) {
        return Err(unsupported_union(
            union_name,
            &format!("nullable scalar alternative uses unsupported `{keyword}` composition"),
        ));
    }
    Ok(())
}

fn is_scalar_validation_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "const"
            | "enum"
            | "format"
            | "pattern"
            | "minLength"
            | "maxLength"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "multipleOf"
            | "contentEncoding"
            | "contentMediaType"
            | "contentSchema"
    )
}

fn ensure_annotation_or_format_only(
    union_name: &str,
    schema: &serde_json::Value,
    shape_keyword: &str,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "scalar alternatives must be schema objects",
        ));
    };
    if let Some(keyword) = object.keys().find(|keyword| {
        keyword.as_str() != shape_keyword
            && keyword.as_str() != "format"
            && keyword.as_str() != "minLength"
            && keyword.as_str() != "maxLength"
            && !files::is_internal_ref_keyword(keyword)
            && !is_annotation_keyword(keyword.as_str())
    }) {
        return Err(unsupported_union(
            union_name,
            &format!("scalar composition cannot preserve `{keyword}` validation"),
        ));
    }
    Ok(())
}

fn ensure_annotation_or_range_only(
    union_name: &str,
    schema: &serde_json::Value,
    shape_keyword: &str,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "nullable scalar alternatives must be schema objects",
        ));
    };
    if let Some(keyword) = object.keys().find(|keyword| {
        keyword.as_str() != shape_keyword
            && !is_annotation_keyword(keyword.as_str())
            && !matches!(
                keyword.as_str(),
                "format"
                    | "minimum"
                    | "maximum"
                    | "exclusiveMinimum"
                    | "exclusiveMaximum"
                    | "minItems"
                    | "maxItems"
                    | "minLength"
                    | "maxLength"
            )
    }) {
        return Err(unsupported_union(
            union_name,
            &format!("nullable composition cannot preserve `{keyword}` validation"),
        ));
    }
    Ok(())
}

fn is_annotation_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "$schema"
            | "$id"
            | "id"
            | "$anchor"
            | "$dynamicAnchor"
            | "$comment"
            | "$defs"
            | "definitions"
            | "title"
            | "description"
            | "default"
            | "deprecated"
            | "readOnly"
            | "writeOnly"
            | "examples"
    )
}

pub(super) fn parse_object_alternatives(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    mode: GroupAlternativeMode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let keyword = match mode {
        GroupAlternativeMode::Exclusive => "oneOf",
        GroupAlternativeMode::Inclusive => "anyOf",
    };
    let alternatives = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
        .ok_or_else(|| {
            unsupported_union(
                name,
                &format!("{keyword} must contain at least two alternatives"),
            )
        })?;
    let mut base_children = parse_alternative_properties(schema, doc, active_refs)?;
    let base_required = required_names(schema);
    let base_additional = match schema.get("additionalProperties") {
        None | Some(serde_json::Value::Bool(true)) => WrapperAdditional::Open,
        Some(serde_json::Value::Bool(false)) => WrapperAdditional::Closed,
        Some(additional @ serde_json::Value::Object(_)) => {
            WrapperAdditional::Typed(Box::new(parse("*", additional, doc, active_refs)?))
        }
        Some(_) => {
            return Err(unsupported_union(
                name,
                "alternative wrapper additionalProperties must be a boolean or schema",
            ));
        }
    };
    let base_constraints = scalar_constraints(name, schema, &base_children)?;
    clear_constrained_fixed(&mut base_children, &base_constraints);
    let mut merged = base_children.clone();
    let mut metadata = Vec::with_capacity(alternatives.len());
    for (index, alternative_schema) in alternatives.iter().enumerate() {
        if alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .is_some()
            && files::ref_siblings_apply(alternative_schema)
        {
            reject_unsupported_ref_siblings(name, alternative_schema)?;
        }
        if alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            formats::validate(name, alternative_schema)?;
        }
        let resolved = alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .and_then(|reference| resolve_ref(doc, reference))
            .unwrap_or(alternative_schema);
        if !core::ptr::eq(resolved, alternative_schema) {
            formats::validate(name, resolved)?;
        }
        let alternative_name = files::ref_siblings_apply(alternative_schema)
            .then(|| {
                alternative_schema
                    .get("title")
                    .and_then(serde_json::Value::as_str)
            })
            .flatten()
            .or_else(|| resolved.get("title").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .or_else(|| {
                alternative_schema
                    .get("$ref")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|reference| reference.rsplit('/').next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("{keyword}{index}"));
        let normalized = without_direct_property_constraints(resolved);
        let parsed = parse(&alternative_name, &normalized, doc, active_refs)?;
        if parsed.repeating {
            return Err(unsupported_union(
                name,
                "array alternatives are not supported",
            ));
        }
        let nested_mode = parsed.alternative_mode();
        let SchemaKind::Group {
            children: mut variant_children,
            alternatives: nested_alternatives,
            ..
        } = parsed.kind
        else {
            return Err(unsupported_union(
                name,
                "only object alternatives are supported",
            ));
        };
        if !nested_alternatives.is_empty() {
            if nested_mode != mode && !alternatives_are_pairwise_disjoint(&nested_alternatives) {
                return Err(unsupported_union(
                    name,
                    "cross-mode nested object alternatives must be provably mutually exclusive",
                ));
            }
            for mut nested in nested_alternatives {
                nested.name = format!("{alternative_name}/{}", nested.name);
                merge_nested_alternative(
                    name,
                    mode,
                    &base_children,
                    &base_required,
                    &base_constraints,
                    &base_additional,
                    &variant_children,
                    nested,
                    &mut merged,
                    &mut metadata,
                )?;
            }
            continue;
        }
        if resolved.get("additionalProperties") != Some(&serde_json::Value::Bool(false)) {
            return Err(unsupported_union(
                name,
                "object alternatives must declare additionalProperties false",
            ));
        }
        let mut required = base_required.clone();
        for field in required_names(resolved) {
            if !required.contains(&field) {
                required.push(field);
            }
        }
        let constraints = scalar_constraints(name, resolved, &variant_children)?;
        clear_constrained_fixed(&mut variant_children, &constraints);
        let constraints = merge_constraints(name, &base_constraints, constraints)?;
        let mut members = Vec::new();
        for child in variant_children {
            if let Some(base) = base_children.iter().find(|base| base.name == child.name)
                && base != &child
            {
                return Err(unsupported_union(
                    name,
                    &format!(
                        "field `{}` has incompatible wrapper and alternative schemas",
                        child.name
                    ),
                ));
            }
            let allowed = wrapper_allows(name, &base_children, &base_additional, &child)?;
            if allowed {
                if let Some(existing) = merged
                    .iter_mut()
                    .find(|existing| existing.name == child.name)
                {
                    if existing != &child
                        && !merge_exact_constrained_nullability(
                            existing,
                            &child,
                            &constraints,
                            &metadata,
                        )
                    {
                        return Err(unsupported_union(
                            name,
                            &format!(
                                "field `{}` has incompatible schemas across alternatives",
                                child.name
                            ),
                        ));
                    }
                } else {
                    merged.push(child.clone());
                }
                if !members.contains(&child.name) {
                    members.push(child.name);
                }
            }
        }
        push_alternative(
            name,
            mode,
            GroupAlternative {
                name: alternative_name,
                members,
                required,
                constraints,
            },
            &mut metadata,
        )?;
    }
    merged.retain(|child| {
        metadata
            .iter()
            .any(|alternative| alternative.members.contains(&child.name))
    });
    let group = SchemaNode::group(name, merged);
    match mode {
        GroupAlternativeMode::Exclusive => group.with_alternatives(metadata),
        GroupAlternativeMode::Inclusive => group.with_inclusive_alternatives(metadata),
    }
    .ok_or_else(|| unsupported_union(name, "alternative metadata is internally inconsistent"))
}

fn merge_exact_constrained_nullability(
    existing: &mut SchemaNode,
    incoming: &SchemaNode,
    incoming_constraints: &[GroupAlternativeConstraint],
    previous: &[GroupAlternative],
) -> bool {
    if existing.nullable == incoming.nullable
        || !incoming_constraints
            .iter()
            .any(|constraint| constraint.member == incoming.name)
        || previous.iter().any(|alternative| {
            alternative.members.contains(&incoming.name)
                && !alternative
                    .constraints
                    .iter()
                    .any(|constraint| constraint.member == incoming.name)
        })
    {
        return false;
    }
    let mut normalized_existing = existing.clone();
    normalized_existing.nullable = false;
    let mut normalized_incoming = incoming.clone();
    normalized_incoming.nullable = false;
    if normalized_existing != normalized_incoming {
        return false;
    }
    existing.nullable = true;
    true
}

fn merge_constraints(
    union_name: &str,
    base: &[GroupAlternativeConstraint],
    nested: Vec<GroupAlternativeConstraint>,
) -> Result<Vec<GroupAlternativeConstraint>, JsonFormatError> {
    let mut merged = base.to_vec();
    for constraint in nested {
        if let Some(previous) = merged
            .iter()
            .find(|previous| previous.member == constraint.member)
        {
            if previous.value != constraint.value {
                return Err(unsupported_union(
                    union_name,
                    &format!(
                        "const discriminator `{}` conflicts with its wrapper constraint",
                        constraint.member
                    ),
                ));
            }
        } else {
            merged.push(constraint);
        }
    }
    Ok(merged)
}

fn wrapper_allows(
    union_name: &str,
    base_children: &[SchemaNode],
    additional: &WrapperAdditional,
    child: &SchemaNode,
) -> Result<bool, JsonFormatError> {
    if base_children.iter().any(|base| base.name == child.name) {
        return Ok(true);
    }
    match additional {
        WrapperAdditional::Open => Ok(true),
        WrapperAdditional::Closed => Ok(false),
        WrapperAdditional::Typed(expected) => {
            let mut expected = expected.as_ref().clone();
            expected.name = child.name.clone();
            if expected == *child {
                Ok(true)
            } else {
                Err(unsupported_union(
                    union_name,
                    &format!(
                        "field `{}` does not match the alternative wrapper's typed additionalProperties schema",
                        child.name
                    ),
                ))
            }
        }
    }
}

fn alternatives_are_pairwise_disjoint(alternatives: &[GroupAlternative]) -> bool {
    alternatives.iter().enumerate().all(|(index, left)| {
        alternatives[index + 1..]
            .iter()
            .all(|right| alternatives_are_disjoint(left, right))
    })
}

fn alternatives_are_disjoint(left: &GroupAlternative, right: &GroupAlternative) -> bool {
    left.required
        .iter()
        .any(|required| !right.members.contains(required))
        || right
            .required
            .iter()
            .any(|required| !left.members.contains(required))
        || left.constraints.iter().any(|left_constraint| {
            right.constraints.iter().any(|right_constraint| {
                left_constraint.member == right_constraint.member
                    && left_constraint.value != right_constraint.value
                    && (left.required.contains(&left_constraint.member)
                        || right.required.contains(&right_constraint.member))
            })
        })
}

#[allow(clippy::too_many_arguments)]
fn merge_nested_alternative(
    union_name: &str,
    mode: GroupAlternativeMode,
    base_children: &[SchemaNode],
    base_required: &[String],
    base_constraints: &[GroupAlternativeConstraint],
    base_additional: &WrapperAdditional,
    variant_children: &[SchemaNode],
    alternative: GroupAlternative,
    merged: &mut Vec<SchemaNode>,
    metadata: &mut Vec<GroupAlternative>,
) -> Result<(), JsonFormatError> {
    let mut members = Vec::new();
    for member in &alternative.members {
        let child = variant_children
            .iter()
            .find(|child| child.name == *member)
            .ok_or_else(|| {
                unsupported_union(
                    union_name,
                    &format!("nested union member `{member}` has no declared field"),
                )
            })?;
        if let Some(base) = base_children.iter().find(|base| base.name == child.name)
            && base != child
        {
            return Err(unsupported_union(
                union_name,
                &format!(
                    "field `{}` has incompatible wrapper and alternative schemas",
                    child.name
                ),
            ));
        }
        let allowed = wrapper_allows(union_name, base_children, base_additional, child)?;
        if !allowed {
            continue;
        }
        if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
            if existing != child {
                return Err(unsupported_union(
                    union_name,
                    &format!(
                        "field `{}` has incompatible schemas across alternatives",
                        child.name
                    ),
                ));
            }
        } else {
            merged.push(child.clone());
        }
        if !members.contains(member) {
            members.push(member.clone());
        }
    }
    let mut required = base_required.to_vec();
    for member in alternative.required {
        if !required.contains(&member) {
            required.push(member);
        }
    }
    let constraints = merge_constraints(union_name, base_constraints, alternative.constraints)?;
    push_alternative(
        union_name,
        mode,
        GroupAlternative {
            name: alternative.name,
            members,
            required,
            constraints,
        },
        metadata,
    )
}

fn push_alternative(
    union_name: &str,
    mode: GroupAlternativeMode,
    alternative: GroupAlternative,
    metadata: &mut Vec<GroupAlternative>,
) -> Result<(), JsonFormatError> {
    let keyword = match mode {
        GroupAlternativeMode::Exclusive => "oneOf",
        GroupAlternativeMode::Inclusive => "anyOf",
    };
    if alternative
        .required
        .iter()
        .any(|field| !alternative.members.iter().any(|member| member == field))
    {
        return Err(unsupported_union(
            union_name,
            &format!("{keyword} requires a field not declared by that object alternative"),
        ));
    }
    if mode == GroupAlternativeMode::Exclusive
        && metadata.iter().any(|previous| {
            previous.members == alternative.members
                && previous.required == alternative.required
                && previous.constraints == alternative.constraints
        })
    {
        return Err(unsupported_union(
            union_name,
            "alternatives are not distinguishable by supported object fields and requirements",
        ));
    }
    if metadata
        .iter()
        .any(|previous| previous.name == alternative.name)
    {
        return Err(unsupported_union(
            union_name,
            &format!("{keyword} alternatives must have distinct names"),
        ));
    }
    metadata.push(alternative);
    Ok(())
}

fn scalar_constraints(
    union_name: &str,
    schema: &serde_json::Value,
    children: &[SchemaNode],
) -> Result<Vec<GroupAlternativeConstraint>, JsonFormatError> {
    let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(Vec::new());
    };
    properties
        .iter()
        .map(|(member, property)| {
            discriminator_value(union_name, member, property)
                .map(|value| value.map(|value| (member, value)))
        })
        .filter_map(Result::transpose)
        .map(|result| {
            let (member, value) = result?;
            let child = children
                .iter()
                .find(|child| child.name == *member)
                .ok_or_else(|| {
                    unsupported_union(
                        union_name,
                        &format!("scalar discriminator `{member}` has no declared scalar field"),
                    )
                })?;
            if child.repeating {
                return Err(unsupported_union(
                    union_name,
                    &format!("scalar discriminator `{member}` cannot be an array"),
                ));
            }
            let constraint = match child.kind {
                SchemaKind::Scalar { ty } => {
                    constraint_value(union_name, member, value, ty, child.nullable)
                }
                SchemaKind::ScalarUnion { types } => {
                    union_constraint_value(union_name, member, value, types, child.nullable)
                }
                SchemaKind::Group { .. } => Err(unsupported_union(
                    union_name,
                    &format!("scalar discriminator `{member}` must be a scalar field"),
                )),
            };
            let value = constraint?;
            Ok(GroupAlternativeConstraint {
                member: member.clone(),
                value,
            })
        })
        .collect()
}

fn clear_constrained_fixed(
    children: &mut [SchemaNode],
    constraints: &[GroupAlternativeConstraint],
) {
    for constraint in constraints {
        if let Some(child) = children
            .iter_mut()
            .find(|child| child.name == constraint.member)
        {
            child.fixed = None;
        }
    }
}

fn parse_alternative_properties(
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<Vec<SchemaNode>, JsonFormatError> {
    schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, property)| {
                    parse(name, &without_scalar_constraint(property), doc, active_refs)
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn without_direct_property_constraints(schema: &serde_json::Value) -> serde_json::Value {
    let mut schema = schema.clone();
    let Some(properties) = schema
        .as_object_mut()
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
    else {
        return schema;
    };
    for property in properties.values_mut() {
        *property = without_scalar_constraint(property);
    }
    schema
}

fn without_scalar_constraint(schema: &serde_json::Value) -> serde_json::Value {
    let mut schema = schema.clone();
    if let Some(schema) = schema.as_object_mut() {
        if !schema.contains_key("type") {
            let value = schema.get("const").or_else(|| {
                schema
                    .get("enum")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|values| values.first().filter(|_| values.len() == 1))
            });
            let inferred = match value {
                Some(serde_json::Value::String(_)) => Some("string"),
                Some(serde_json::Value::Bool(_)) => Some("boolean"),
                Some(serde_json::Value::Number(number)) if number.as_i64().is_some() => {
                    Some("integer")
                }
                Some(serde_json::Value::Number(_)) => Some("number"),
                _ => None,
            };
            if let Some(inferred) = inferred {
                schema.insert("type".into(), inferred.into());
            }
        }
        schema.remove("const");
        schema.remove("enum");
    }
    schema
}

fn discriminator_value<'a>(
    union_name: &str,
    member: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<&'a serde_json::Value>, JsonFormatError> {
    let constant = schema.get("const");
    let values = match schema.get("enum") {
        None => None,
        Some(serde_json::Value::Array(values)) if values.is_empty() => {
            return Err(unsupported_union(
                union_name,
                &format!("enum discriminator `{member}` has no possible values"),
            ));
        }
        Some(serde_json::Value::Array(values)) => Some(values),
        Some(_) => {
            return Err(unsupported_union(
                union_name,
                &format!("enum discriminator `{member}` must be an array"),
            ));
        }
    };
    if let Some(constant) = constant {
        if values.is_some_and(|values| !values.contains(constant)) {
            return Err(unsupported_union(
                union_name,
                &format!(
                    "scalar discriminator `{member}` has incompatible const and enum constraints"
                ),
            ));
        }
        return Ok(Some(constant));
    }
    Ok(values.and_then(|values| (values.len() == 1).then(|| &values[0])))
}

fn union_constraint_value(
    union_name: &str,
    member: &str,
    value: &serde_json::Value,
    types: ScalarTypeSet,
    nullable: bool,
) -> Result<GroupAlternativeConstraintValue, JsonFormatError> {
    let selected = match value {
        serde_json::Value::String(_) if types.contains(ScalarType::String) => ScalarType::String,
        serde_json::Value::Bool(_) if types.contains(ScalarType::Bool) => ScalarType::Bool,
        serde_json::Value::Number(value)
            if value.as_i64().is_some() && types.contains(ScalarType::Int) =>
        {
            ScalarType::Int
        }
        serde_json::Value::Number(_) if types.contains(ScalarType::Float) => ScalarType::Float,
        serde_json::Value::Null if nullable => {
            return Ok(GroupAlternativeConstraintValue::JsonNull);
        }
        _ => {
            return Err(unsupported_union(
                union_name,
                &format!(
                    "scalar discriminator `{member}` does not match any declared scalar union type"
                ),
            ));
        }
    };
    constraint_value(union_name, member, value, selected, nullable)
}

fn constraint_value(
    union_name: &str,
    member: &str,
    value: &serde_json::Value,
    ty: ScalarType,
    nullable: bool,
) -> Result<GroupAlternativeConstraintValue, JsonFormatError> {
    let unsupported = |reason: &str| {
        unsupported_union(
            union_name,
            &format!("scalar discriminator `{member}` {reason}"),
        )
    };
    match (ty, value) {
        (ScalarType::String, serde_json::Value::String(value)) => {
            Ok(GroupAlternativeConstraintValue::String(value.clone()))
        }
        (ScalarType::Int, serde_json::Value::Number(value)) => value
            .as_i64()
            .map(GroupAlternativeConstraintValue::Int)
            .ok_or_else(|| unsupported("must be a signed 64-bit integer")),
        (ScalarType::Float, serde_json::Value::Number(value)) => finite_f64(value)
            .and_then(FiniteF64::new)
            .map(GroupAlternativeConstraintValue::Float)
            .ok_or_else(|| unsupported("must be a finite exactly supported number")),
        (ScalarType::Bool, serde_json::Value::Bool(value)) => {
            Ok(GroupAlternativeConstraintValue::Bool(*value))
        }
        (_, serde_json::Value::Null) if nullable => Ok(GroupAlternativeConstraintValue::JsonNull),
        (_, serde_json::Value::Null) => Err(unsupported(
            "can be null only when its scalar type explicitly includes null",
        )),
        _ => Err(unsupported("does not match its declared scalar type")),
    }
}

fn finite_f64(number: &serde_json::Number) -> Option<f64> {
    crate::exact_f64_from_json_number(number)
}

fn required_names(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}
