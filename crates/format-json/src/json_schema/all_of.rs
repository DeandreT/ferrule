use ir::{SchemaKind, SchemaNode};

use super::{attach_dynamic_fields, parse, parse_properties, unsupported_union};
use crate::JsonFormatError;

/// Flattens object intersections into one ordinary object projection.
///
/// Ferrule's JSON schema is structural rather than validating, so required and
/// shape-neutral validation keywords retain the same behavior as an ordinary
/// object import. Conflicting property shapes and alternative-bearing branches
/// reject instead of widening the intersection.
pub(super) fn parse_object_all_of(
    name: &str,
    schema: &serde_json::Value,
    composition: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let branches = composition
        .as_array()
        .filter(|branches| !branches.is_empty())
        .ok_or_else(|| unsupported_union(name, "allOf must contain at least one object schema"))?;
    if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
        return Err(unsupported_union(
            name,
            "allOf cannot be combined with oneOf or anyOf at the same schema level",
        ));
    }
    if let Some(ty) = schema.get("type")
        && ty != "object"
    {
        return Err(unsupported_union(
            name,
            "object allOf may declare only type=\"object\" at the composition level",
        ));
    }

    let mut merged = if schema.get("type").is_some()
        || schema.get("properties").is_some()
        || schema.get("additionalProperties").is_some()
    {
        let children = parse_properties(schema, doc, active_refs)?;
        Some(attach_dynamic_fields(
            SchemaNode::group(name, children),
            schema,
            doc,
            active_refs,
        )?)
    } else {
        None
    };

    for branch in branches {
        if is_unconstrained_branch(branch) {
            continue;
        }
        let branch = parse(name, branch, doc, active_refs)?;
        ensure_plain_object(name, &branch)?;
        match &mut merged {
            Some(current) => merge_object(name, current, branch)?,
            None => merged = Some(branch),
        }
    }
    merged.ok_or_else(|| unsupported_union(name, "allOf did not produce an object schema"))
}

fn ensure_plain_object(name: &str, node: &SchemaNode) -> Result<(), JsonFormatError> {
    if node.repeating || node.nullable || node.container_nullable {
        return Err(unsupported_union(
            name,
            "allOf branches must be non-nullable, non-array object schemas",
        ));
    }
    let SchemaKind::Group {
        alternatives,
        xml_restricted_alternatives,
        ..
    } = &node.kind
    else {
        return Err(unsupported_union(
            name,
            "allOf branches must resolve to object schemas",
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
    ensure_plain_object(name, target)?;
    let SchemaKind::Group {
        children: target_children,
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
        children, dynamic, ..
    } = branch.kind
    else {
        return Err(unsupported_union(
            name,
            "allOf merge branch must be an object schema",
        ));
    };

    for child in children {
        if let Some(existing) = target_children
            .iter()
            .find(|existing| existing.name == child.name)
        {
            if existing != &child {
                return Err(unsupported_union(
                    name,
                    &format!(
                        "allOf property `{}` has incompatible schemas across branches",
                        child.name
                    ),
                ));
            }
        } else {
            target_children.push(child);
        }
    }

    match (target_dynamic.as_ref(), dynamic) {
        // A closed branch makes the intersection closed under Ferrule's
        // existing additionalProperties contract.
        (None, _) | (_, None) => *target_dynamic = None,
        (Some(existing), Some(candidate)) if existing == &candidate => {}
        (Some(_), Some(_)) => {
            return Err(unsupported_union(
                name,
                "allOf branches declare incompatible additionalProperties schemas",
            ));
        }
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
