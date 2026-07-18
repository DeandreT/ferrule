use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode,
};

use super::{parse, parse_properties, resolve_ref, unsupported_union};
use crate::JsonFormatError;

pub(super) fn parse_inferred_const_scalar(
    name: &str,
    value: &serde_json::Value,
) -> Result<SchemaNode, JsonFormatError> {
    let ty = match value {
        serde_json::Value::String(_) => ScalarType::String,
        serde_json::Value::Bool(_) => ScalarType::Bool,
        serde_json::Value::Number(number) if number.as_i64().is_some() => ScalarType::Int,
        serde_json::Value::Number(number) if number.as_u64().is_some() => {
            return Err(unsupported_union(
                name,
                "integer const is outside ferrule's signed 64-bit range",
            ));
        }
        serde_json::Value::Number(number) if finite_f64(number).is_some() => ScalarType::Float,
        serde_json::Value::Number(_) => {
            return Err(unsupported_union(
                name,
                "numeric const cannot be represented as a finite ferrule number",
            ));
        }
        serde_json::Value::Null => {
            return Err(unsupported_union(
                name,
                "null const cannot distinguish required fields because JSON null and absence share one IR value",
            ));
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            return Err(unsupported_union(
                name,
                "const discriminators must be JSON scalar values",
            ));
        }
    };
    Ok(SchemaNode::scalar(name, ty))
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
    let base_children = parse_properties(schema, doc, active_refs)?;
    let base_required = required_names(schema);
    let mut merged = base_children.clone();
    let mut metadata = Vec::with_capacity(alternatives.len());
    for (index, alternative_schema) in alternatives.iter().enumerate() {
        let resolved = alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .and_then(|reference| resolve_ref(doc, reference))
            .unwrap_or(alternative_schema);
        let alternative_name = resolved
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                alternative_schema
                    .get("$ref")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|reference| reference.rsplit('/').next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("{keyword}{index}"));
        let parsed = parse(&alternative_name, alternative_schema, doc, active_refs)?;
        if parsed.repeating {
            return Err(unsupported_union(
                name,
                "array alternatives are not supported",
            ));
        }
        let SchemaKind::Group {
            children: variant_children,
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
            return Err(unsupported_union(
                name,
                "nested oneOf or anyOf object alternatives are not supported",
            ));
        }
        if resolved.get("additionalProperties") != Some(&serde_json::Value::Bool(false)) {
            return Err(unsupported_union(
                name,
                "object alternatives must declare additionalProperties false",
            ));
        }
        let mut members: Vec<String> = base_children
            .iter()
            .map(|child| child.name.clone())
            .collect();
        for child in variant_children {
            if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
                if existing != &child {
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
        let mut required = base_required.clone();
        for field in required_names(resolved) {
            if !required.contains(&field) {
                required.push(field);
            }
        }
        let constraints = required_scalar_constraints(name, resolved, &required, &merged)?;
        if required
            .iter()
            .any(|field| !members.iter().any(|member| member == field))
        {
            return Err(unsupported_union(
                name,
                &format!("{keyword} requires a field not declared by that object alternative"),
            ));
        }
        if mode == GroupAlternativeMode::Exclusive
            && metadata.iter().any(|previous: &GroupAlternative| {
                previous.members == members
                    && previous.required == required
                    && previous.constraints == constraints
            })
        {
            return Err(unsupported_union(
                name,
                "alternatives are not distinguishable by supported object fields and requirements",
            ));
        }
        if metadata
            .iter()
            .any(|previous: &GroupAlternative| previous.name == alternative_name)
        {
            return Err(unsupported_union(
                name,
                &format!("{keyword} alternatives must have distinct names"),
            ));
        }
        metadata.push(GroupAlternative {
            name: alternative_name,
            members,
            required,
            constraints,
        });
    }
    let group = SchemaNode::group(name, merged);
    match mode {
        GroupAlternativeMode::Exclusive => group.with_alternatives(metadata),
        GroupAlternativeMode::Inclusive => group.with_inclusive_alternatives(metadata),
    }
    .ok_or_else(|| unsupported_union(name, "alternative metadata is internally inconsistent"))
}

fn required_scalar_constraints(
    union_name: &str,
    schema: &serde_json::Value,
    required: &[String],
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
        .filter_map(|(member, property)| property.get("const").map(|value| (member, value)))
        .map(|(member, value)| {
            if !required.iter().any(|required| required == member) {
                return Err(unsupported_union(
                    union_name,
                    &format!("const discriminator `{member}` must be required"),
                ));
            }
            let child = children
                .iter()
                .find(|child| child.name == *member)
                .ok_or_else(|| {
                    unsupported_union(
                        union_name,
                        &format!("const discriminator `{member}` has no declared scalar field"),
                    )
                })?;
            if child.repeating {
                return Err(unsupported_union(
                    union_name,
                    &format!("const discriminator `{member}` cannot be an array"),
                ));
            }
            let SchemaKind::Scalar { ty } = child.kind else {
                return Err(unsupported_union(
                    union_name,
                    &format!("const discriminator `{member}` must be a scalar field"),
                ));
            };
            let value = constraint_value(union_name, member, value, ty)?;
            Ok(GroupAlternativeConstraint {
                member: member.clone(),
                value,
            })
        })
        .collect()
}

fn constraint_value(
    union_name: &str,
    member: &str,
    value: &serde_json::Value,
    ty: ScalarType,
) -> Result<GroupAlternativeConstraintValue, JsonFormatError> {
    let unsupported = |reason: &str| {
        unsupported_union(
            union_name,
            &format!("const discriminator `{member}` {reason}"),
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
        (_, serde_json::Value::Null) => Err(unsupported(
            "cannot be null because JSON null and absence share one IR value",
        )),
        _ => Err(unsupported("does not match its declared scalar type")),
    }
}

fn finite_f64(number: &serde_json::Number) -> Option<f64> {
    const MAX_EXACT_F64_INTEGER: u64 = 1_u64 << f64::MANTISSA_DIGITS;
    if number
        .as_i64()
        .is_some_and(|value| value.unsigned_abs() > MAX_EXACT_F64_INTEGER)
        || number
            .as_u64()
            .is_some_and(|value| value > MAX_EXACT_F64_INTEGER)
    {
        return None;
    }
    number.as_f64().filter(|value| value.is_finite())
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
