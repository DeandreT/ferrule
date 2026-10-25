use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    GroupAlternativeMode, JsonAllowedValue, ScalarType, ScalarTypeSet, SchemaKind, SchemaNode,
};

use super::{
    allowed_values, contains, dependent_schemas, files, formats, item_counts, multiples, parse,
    patterns, property_counts, property_dependencies, property_names, push_active_ref, ranges,
    reject_unsupported_ref_siblings, resolve_ref, unsupported_union,
};
use crate::JsonFormatError;

mod arrays;
mod object;

pub(super) use arrays::parse_scalar_domain_array_any_of;
use arrays::{set_container_nullable, union_string_length_range_set};
pub(super) use object::parse_object_alternatives;

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
        nullable: bool,
        formats: ir::JsonFormatAnnotations,
        numeric_range: Option<ir::NumericRange>,
        multiple_of: Option<ir::JsonMultipleOfConstraints>,
        string_length: Option<ir::StringLengthRange>,
        patterns: Option<ir::JsonPatternConstraints>,
    },
    Other,
}

enum ArrayAlternative {
    Null,
    Array(Box<SchemaNode>),
    Other,
}

/// Imports a scalar composition whose non-null branches are all finite
/// `const`/`enum` domains. Inclusive unions keep every value; exclusive unions
/// keep only values admitted by exactly one branch.
pub(super) fn parse_finite_scalar_composition(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    keyword: &str,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let Some(alternatives) = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
    else {
        return Ok(None);
    };
    let mut branches = Vec::with_capacity(alternatives.len());
    let mut annotations = ir::JsonFormatAnnotations::default();
    let mut saw_finite_non_null = false;
    let mut saw_unbounded_or_structured = false;
    for alternative in alternatives {
        if is_null_alternative(name, alternative, doc, active_refs)? {
            branches.push(vec![JsonAllowedValue::JsonNull]);
            continue;
        }
        if resolves_to_structured_alternative(alternative, doc, active_refs)? {
            saw_unbounded_or_structured = true;
            continue;
        }
        let parsed = parse(name, alternative, doc, active_refs)?;
        if parsed.repeating || parsed.json_any || !parsed.is_scalar() {
            saw_unbounded_or_structured = true;
            continue;
        }
        let Some(values) = allowed_values::from_schema(&parsed)? else {
            saw_unbounded_or_structured = true;
            continue;
        };
        saw_finite_non_null = true;
        formats::accumulate(
            name,
            &mut annotations,
            parsed.json_formats.clone().into_vec(),
        )?;
        branches.push(allowed_values::retained_by_constraints(&parsed, values));
    }
    if !saw_finite_non_null {
        return Ok(None);
    }
    if saw_unbounded_or_structured {
        return Err(unsupported_union(
            name,
            &format!(
                "scalar {keyword} mixes finite const/enum branches with an unbounded or structured branch"
            ),
        ));
    }
    let outer_type = scalar_type_sibling(name, schema, false)?;
    ensure_annotation_or_format_only(name, schema, keyword, outer_type.is_some())?;
    let mut candidates = Vec::new();
    for branch in &branches {
        for value in branch {
            if !candidates
                .iter()
                .any(|candidate: &JsonAllowedValue| candidate.semantically_equals(value))
            {
                candidates.push(value.clone());
            }
        }
    }
    if keyword == "oneOf" {
        candidates.retain(|value| {
            branches
                .iter()
                .filter(|branch| {
                    branch
                        .iter()
                        .any(|candidate| candidate.semantically_equals(value))
                })
                .count()
                == 1
        });
    }
    if outer_type.is_some_and(|domain| !candidates.iter().all(|value| domain.contains_value(value)))
    {
        return Err(type_sibling_error(name, false));
    }
    let mut node = allowed_values::schema_from_values(name, candidates)?;
    if node.accepts_scalar_type(ScalarType::String) {
        node.json_formats = annotations;
    }
    Ok(Some(node))
}

fn resolves_to_structured_alternative(
    schema: &serde_json::Value,
    doc: &super::files::LoadedSchema,
    active_refs: &[String],
) -> Result<bool, JsonFormatError> {
    let mut schema = schema;
    let mut visited = Vec::new();
    while let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        if active_refs.iter().any(|active| active == reference) || visited.contains(&reference) {
            return Ok(false);
        }
        let Some(resolved) = resolve_ref(doc, reference) else {
            return Ok(false);
        };
        if active_refs.len() + visited.len() >= files::MAX_REFERENCE_DEPTH {
            return Err(JsonFormatError::SchemaResourceLimit {
                kind: "reference depth",
                limit: files::MAX_REFERENCE_DEPTH,
            });
        }
        visited.push(reference);
        schema = resolved;
    }
    Ok(matches!(
        schema.get("type").and_then(serde_json::Value::as_str),
        Some("object" | "array")
    ) || schema.get("properties").is_some()
        || schema.get("items").is_some())
}

/// Imports an inclusive union made entirely from exact scalar type branches.
/// Duplicate branches collapse, while distinct types become a first-class
/// scalar union and retain their runtime value tags.
pub(super) fn parse_scalar_any_of(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    doc: &super::files::LoadedSchema,
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
    doc: &super::files::LoadedSchema,
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
    doc: &super::files::LoadedSchema,
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
    let mut string_branches = 0_usize;
    let mut integer_branches = 0_usize;
    let mut numeric_types = Vec::new();
    let mut numeric_ranges = Vec::new();
    let mut numeric_multiples = Vec::new();
    let mut string_lengths = Vec::new();
    let mut string_patterns = Vec::new();
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
                nullable: branch_nullable,
                formats: branch_formats,
                numeric_range,
                multiple_of,
                string_length,
                patterns: branch_patterns,
            } => {
                if exclusive && branch_nullable && nullable {
                    return Err(overlapping_scalar_one_of(name));
                }
                nullable |= branch_nullable;
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
                    string_branches += 1;
                    string_lengths.push(string_length);
                    string_patterns.push(branch_patterns);
                }
                if ty == ScalarType::Int {
                    integer_branches += 1;
                }
                if matches!(ty, ScalarType::Int | ScalarType::Float) {
                    numeric_types.push(ty);
                    numeric_ranges.push(numeric_range);
                    numeric_multiples.push(multiple_of);
                }
            }
            ExactScalarAlternative::Other => return Ok(None),
        }
    }
    let Some(first) = scalar_types.first().copied() else {
        return Ok(None);
    };
    let outer_type = scalar_type_sibling(name, schema, false)?;
    if outer_type.is_some_and(|domain| !domain.contains_types(&scalar_types, nullable)) {
        return Err(type_sibling_error(name, false));
    }
    ensure_annotation_or_format_only(name, schema, keyword, outer_type.is_some())?;
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
    if !numeric_ranges.is_empty() {
        let ranges_are_independent = numeric_ranges
            .first()
            .is_none_or(|first| numeric_ranges.iter().all(|candidate| candidate == first));
        let multiples_are_independent = numeric_multiples
            .first()
            .is_none_or(|first| numeric_multiples.iter().all(|candidate| candidate == first));
        if !ranges_are_independent && !multiples_are_independent {
            return Err(unsupported_union(
                name,
                &format!(
                    "scalar {keyword} correlates different numeric-range and multipleOf constraints"
                ),
            ));
        }
        let heterogeneous_numeric_types =
            numeric_types.contains(&ScalarType::Int) && numeric_types.contains(&ScalarType::Float);
        if heterogeneous_numeric_types && !multiples_are_independent {
            return Err(unsupported_union(
                name,
                &format!(
                    "scalar {keyword} correlates different numeric types and multipleOf constraints"
                ),
            ));
        }
        let exact_string_int_interval = node.is_string_int_union()
            && string_branches == 1
            && integer_branches == 1
            && matches!(
                numeric_ranges.as_slice(),
                [Some(ir::NumericRange::Integer(_))]
            );
        let exact_string_float_interval = node.is_string_float_union()
            && string_branches == 1
            && numeric_types.as_slice() == [ScalarType::Float]
            && matches!(
                numeric_ranges.as_slice(),
                [Some(ir::NumericRange::Number(_))]
            )
            && numeric_multiples.iter().all(Option::is_none)
            && string_lengths.iter().all(Option::is_none)
            && string_patterns.iter().all(Option::is_none);
        if matches!(node.kind, SchemaKind::ScalarUnion { .. })
            && numeric_ranges.iter().any(Option::is_some)
            && !exact_string_int_interval
            && !exact_string_float_interval
        {
            return Err(unsupported_union(
                name,
                &format!(
                    "scalar {keyword} numeric ranges cannot be attached to a heterogeneous scalar union"
                ),
            ));
        }
        node.numeric_range = if ranges_are_independent {
            numeric_ranges.first().copied().flatten()
        } else {
            ranges::union(name, numeric_ranges)?
        };
        node.json_multiple_of = if multiples_are_independent {
            numeric_multiples.first().cloned().flatten()
        } else {
            multiples::union(name, numeric_multiples)?
        };
    }
    if node.accepts_scalar_type(ScalarType::String) {
        let lengths_are_independent = string_lengths
            .first()
            .is_none_or(|first| string_lengths.iter().all(|candidate| candidate == first));
        let patterns_are_independent = string_patterns.first().is_none_or(|first| {
            string_patterns
                .iter()
                .all(|candidate| patterns::equivalent(candidate.as_ref(), first.as_ref()))
        });
        if !lengths_are_independent && !patterns_are_independent {
            return Err(unsupported_union(
                name,
                &format!(
                    "scalar {keyword} correlates different string-length and pattern constraints"
                ),
            ));
        }
        node.json_formats = format_annotations;
        node.string_length_range = union_string_length_range_set(name, string_lengths)?;
        node.json_patterns = patterns::union(name, string_patterns)?;
    }
    reject_string_float_interval_siblings(name, schema, &node)?;
    Ok(Some(node))
}

pub(super) fn reject_string_float_interval_siblings(
    name: &str,
    schema: &serde_json::Value,
    node: &SchemaNode,
) -> Result<(), JsonFormatError> {
    if !node.is_string_float_union()
        || !matches!(node.numeric_range, Some(ir::NumericRange::Number(_)))
    {
        return Ok(());
    }
    if let Some(keyword) = [
        "const",
        "enum",
        "minLength",
        "maxLength",
        "pattern",
        "multipleOf",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
    ]
    .into_iter()
    .find(|keyword| schema.get(*keyword).is_some())
    {
        return Err(unsupported_union(
            name,
            &format!(
                "String-or-Float interval composition cannot preserve outer `{keyword}` validation"
            ),
        ));
    }
    Ok(())
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
/// Canonicalizes the common nullable-scalar union spelling used by OpenAPI
/// and generated JSON Schemas. Structured nullability needs a distinct
/// instance variant, while scalar nullability maps exactly to
/// `SchemaNode::nullable`.
pub(super) fn parse_nullable_scalar_alternatives(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    keyword: &str,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let Some(alternatives) = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() == 2)
    else {
        return Ok(None);
    };
    let first_is_null = is_null_alternative(name, &alternatives[0], doc, active_refs)?;
    let second_is_null = is_null_alternative(name, &alternatives[1], doc, active_refs)?;
    let content = match (first_is_null, second_is_null) {
        (true, false) => &alternatives[1],
        (false, true) => &alternatives[0],
        _ => return Ok(None),
    };
    if content.get("allOf").is_some()
        || content.get("oneOf").is_some()
        || content.get("anyOf").is_some()
    {
        return Ok(None);
    }
    let ScalarAlternative::Scalar(ty) =
        classify_scalar_alternative(name, content, doc, active_refs)?
    else {
        return Ok(None);
    };
    let outer_type = scalar_type_sibling(name, schema, true)?;
    if outer_type.is_some_and(|domain| !domain.contains_types(&[ty], true)) {
        return Err(type_sibling_error(name, true));
    }
    ensure_annotation_or_range_only_with_type(name, schema, keyword, outer_type.is_some())?;
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
    doc: &super::files::LoadedSchema,
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
    let outer_type = scalar_type_sibling(name, schema, true)?;
    ensure_annotation_or_range_only_with_type(name, schema, keyword, outer_type.is_some())?;
    let mut node = if content.len() == 1 {
        parse(name, &content[0], doc, active_refs)?
    } else {
        let mut reduced = schema.clone();
        let object = reduced.as_object_mut().ok_or_else(|| {
            unsupported_union(name, "nullable composition must be a schema object")
        })?;
        object.insert(keyword.to_string(), serde_json::Value::Array(content));
        // Syntax has been checked. Prove the outer assertion against the
        // complete result after null/oneOf normalization, without narrowing
        // this intermediate composition.
        if outer_type.is_some() {
            object.remove("type");
        }
        for constraint_keyword in [
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "multipleOf",
            "minItems",
            "maxItems",
            "minProperties",
            "maxProperties",
            "dependencies",
            "dependentRequired",
            "minLength",
            "maxLength",
        ] {
            object.remove(constraint_keyword);
        }
        parse(name, &reduced, doc, active_refs)?
    };
    reject_string_float_interval_siblings(name, schema, &node)?;
    if node.json_any {
        if outer_type.is_some() {
            return Err(type_sibling_error(name, true));
        }
        if keyword == "oneOf" {
            return Err(unsupported_union(
                name,
                "nullable oneOf overlaps an unconstrained branch on null",
            ));
        }
        return Ok(Some(node));
    }
    if keyword == "oneOf"
        && (crate::null_layers::value_null(&node)? || crate::null_layers::container_null(&node)?)
    {
        // The retained composition already accepts null, so the outer exact
        // null branch makes null match twice. Its non-null domain is unchanged.
        node.nullable = false;
        set_container_nullable(&mut node, false)?;
        check_scalar_type_sibling(name, outer_type, &node, true)?;
        return Ok(Some(node));
    }
    if node.repeating || matches!(node.kind, ir::SchemaKind::Group { .. }) {
        set_container_nullable(&mut node, true)?;
    } else if node.is_scalar() {
        node.nullable = true;
    } else {
        return Ok(None);
    }
    check_scalar_type_sibling(name, outer_type, &node, true)?;
    reject_string_float_interval_siblings(name, schema, &node)?;
    Ok(Some(node))
}

fn is_null_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &super::files::LoadedSchema,
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
        push_active_ref(active_refs, reference)?;
        let is_null = is_null_alternative(union_name, resolved, doc, active_refs);
        active_refs.pop();
        return is_null;
    }
    let type_accepts_null = match schema.get("type") {
        None => true,
        Some(serde_json::Value::String(value)) => value == "null",
        Some(serde_json::Value::Array(values)) => values.iter().any(|value| value == "null"),
        Some(_) => false,
    };
    if !type_accepts_null {
        return Ok(false);
    }
    if schema.get("const").is_some() {
        return Ok(schema.get("const").is_some_and(serde_json::Value::is_null)
            && schema.get("enum").is_none_or(|values| {
                values
                    .as_array()
                    .is_some_and(|values| values.iter().any(serde_json::Value::is_null))
            }));
    }
    if let Some(values) = schema.get("enum").and_then(serde_json::Value::as_array) {
        return Ok(!values.is_empty() && values.iter().all(serde_json::Value::is_null));
    }
    Ok(schema.get("type").and_then(serde_json::Value::as_str) == Some("null"))
}

fn classify_scalar_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &super::files::LoadedSchema,
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
        push_active_ref(active_refs, reference)?;
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
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<ExactScalarAlternative, JsonFormatError> {
    if schema.get("allOf").is_some() {
        let parsed = parse(union_name, schema, doc, active_refs)?;
        return Ok(exact_scalar_from_node(parsed));
    }
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
        push_active_ref(active_refs, reference)?;
        let classified = classify_exact_scalar_alternative(union_name, resolved, doc, active_refs);
        active_refs.pop();
        let classified = classified?;
        if apply_siblings && !matches!(classified, ExactScalarAlternative::Other) {
            ensure_annotation_or_format_only(union_name, schema, "$ref", false)?;
            return parse(union_name, schema, doc, active_refs).map(exact_scalar_from_node);
        }
        return Ok(classified);
    }
    let Some(ty) = schema.get("type").and_then(serde_json::Value::as_str) else {
        return Ok(ExactScalarAlternative::Other);
    };
    if ty == "null" {
        ensure_exact_scalar_shape(union_name, schema)?;
        return Ok(ExactScalarAlternative::Null);
    }
    if !matches!(ty, "string" | "integer" | "number" | "boolean") {
        return Ok(ExactScalarAlternative::Other);
    }
    ensure_exact_scalar_shape(union_name, schema)?;
    parse(union_name, schema, doc, active_refs).map(exact_scalar_from_node)
}

fn exact_scalar_from_node(parsed: SchemaNode) -> ExactScalarAlternative {
    if parsed.repeating
        || parsed.json_any
        || parsed.fixed.is_some()
        || parsed.item_count_range.is_some()
    {
        return ExactScalarAlternative::Other;
    }
    let SchemaKind::Scalar { ty } = parsed.kind else {
        return ExactScalarAlternative::Other;
    };
    let Ok(nullable) = crate::null_layers::value_null(&parsed) else {
        return ExactScalarAlternative::Other;
    };
    ExactScalarAlternative::Scalar {
        ty,
        nullable,
        formats: parsed.json_formats,
        numeric_range: parsed.numeric_range,
        multiple_of: parsed.json_multiple_of,
        string_length: parsed.string_length_range,
        patterns: parsed.json_patterns,
    }
}

fn classify_exact_array_alternative(
    union_name: &str,
    schema: &serde_json::Value,
    doc: &super::files::LoadedSchema,
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
        push_active_ref(active_refs, reference)?;
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
            && keyword.as_str() != "pattern"
            && keyword.as_str() != "minimum"
            && keyword.as_str() != "maximum"
            && keyword.as_str() != "exclusiveMinimum"
            && keyword.as_str() != "exclusiveMaximum"
            && keyword.as_str() != "multipleOf"
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
        !matches!(
            keyword.as_str(),
            "type"
                | "items"
                | "prefixItems"
                | "additionalItems"
                | "minItems"
                | "maxItems"
                | "contains"
                | "minContains"
                | "maxContains"
                | "uniqueItems"
                | "minLength"
                | "maxLength"
                | "pattern"
                | "format"
        ) && !files::is_internal_ref_keyword(keyword)
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

/// JSON type assertions are a domain check, independent of Ferrule's runtime
/// scalar tags. A number declaration contains integers; the converse does not
/// hold for an unbounded Float domain.
#[derive(Clone, Copy)]
struct ScalarTypeSibling(u8);

impl ScalarTypeSibling {
    fn contains_type(self, ty: ScalarType) -> bool {
        match ty {
            ScalarType::String => self.0 & 1 != 0,
            ScalarType::Bool => self.0 & 2 != 0,
            ScalarType::Int => self.0 & (4 | 8) != 0,
            ScalarType::Float => self.0 & 8 != 0,
        }
    }

    fn contains_types(self, types: &[ScalarType], nullable: bool) -> bool {
        (!nullable || self.0 & 16 != 0) && types.iter().all(|ty| self.contains_type(*ty))
    }

    fn contains_value(self, value: &JsonAllowedValue) -> bool {
        match value {
            JsonAllowedValue::JsonNull => self.0 & 16 != 0,
            JsonAllowedValue::Float(value) if value.get().fract() == 0.0 => self.0 & (4 | 8) != 0,
            other => other.scalar_type().is_some_and(|ty| self.contains_type(ty)),
        }
    }
}

fn type_sibling_error(name: &str, nullable: bool) -> JsonFormatError {
    unsupported_union(
        name,
        if nullable {
            "nullable composition cannot preserve `type` validation"
        } else {
            "scalar composition cannot preserve `type` validation"
        },
    )
}

/// Validate syntax before any intermediate type omission. This deliberately
/// does not use schema_type(), whose absent-shape fallback also covers some
/// malformed declarations.
fn scalar_type_sibling(
    name: &str,
    schema: &serde_json::Value,
    nullable: bool,
) -> Result<Option<ScalarTypeSibling>, JsonFormatError> {
    let Some(value) = schema.get("type") else {
        return Ok(None);
    };
    let values = match value {
        serde_json::Value::String(_) => core::slice::from_ref(value),
        serde_json::Value::Array(values) if !values.is_empty() && values.len() <= 7 => {
            values.as_slice()
        }
        _ => return Err(type_sibling_error(name, nullable)),
    };
    let mut domain = 0_u8;
    for value in values {
        let bit = match value.as_str() {
            Some("string") => 1,
            Some("boolean") => 2,
            Some("integer") => 4,
            Some("number") => 8,
            Some("null") => 16,
            Some("object") => 32,
            Some("array") => 64,
            _ => return Err(type_sibling_error(name, nullable)),
        };
        if domain & bit != 0 {
            return Err(type_sibling_error(name, nullable));
        }
        domain |= bit;
    }
    Ok(Some(ScalarTypeSibling(domain)))
}

fn check_scalar_type_sibling(
    name: &str,
    outer_type: Option<ScalarTypeSibling>,
    node: &SchemaNode,
    nullable: bool,
) -> Result<(), JsonFormatError> {
    let Some(domain) = outer_type else {
        return Ok(());
    };
    let layers = crate::null_layers::layers(node)?;
    let contained = !node.repeating
        && !node.json_any
        && !layers.container_null
        && (!layers.value_null || domain.0 & 16 != 0)
        && match node.kind {
            SchemaKind::Scalar { ty } => domain.contains_type(ty),
            SchemaKind::ScalarUnion { types } => types.iter().all(|ty| domain.contains_type(ty)),
            SchemaKind::Group { .. } => false,
        };
    if contained {
        Ok(())
    } else {
        Err(type_sibling_error(name, nullable))
    }
}

fn ensure_annotation_or_format_only(
    union_name: &str,
    schema: &serde_json::Value,
    shape_keyword: &str,
    allow_type: bool,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "scalar alternatives must be schema objects",
        ));
    };
    if let Some(keyword) = object.keys().find(|keyword| {
        keyword.as_str() != shape_keyword
            && !(allow_type && keyword.as_str() == "type")
            && keyword.as_str() != "const"
            && keyword.as_str() != "enum"
            && keyword.as_str() != "format"
            && keyword.as_str() != "minLength"
            && keyword.as_str() != "maxLength"
            && keyword.as_str() != "pattern"
            && keyword.as_str() != "minimum"
            && keyword.as_str() != "maximum"
            && keyword.as_str() != "exclusiveMinimum"
            && keyword.as_str() != "exclusiveMaximum"
            && keyword.as_str() != "multipleOf"
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
    ensure_annotation_or_range_only_with_type(union_name, schema, shape_keyword, false)
}

fn ensure_annotation_or_range_only_with_type(
    union_name: &str,
    schema: &serde_json::Value,
    shape_keyword: &str,
    allow_type: bool,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Err(unsupported_union(
            union_name,
            "nullable scalar alternatives must be schema objects",
        ));
    };
    if let Some(keyword) = object.keys().find(|keyword| {
        keyword.as_str() != shape_keyword
            && !(allow_type && keyword.as_str() == "type")
            && !is_annotation_keyword(keyword.as_str())
            && !matches!(
                keyword.as_str(),
                "format"
                    | "minimum"
                    | "maximum"
                    | "exclusiveMinimum"
                    | "exclusiveMaximum"
                    | "multipleOf"
                    | "minItems"
                    | "maxItems"
                    | "contains"
                    | "minContains"
                    | "maxContains"
                    | "minProperties"
                    | "maxProperties"
                    | "dependencies"
                    | "dependentRequired"
                    | "dependentSchemas"
                    | "uniqueItems"
                    | "minLength"
                    | "maxLength"
                    | "pattern"
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
