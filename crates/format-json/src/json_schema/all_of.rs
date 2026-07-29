use ir::{ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};

use super::{constraints, item_counts, parse, ranges, unsupported_union};
use crate::JsonFormatError;

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
    let mut merged = None;
    if let Some(base) = composition_base(schema) {
        if is_constraint_only_branch(&base) {
            pending_constraints.push(base);
        } else {
            merged = Some(parse(name, &base, doc, active_refs)?);
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
        if is_constraint_only_branch(branch) {
            pending_constraints.push(branch.clone());
            continue;
        }
        let branch = parse(name, branch, doc, active_refs)?;
        match &mut merged {
            Some(current) => intersect(name, current, branch)?,
            None => merged = Some(branch),
        }
    }
    let mut merged = merged
        .ok_or_else(|| unsupported_union(name, "allOf did not produce a structural schema"))?;
    for constraints in pending_constraints {
        if merged.repeating {
            ranges::validate_ignored(name, &constraints)?;
            item_counts::apply(name, &constraints, &mut merged, false)?;
        } else {
            ranges::apply(name, &constraints, &mut merged, false)?;
            item_counts::validate_ignored(name, &constraints)?;
        }
    }
    Ok(merged)
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
                | "minItems"
                | "maxItems"
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
    (ranges::has_range_keywords(schema) || item_counts::has_keywords(schema))
        && object.keys().all(|keyword| {
            matches!(
                keyword.as_str(),
                "minimum"
                    | "maximum"
                    | "exclusiveMinimum"
                    | "exclusiveMaximum"
                    | "minItems"
                    | "maxItems"
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

    match (&target.kind, &branch.kind) {
        (SchemaKind::Group { .. }, SchemaKind::Group { .. }) => merge_object(name, target, branch),
        (
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. },
        ) => intersect_scalar(name, target, &branch),
        _ => Err(unsupported_union(
            name,
            "allOf branches have incompatible scalar and object shapes",
        )),
    }
}

fn intersect_scalar(
    name: &str,
    target: &mut SchemaNode,
    branch: &SchemaNode,
) -> Result<(), JsonFormatError> {
    let target_constant = constraints::from_schema(target)?;
    let branch_constant = constraints::from_schema(branch)?;
    let target_range = target.numeric_range;
    let branch_range = branch.numeric_range;
    if let (Some(left), Some(right)) = (&target_constant, &branch_constant)
        && !left.semantically_equals(right)
    {
        return Err(unsupported_union(
            name,
            "allOf fixed scalar constraints have no value in common",
        ));
    }
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
    let constant = target_constant.or(branch_constant);
    if let Some(constant) = constant {
        let value = constant.to_json();
        let constant = match target.kind {
            SchemaKind::Scalar { ty } => constraints::for_type(name, &value, ty)?,
            SchemaKind::ScalarUnion { types } => constraints::for_types(name, &value, types)?,
            SchemaKind::Group { .. } => {
                return Err(unsupported_union(
                    name,
                    "allOf scalar intersection produced an object",
                ));
            }
        };
        let constrained = constraints::schema(&target.name, &constant);
        target.kind = constrained.kind;
        target.fixed = constrained.fixed;
        target.nullable = false;
    } else {
        target.fixed = None;
    }
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
        children,
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

    for child in children {
        if let Some(existing) = target_children
            .iter_mut()
            .find(|existing| existing.name == child.name)
        {
            intersect(name, existing, child)?;
        } else {
            target_children.push(child);
        }
    }
    for field in required {
        if !target_required.contains(&field) {
            target_required.push(field);
        }
    }

    match (target_dynamic.as_mut(), dynamic) {
        // A closed branch makes the intersection closed under Ferrule's
        // existing additionalProperties contract.
        (None, _) | (Some(_), None) => *target_dynamic = None,
        (Some(existing), Some(candidate)) => intersect(name, existing, *candidate)?,
    }
    if !target.required_fields_are_valid() {
        return Err(unsupported_union(
            name,
            "allOf requires an undeclared property in a closed object intersection",
        ));
    }
    Ok(())
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
