//! A deliberately small JSON Schema importer: enough to turn the common
//! `type: object/array/scalar` shapes into a [`SchemaNode`] tree. It reads
//! `properties` (in document order) and `items`, maps `integer`/`number`/
//! `boolean` to the corresponding scalar types, and resolves document-local
//! `$ref` pointers (`#/definitions/...`, `#/$defs/...`; cyclic or external
//! refs degrade to string scalars). Compatible closed-object `oneOf` and
//! `anyOf` unions, their required scalar `const` discriminators, and typed
//! `additionalProperties` schemas are preserved. Scalar/container-plus-null
//! `oneOf` / `anyOf` and nullable type arrays retain explicit nullability,
//! including scalar array items. Unconstrained `additionalProperties` values
//! are retained as canonical JSON text in the graph's string domain. An
//! omitted or false `additionalProperties` is treated as closed. General
//! composition remains outside this subset; shape-neutral validation keywords
//! are accepted but are not enforced by the mapping schema.

use ir::{GroupAlternativeMode, ScalarType, SchemaNode};

use crate::JsonFormatError;

mod alternatives;
mod render;

use alternatives::{
    parse_inferred_const_scalar, parse_nullable_container_alternatives,
    parse_nullable_scalar_alternatives, parse_object_alternatives,
};

/// Imports the root of a JSON Schema file as a [`SchemaNode`]. The root
/// node is named by the schema's `title` (looked up through a root-level
/// `$ref` too), falling back to `"root"`.
pub fn import(path: &std::path::Path) -> Result<SchemaNode, JsonFormatError> {
    let text = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let name = value
        .get("title")
        .and_then(|t| t.as_str())
        .or_else(|| {
            value
                .get("$ref")
                .and_then(|r| r.as_str())
                .and_then(|r| resolve_ref(&value, r))
                .and_then(|resolved| resolved.get("title"))
                .and_then(|t| t.as_str())
        })
        .unwrap_or("root");
    parse(name, &value, &value, &mut Vec::new())
}

/// Resolves a document-local JSON pointer ref (`#/definitions/office`).
fn resolve_ref<'a>(doc: &'a serde_json::Value, r: &str) -> Option<&'a serde_json::Value> {
    let pointer = r.strip_prefix('#')?;
    doc.pointer(pointer)
}

fn parse(
    name: &str,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
        // Cyclic and external (non-`#/...`) refs degrade to string scalars.
        if active_refs.iter().any(|a| a == r) {
            return Ok(SchemaNode::scalar(name, ScalarType::String));
        }
        let Some(resolved) = resolve_ref(doc, r) else {
            return Ok(SchemaNode::scalar(name, ScalarType::String));
        };
        active_refs.push(r.to_string());
        let node = parse(name, resolved, doc, active_refs);
        active_refs.pop();
        return node;
    }
    if let Some(alternatives) = schema.get("oneOf") {
        if let Some(nullable) = parse_nullable_scalar_alternatives(
            name,
            schema,
            alternatives,
            "oneOf",
            doc,
            active_refs,
        )? {
            return Ok(nullable);
        }
        if let Some(nullable) = parse_nullable_container_alternatives(
            name,
            schema,
            alternatives,
            "oneOf",
            doc,
            active_refs,
        )? {
            return Ok(nullable);
        }
        return parse_object_alternatives(
            name,
            schema,
            alternatives,
            GroupAlternativeMode::Exclusive,
            doc,
            active_refs,
        );
    }
    if let Some(alternatives) = schema.get("anyOf") {
        if let Some(nullable) = parse_nullable_scalar_alternatives(
            name,
            schema,
            alternatives,
            "anyOf",
            doc,
            active_refs,
        )? {
            return Ok(nullable);
        }
        if let Some(nullable) = parse_nullable_container_alternatives(
            name,
            schema,
            alternatives,
            "anyOf",
            doc,
            active_refs,
        )? {
            return Ok(nullable);
        }
        return parse_object_alternatives(
            name,
            schema,
            alternatives,
            GroupAlternativeMode::Inclusive,
            doc,
            active_refs,
        );
    }
    let (ty, nullable) = schema_type(name, schema)?;
    match ty {
        Some("object") => {
            let children = parse_properties(schema, doc, active_refs)?;
            let mut node =
                attach_dynamic_fields(SchemaNode::group(name, children), schema, doc, active_refs)?;
            node.container_nullable = nullable;
            Ok(node)
        }
        Some("array") => {
            let Some(items) = schema.get("items") else {
                let mut node = SchemaNode::scalar(name, ScalarType::String).repeating();
                node.container_nullable = nullable;
                return Ok(node);
            };
            let mut node = parse(name, items, doc, active_refs)?.repeating();
            node.container_nullable = nullable;
            Ok(node)
        }
        Some("string") => Ok(scalar_schema(name, ScalarType::String, nullable)),
        Some("integer") => Ok(scalar_schema(name, ScalarType::Int, nullable)),
        Some("number") => Ok(scalar_schema(name, ScalarType::Float, nullable)),
        Some("boolean") => Ok(scalar_schema(name, ScalarType::Bool, nullable)),
        Some("null") => Err(unsupported_union(
            name,
            "a null-only schema has no distinct ferrule scalar value type",
        )),
        None if schema.get("const").is_some() => {
            parse_inferred_const_scalar(name, &schema["const"])
        }
        _ if schema.get("properties").is_some() => {
            let children = parse_properties(schema, doc, active_refs)?;
            attach_dynamic_fields(SchemaNode::group(name, children), schema, doc, active_refs)
        }
        _ => Ok(SchemaNode::scalar(name, ScalarType::String)),
    }
}

fn schema_type<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<(Option<&'a str>, bool), JsonFormatError> {
    let Some(value) = schema.get("type") else {
        return Ok((None, false));
    };
    let serde_json::Value::Array(types) = value else {
        return Ok((value.as_str(), false));
    };
    let mut concrete = None;
    let mut nullable = false;
    for ty in types {
        let Some(ty) = ty.as_str() else {
            return Err(unsupported_union(
                name,
                "type arrays may contain only string type names",
            ));
        };
        if ty == "null" {
            if nullable {
                return Err(unsupported_union(
                    name,
                    "type arrays may not repeat the null type",
                ));
            }
            nullable = true;
        } else if concrete.replace(ty).is_some() {
            return Err(unsupported_union(
                name,
                "type arrays may contain only one non-null type",
            ));
        }
    }
    let Some(concrete) = concrete else {
        return Err(unsupported_union(
            name,
            "type arrays must contain one non-null type",
        ));
    };
    if nullable
        && !matches!(
            concrete,
            "string" | "integer" | "number" | "boolean" | "object" | "array"
        )
    {
        return Err(unsupported_union(
            name,
            "nullable type arrays require a supported scalar type",
        ));
    }
    Ok((Some(concrete), nullable))
}

fn scalar_schema(name: &str, ty: ScalarType, nullable: bool) -> SchemaNode {
    let mut node = SchemaNode::scalar(name, ty);
    node.nullable = nullable;
    node
}

fn attach_dynamic_fields(
    group: SchemaNode,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let additional = match schema.get("additionalProperties") {
        None | Some(serde_json::Value::Bool(false)) => return Ok(group),
        Some(serde_json::Value::Bool(true)) => return attach_unconstrained_dynamic(group),
        Some(serde_json::Value::Object(object))
            if object.is_empty() || !declares_supported_shape(object) =>
        {
            return attach_unconstrained_dynamic(group);
        }
        Some(additional @ serde_json::Value::Object(_)) => additional,
        Some(_) => {
            return Err(unsupported_object(
                &group.name,
                "additionalProperties must be false or a typed schema",
            ));
        }
    };
    let value = parse("*", additional, doc, active_refs)?;
    let name = group.name.clone();
    group
        .with_dynamic_fields(value)
        .ok_or_else(|| JsonFormatError::UnsupportedSchemaUnion {
            name,
            reason: "open objects cannot use closed object alternatives".to_string(),
        })
}

fn attach_unconstrained_dynamic(group: SchemaNode) -> Result<SchemaNode, JsonFormatError> {
    let value = SchemaNode::scalar("*", ScalarType::String)
        .json_any()
        .ok_or_else(|| unsupported_object(&group.name, "invalid arbitrary JSON value schema"))?;
    let name = group.name.clone();
    group
        .with_dynamic_fields(value)
        .ok_or_else(|| JsonFormatError::UnsupportedSchemaUnion {
            name,
            reason: "open objects cannot use closed object alternatives".to_string(),
        })
}

fn declares_supported_shape(schema: &serde_json::Map<String, serde_json::Value>) -> bool {
    schema.contains_key("$ref")
        || schema.contains_key("oneOf")
        || schema.contains_key("anyOf")
        || schema.contains_key("properties")
        || schema.get("type").is_some_and(|value| match value {
            serde_json::Value::String(ty) => matches!(
                ty.as_str(),
                "object" | "array" | "string" | "integer" | "number" | "boolean"
            ),
            serde_json::Value::Array(types) => types.iter().any(|ty| {
                ty.as_str().is_some_and(|ty| {
                    matches!(
                        ty,
                        "object" | "array" | "string" | "integer" | "number" | "boolean"
                    )
                })
            }),
            _ => false,
        })
}

fn parse_properties(
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
                .map(|(child_name, child_schema)| parse(child_name, child_schema, doc, active_refs))
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn unsupported_union(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

fn unsupported_object(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

/// Renders a [`SchemaNode`] as JSON Schema text -- the inverse of
/// [`import`], producing the same `type: object/array/scalar` subset it
/// reads (repeating nodes become `type: array` wrappers). The root gets a
/// `title` so the name survives a roundtrip.
pub fn export(schema: &SchemaNode) -> String {
    let mut root = serde_json::Map::new();
    root.insert("title".into(), schema.name.clone().into());
    render::render(schema, &mut root);
    serde_json::to_string_pretty(&serde_json::Value::Object(root)).expect("schema is valid JSON")
}

#[cfg(test)]
mod tests;
