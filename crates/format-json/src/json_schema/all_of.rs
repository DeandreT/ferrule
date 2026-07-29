use ir::{ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};

use super::{
    allowed_values, conditionals, contains, dependent_schemas, files, formats, item_counts,
    multiples, parse, pattern_properties, patterns, property_counts, property_dependencies,
    property_names, ranges, string_lengths, unique_items, unsupported_union,
};
use crate::JsonFormatError;

enum FormatEvent {
    Retained(ir::JsonFormatAnnotations),
    Direct(String),
}

/// Flattens representable intersections into one structural projection.
///
/// Ferrule's JSON schema is structural rather than validating, so required and
/// shape-neutral validation keywords retain the same behavior as an ordinary
/// import. Conflicting shapes and alternative-bearing branches reject instead
/// of widening the intersection.
pub(super) fn parse_all_of(
    name: &str,
    schema: &serde_json::Value,
    composition: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let branches = composition
        .as_array()
        .filter(|branches| !branches.is_empty())
        .ok_or_else(|| unsupported_union(name, "allOf must contain at least one schema"))?;
    if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
        return Err(unsupported_union(
            name,
            "allOf cannot be combined with oneOf or anyOf at the same schema level",
        ));
    }

    let mut pending_constraints = Vec::new();
    let mut format_events = Vec::new();
    let mut structural = Vec::new();
    if let Some(base) = composition_base(schema) {
        if is_constraint_only_branch(&base) && !allowed_values::has_keyword(&base) {
            collect_direct_format(name, &base, &mut format_events)?;
            pending_constraints.push(base);
        } else {
            let mut base = parse(name, &base, doc, active_refs)?;
            collect_retained_formats(&mut base, &mut format_events);
            structural.push(base);
        }
    }

    for branch in branches {
        if is_unconstrained_branch(branch) {
            continue;
        }
        if branch == &serde_json::Value::Bool(false) {
            return Err(unsupported_union(
                name,
                "allOf contains the always-invalid false schema",
            ));
        }
        if is_constraint_only_branch(branch) && !allowed_values::has_keyword(branch) {
            collect_direct_format(name, branch, &mut format_events)?;
            pending_constraints.push(branch.clone());
            continue;
        }
        let mut branch = parse(name, branch, doc, active_refs)?;
        collect_retained_formats(&mut branch, &mut format_events);
        structural.push(branch);
    }
    for branch in &structural {
        pattern_properties::reject_composed_node(name, branch, "allOf")?;
    }
    let merged = merge_structural_branches(name, structural)?;
    let format_only_fallback =
        merged.is_none() && pending_constraints.iter().any(formats::has_keyword);
    let mut no_op_constraint_fallback = merged.is_none() && !pending_constraints.is_empty();
    for constraints in &pending_constraints {
        no_op_constraint_fallback &= (patterns::has_keyword(constraints)
            || unique_items::has_keyword(constraints)
            || property_counts::has_keywords(constraints)
            || property_dependencies::has_keywords(constraints)
            || dependent_schemas::has_keywords(constraints)
            || conditionals::has_keywords(constraints)
            || property_names::has_keyword(constraints)
            || contains::has_keyword(constraints))
            && (!patterns::has_keyword(constraints)
                || !patterns::is_effectively_constrained(name, constraints)?)
            && (!unique_items::has_keyword(constraints)
                || !unique_items::selected(name, constraints)?)
            && (!property_counts::has_keywords(constraints)
                || !property_counts::is_effectively_constrained(name, constraints)?)
            && (!property_dependencies::has_keywords(constraints)
                || !property_dependencies::is_effectively_constrained(name, constraints)?)
            && (!dependent_schemas::has_keywords(constraints)
                || !dependent_schemas::is_effectively_constrained(
                    name,
                    constraints,
                    doc,
                    active_refs,
                )?)
            && (!conditionals::has_keywords(constraints)
                || !conditionals::is_effectively_constrained(name, constraints)?)
            && (!property_names::has_keyword(constraints)
                || property_names::selected(name, constraints, doc, active_refs)?.is_none())
            && (!contains::has_keyword(constraints)
                || !contains::is_effectively_constrained(name, constraints, doc, active_refs)?)
            && !ranges::has_range_keywords(constraints)
            && !allowed_values::has_keyword(constraints)
            && !multiples::has_keyword(constraints)
            && !item_counts::has_keywords(constraints)
            && !string_lengths::is_effectively_constrained(name, constraints)?
            && !formats::has_keyword(constraints);
    }
    if format_only_fallback {
        for constraints in &pending_constraints {
            if string_lengths::is_effectively_constrained(name, constraints)? {
                return Err(unsupported_union(
                    name,
                    "string-length constraints without a concrete string-capable type also admit unconstrained non-string values",
                ));
            }
            if patterns::is_effectively_constrained(name, constraints)? {
                return Err(unsupported_union(
                    name,
                    "pattern constraints without a concrete string-capable type also admit unconstrained non-string values",
                ));
            }
            if property_counts::is_effectively_constrained(name, constraints)? {
                return Err(unsupported_union(
                    name,
                    "property-count constraints without a concrete object type also admit unconstrained non-object values",
                ));
            }
            if property_dependencies::is_effectively_constrained(name, constraints)? {
                return Err(unsupported_union(
                    name,
                    "property dependencies without a concrete object type also admit unconstrained non-object values",
                ));
            }
            if dependent_schemas::is_effectively_constrained(name, constraints, doc, active_refs)? {
                return Err(unsupported_union(
                    name,
                    "dependent schemas without a concrete object type also admit unconstrained non-object values",
                ));
            }
            if conditionals::is_effectively_constrained(name, constraints)? {
                return Err(unsupported_union(
                    name,
                    "presence-based conditionals without a concrete object type also admit unconstrained non-object values",
                ));
            }
            if property_names::selected(name, constraints, doc, active_refs)?.is_some() {
                return Err(unsupported_union(
                    name,
                    "propertyNames without a concrete object type also admits unconstrained non-object values",
                ));
            }
            if contains::is_effectively_constrained(name, constraints, doc, active_refs)? {
                return Err(unsupported_union(
                    name,
                    "contains without a concrete array type also admits unconstrained non-array values",
                ));
            }
        }
    }
    let mut merged = match merged {
        Some(merged) => merged,
        None if format_only_fallback => SchemaNode::scalar(name, ScalarType::String),
        None if no_op_constraint_fallback => super::arbitrary_json_schema(name)?,
        None => {
            return Err(unsupported_union(
                name,
                "allOf did not produce a structural schema",
            ));
        }
    };
    for constraints in pending_constraints {
        allowed_values::apply(name, &constraints, &mut merged)?;
        property_counts::apply(name, &constraints, &mut merged, false)?;
        property_dependencies::apply(name, &constraints, &mut merged, false)?;
        dependent_schemas::apply(name, &constraints, &mut merged, doc, active_refs, false)?;
        conditionals::apply(name, &constraints, &mut merged, doc, active_refs, false)?;
        property_names::apply(name, &constraints, &mut merged, doc, active_refs, false)?;
        if merged.repeating {
            ranges::validate_ignored(name, &constraints)?;
            multiples::validate_ignored(name, &constraints)?;
            item_counts::apply(name, &constraints, &mut merged, false)?;
            contains::apply(name, &constraints, &mut merged, doc, active_refs, false)?;
            unique_items::apply(name, &constraints, &mut merged, false)?;
            string_lengths::validate_ignored(name, &constraints)?;
            patterns::validate_ignored(name, &constraints)?;
        } else {
            ranges::apply(name, &constraints, &mut merged, false)?;
            multiples::apply(name, &constraints, &mut merged, false)?;
            item_counts::validate_ignored(name, &constraints)?;
            unique_items::validate_ignored(name, &constraints)?;
            string_lengths::apply(name, &constraints, &mut merged, false)?;
            patterns::apply(name, &constraints, &mut merged, false)?;
        }
    }
    apply_format_events(name, &mut merged, format_events)?;
    pattern_properties::reject_composed_node(name, &merged, "allOf")?;
    Ok(merged)
}

fn merge_structural_branches(
    name: &str,
    mut branches: Vec<SchemaNode>,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    if branches.is_empty() {
        return Ok(None);
    }
    let closing_pair = branches.iter().enumerate().find_map(|(left_index, left)| {
        let left = left.dynamic_fields()?;
        branches[left_index + 1..]
            .iter()
            .position(|right| {
                right
                    .dynamic_fields()
                    .is_some_and(|right| schemas_are_provably_disjoint(left, right))
            })
            .map(|offset| (left_index, left_index + 1 + offset))
    });
    let closed_pivot = branches.iter().position(is_closed_object);
    let selected_pair = closed_pivot.is_none().then_some(closing_pair).flatten();
    let pivot = closed_pivot.unwrap_or_else(|| {
        selected_pair
            .map(|(left_index, _)| left_index)
            .unwrap_or_default()
    });
    let mut merged = branches.remove(pivot);
    if let Some((left_index, right_index)) = selected_pair {
        let other_index = if pivot == left_index {
            right_index - 1
        } else {
            left_index
        };
        let branch = branches.remove(other_index);
        intersect(name, &mut merged, branch)?;
    }
    for branch in branches {
        intersect(name, &mut merged, branch)?;
    }
    Ok(Some(merged))
}

fn is_closed_object(node: &SchemaNode) -> bool {
    matches!(&node.kind, SchemaKind::Group { dynamic: None, .. })
}

fn collect_direct_format(
    name: &str,
    schema: &serde_json::Value,
    events: &mut Vec<FormatEvent>,
) -> Result<(), JsonFormatError> {
    if let Some(format) = formats::validate(name, schema)? {
        events.push(FormatEvent::Direct(format.to_string()));
    }
    Ok(())
}

fn collect_retained_formats(node: &mut SchemaNode, events: &mut Vec<FormatEvent>) {
    let formats = core::mem::take(&mut node.json_formats);
    if !formats.is_empty() {
        events.push(FormatEvent::Retained(formats));
    }
}

fn apply_format_events(
    name: &str,
    node: &mut SchemaNode,
    events: Vec<FormatEvent>,
) -> Result<(), JsonFormatError> {
    if node.json_any || !node.accepts_scalar_type(ScalarType::String) {
        return Ok(());
    }
    for event in events {
        match event {
            FormatEvent::Retained(formats) => {
                formats::extend(name, node, formats.into_vec())?;
            }
            FormatEvent::Direct(format) if !node.repeating => {
                formats::extend(name, node, core::iter::once(format))?;
            }
            FormatEvent::Direct(_) => {}
        }
    }
    Ok(())
}

fn composition_base(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let object = schema.as_object()?;
    if !object.keys().any(|key| {
        matches!(
            key.as_str(),
            "type"
                | "properties"
                | "required"
                | "additionalProperties"
                | "const"
                | "enum"
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
                | "if"
                | "then"
                | "else"
                | "propertyNames"
                | "uniqueItems"
                | "minLength"
                | "maxLength"
                | "pattern"
                | "format"
        )
    }) {
        return None;
    }
    let mut base = object.clone();
    base.remove("allOf");
    Some(serde_json::Value::Object(base))
}

fn is_constraint_only_branch(schema: &serde_json::Value) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    (ranges::has_range_keywords(schema)
        || allowed_values::has_keyword(schema)
        || multiples::has_keyword(schema)
        || item_counts::has_keywords(schema)
        || contains::has_keyword(schema)
        || property_counts::has_keywords(schema)
        || property_dependencies::has_keywords(schema)
        || dependent_schemas::has_keywords(schema)
        || conditionals::has_keywords(schema)
        || property_names::has_keyword(schema)
        || unique_items::has_keyword(schema)
        || string_lengths::has_keywords(schema)
        || patterns::has_keyword(schema)
        || formats::has_keyword(schema))
        && object.keys().all(|keyword| {
            files::is_internal_ref_keyword(keyword)
                || matches!(
                    keyword.as_str(),
                    "minimum"
                        | "const"
                        | "enum"
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
                        | "if"
                        | "then"
                        | "else"
                        | "propertyNames"
                        | "uniqueItems"
                        | "minLength"
                        | "maxLength"
                        | "pattern"
                        | "format"
                        | "$schema"
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
        })
}

fn intersect(
    name: &str,
    target: &mut SchemaNode,
    branch: SchemaNode,
) -> Result<(), JsonFormatError> {
    if target.json_any && !target.repeating {
        *target = branch;
        return Ok(());
    }
    if branch.json_any && !branch.repeating {
        return Ok(());
    }
    if target.repeating != branch.repeating {
        return Err(unsupported_union(
            name,
            "allOf branches have incompatible array and non-array shapes",
        ));
    }
    target.nullable &= branch.nullable;
    target.container_nullable &= branch.container_nullable;
    target.item_count_range =
        item_counts::intersect(name, target.item_count_range, branch.item_count_range)?;
    target.json_contains = contains::merge(
        name,
        target.json_contains.take(),
        branch.json_contains.clone(),
    )?;
    target.json_dependent_schemas = dependent_schemas::merge(
        name,
        target.json_dependent_schemas.take(),
        branch.json_dependent_schemas.clone(),
    )?;
    target.property_count_range = property_counts::intersect(
        name,
        target.property_count_range,
        branch.property_count_range,
    )?;
    target.json_property_dependencies = property_dependencies::intersect(
        name,
        target.json_property_dependencies.as_ref(),
        branch.json_property_dependencies.as_ref(),
    )?;
    target.json_property_names = property_names::intersect(
        name,
        target.json_property_names.take(),
        branch.json_property_names.clone(),
    )?;
    target.json_unique_items |= branch.json_unique_items;

    let result = match (&target.kind, &branch.kind) {
        (SchemaKind::Group { .. }, SchemaKind::Group { .. }) => merge_object(name, target, branch),
        (
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
        ) => intersect_scalar(name, target, &branch),
        _ => Err(unsupported_union(
            name,
            "allOf branches have incompatible scalar and object shapes",
        )),
    };
    result?;
    property_counts::ensure_feasible(name, target)?;
    property_dependencies::ensure_feasible(name, target)?;
    if target.json_property_names_are_valid() && target.property_count_range_is_valid() {
        Ok(())
    } else {
        Err(unsupported_union(
            name,
            "allOf propertyNames constraints reject an unconditionally required property or conflict with cardinality",
        ))
    }
}

fn intersect_scalar(
    name: &str,
    target: &mut SchemaNode,
    branch: &SchemaNode,
) -> Result<(), JsonFormatError> {
    let target_allowed = allowed_values::from_schema(target)?;
    let branch_allowed = allowed_values::from_schema(branch)?;
    let target_range = target.numeric_range;
    let branch_range = branch.numeric_range;
    let target_multiples = target.json_multiple_of.clone();
    let branch_multiples = branch.json_multiple_of.clone();
    let target_string_length = target.string_length_range;
    let branch_string_length = branch.string_length_range;
    let target_patterns = target.json_patterns.clone();
    let branch_patterns = branch.json_patterns.clone();
    if target.kind != branch.kind {
        let domain = scalar_domain(target) & scalar_domain(branch);
        let mut types = Vec::new();
        if domain & STRING != 0 {
            types.push(ScalarType::String);
        }
        if domain & INTEGER != 0 && domain & NUMBER == 0 {
            types.push(ScalarType::Int);
        }
        if domain & NUMBER != 0 {
            types.push(ScalarType::Float);
        }
        if domain & BOOLEAN != 0 {
            types.push(ScalarType::Bool);
        }
        target.kind = match types.as_slice() {
            [] => {
                return Err(unsupported_union(
                    name,
                    "allOf scalar branches have no value type in common",
                ));
            }
            [ty] => SchemaKind::Scalar { ty: *ty },
            _ => {
                let Some(types) = ScalarTypeSet::new(types) else {
                    return Err(unsupported_union(
                        name,
                        "allOf scalar intersection produced an invalid type set",
                    ));
                };
                SchemaKind::ScalarUnion { types }
            }
        };
    }
    allowed_values::intersect_nodes(name, target, target_allowed, branch_allowed)?;
    target.numeric_range = match target.kind {
        SchemaKind::Scalar {
            ty: ty @ (ScalarType::Int | ScalarType::Float),
        } => ranges::intersect(name, target_range, branch_range, ty)?,
        _ if target_range.is_none() && branch_range.is_none() => None,
        _ => {
            return Err(unsupported_union(
                name,
                "numeric range is incompatible with the intersected scalar type",
            ));
        }
    };
    if !target.numeric_range_is_valid() {
        return Err(unsupported_union(
            name,
            "allOf fixed numeric value falls outside the intersected range",
        ));
    }
    target.json_multiple_of = if target.accepts_scalar_type(ScalarType::Int)
        || target.accepts_scalar_type(ScalarType::Float)
    {
        multiples::intersect(name, target_multiples, branch_multiples)?
    } else if target_multiples.is_none() && branch_multiples.is_none() {
        None
    } else {
        return Err(unsupported_union(
            name,
            "multipleOf is incompatible with the intersected scalar type",
        ));
    };
    if !target.json_multiple_of_is_valid() {
        return Err(unsupported_union(
            name,
            "allOf fixed numeric value is not exactly divisible by the intersected multipleOf constraints",
        ));
    }
    target.string_length_range = if target.accepts_scalar_type(ScalarType::String) {
        string_lengths::intersect(name, target_string_length, branch_string_length)?
    } else {
        None
    };
    if !target.string_length_range_is_valid() {
        return Err(unsupported_union(
            name,
            "allOf fixed string value falls outside the intersected length range",
        ));
    }
    target.json_patterns = if target.accepts_scalar_type(ScalarType::String) {
        patterns::intersect(name, target_patterns, branch_patterns)?
    } else {
        None
    };
    formats::merge_scalar(name, target, branch)?;
    Ok(())
}

const STRING: u8 = 1 << 0;
const INTEGER: u8 = 1 << 1;
const NUMBER: u8 = 1 << 2;
const BOOLEAN: u8 = 1 << 3;

fn scalar_domain(node: &SchemaNode) -> u8 {
    let mut domain = 0;
    for ty in [
        ScalarType::String,
        ScalarType::Int,
        ScalarType::Float,
        ScalarType::Bool,
    ] {
        if node.accepts_scalar_type(ty) {
            domain |= match ty {
                ScalarType::String => STRING,
                ScalarType::Int => INTEGER,
                ScalarType::Float => INTEGER | NUMBER,
                ScalarType::Bool => BOOLEAN,
            };
        }
    }
    domain
}

fn ensure_plain_group(name: &str, node: &SchemaNode) -> Result<(), JsonFormatError> {
    let SchemaKind::Group {
        alternatives,
        xml_restricted_alternatives,
        ..
    } = &node.kind
    else {
        return Err(unsupported_union(
            name,
            "allOf branch must resolve to an object schema",
        ));
    };
    if !alternatives.is_empty() || !xml_restricted_alternatives.is_empty() {
        return Err(unsupported_union(
            name,
            "allOf branches cannot contain object alternatives",
        ));
    }
    Ok(())
}

fn merge_object(
    name: &str,
    target: &mut SchemaNode,
    branch: SchemaNode,
) -> Result<(), JsonFormatError> {
    ensure_plain_group(name, target)?;
    ensure_plain_group(name, &branch)?;
    let SchemaKind::Group {
        children: target_children,
        required: target_required,
        dynamic: target_dynamic,
        ..
    } = &mut target.kind
    else {
        return Err(unsupported_union(
            name,
            "allOf merge target must be an object schema",
        ));
    };
    let SchemaKind::Group {
        mut children,
        required,
        dynamic,
        ..
    } = branch.kind
    else {
        return Err(unsupported_union(
            name,
            "allOf merge branch must be an object schema",
        ));
    };

    let target_additional = target_dynamic.as_deref().cloned();
    let branch_additional = dynamic.as_deref().cloned();
    let merged_dynamic = match (target_dynamic.take(), dynamic) {
        (None, _) | (Some(_), None) => None,
        (Some(mut existing), Some(candidate)) => {
            let disjoint = schemas_are_provably_disjoint(&existing, &candidate);
            match intersect(name, &mut existing, *candidate) {
                Ok(()) => Some(existing),
                Err(_) if disjoint => None,
                Err(error) => return Err(error),
            }
        }
    };
    let result_is_open = merged_dynamic.is_some();
    let mut merged_children = Vec::with_capacity(target_children.len() + children.len());
    for child in core::mem::take(target_children) {
        let required_property =
            target_required.contains(&child.name) || required.contains(&child.name);
        if let Some(index) = children
            .iter()
            .position(|candidate| candidate.name == child.name)
        {
            let candidate = children.remove(index);
            if let Some(child) =
                intersect_named_property(name, child, candidate, required_property, result_is_open)?
            {
                merged_children.push(child);
            }
        } else if let Some(candidate) = branch_additional.as_ref() {
            let mut candidate = candidate.clone();
            candidate.name = child.name.clone();
            if let Some(child) =
                intersect_named_property(name, child, candidate, required_property, result_is_open)?
            {
                merged_children.push(child);
            }
        } else if required_property {
            return Err(unsupported_union(
                name,
                "allOf requires a property forbidden by a closed object branch",
            ));
        }
    }
    for child in children {
        let required_property =
            target_required.contains(&child.name) || required.contains(&child.name);
        if let Some(candidate) = target_additional.as_ref() {
            let mut candidate = candidate.clone();
            candidate.name = child.name.clone();
            if let Some(child) =
                intersect_named_property(name, child, candidate, required_property, result_is_open)?
            {
                merged_children.push(child);
            }
        } else if required_property {
            return Err(unsupported_union(
                name,
                "allOf requires a property forbidden by a closed object branch",
            ));
        }
    }
    *target_children = merged_children;
    *target_dynamic = merged_dynamic;

    for field in required {
        if !target_required.contains(&field) {
            target_required.push(field);
        }
    }

    if !target.required_fields_are_valid() {
        return Err(unsupported_union(
            name,
            "allOf requires an undeclared property in a closed object intersection",
        ));
    }
    Ok(())
}

fn intersect_named_property(
    object_name: &str,
    mut property: SchemaNode,
    candidate: SchemaNode,
    required: bool,
    result_is_open: bool,
) -> Result<Option<SchemaNode>, JsonFormatError> {
    let disjoint = schemas_are_provably_disjoint(&property, &candidate);
    match intersect(object_name, &mut property, candidate) {
        Ok(()) => Ok(Some(property)),
        Err(_) if disjoint && !required && !result_is_open => Ok(None),
        Err(error) => Err(error),
    }
}

fn schemas_are_provably_disjoint(left: &SchemaNode, right: &SchemaNode) -> bool {
    if (left.nullable || left.container_nullable) && (right.nullable || right.container_nullable) {
        return false;
    }
    if left.json_any || right.json_any {
        return false;
    }
    if left.repeating != right.repeating {
        return true;
    }
    match (&left.kind, &right.kind) {
        (
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
        ) => scalar_domain(left) & scalar_domain(right) == 0,
        (SchemaKind::Group { .. }, SchemaKind::Group { .. }) => false,
        _ => true,
    }
}

fn is_unconstrained_branch(schema: &serde_json::Value) -> bool {
    if schema == &serde_json::Value::Bool(true) {
        return true;
    }
    schema.as_object().is_some_and(|object| {
        object.keys().all(|keyword| {
            matches!(
                keyword.as_str(),
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
        })
    })
}
