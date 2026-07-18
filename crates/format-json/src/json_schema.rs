//! A deliberately small JSON Schema importer: enough to turn the common
//! `type: object/array/scalar` shapes into a [`SchemaNode`] tree. It reads
//! `properties` (in document order) and `items`, maps `integer`/`number`/
//! `boolean` to the corresponding scalar types, and resolves document-local
//! `$ref` pointers (`#/definitions/...`, `#/$defs/...`; cyclic or external
//! refs degrade to string scalars). Compatible closed-object `oneOf` and
//! `anyOf` unions, their required scalar `const` discriminators, and typed
//! `additionalProperties` schemas are preserved. An omitted or false
//! `additionalProperties` is treated as closed; explicitly unconstrained
//! `true`/`{}` schemas and general composition or validation keywords remain
//! outside this "lite" subset.

use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode,
};

use crate::JsonFormatError;

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
        return parse_object_alternatives(
            name,
            schema,
            alternatives,
            GroupAlternativeMode::Inclusive,
            doc,
            active_refs,
        );
    }
    let ty = schema_type(name, schema)?;
    match ty {
        Some("object") => {
            let children = parse_properties(schema, doc, active_refs)?;
            attach_dynamic_fields(SchemaNode::group(name, children), schema, doc, active_refs)
        }
        Some("array") => {
            let Some(items) = schema.get("items") else {
                return Ok(SchemaNode::scalar(name, ScalarType::String).repeating());
            };
            Ok(parse(name, items, doc, active_refs)?.repeating())
        }
        Some("integer") => Ok(SchemaNode::scalar(name, ScalarType::Int)),
        Some("number") => Ok(SchemaNode::scalar(name, ScalarType::Float)),
        Some("boolean") => Ok(SchemaNode::scalar(name, ScalarType::Bool)),
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

fn parse_inferred_const_scalar(
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

fn schema_type<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<&'a str>, JsonFormatError> {
    let Some(value) = schema.get("type") else {
        return Ok(None);
    };
    let serde_json::Value::Array(types) = value else {
        return Ok(value.as_str());
    };
    let mut concrete = types
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|ty| *ty != "null");
    let first = concrete.next();
    if concrete.next().is_some() {
        return Err(unsupported_union(
            name,
            "type arrays may contain only one non-null type",
        ));
    }
    Ok(first)
}

fn attach_dynamic_fields(
    group: SchemaNode,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let additional = match schema.get("additionalProperties") {
        None | Some(serde_json::Value::Bool(false)) => return Ok(group),
        Some(serde_json::Value::Bool(true)) => {
            return Err(unsupported_object(
                &group.name,
                "unconstrained additionalProperties true has no exact ferrule value schema",
            ));
        }
        Some(serde_json::Value::Object(object))
            if object.is_empty() || !declares_supported_shape(object) =>
        {
            return Err(unsupported_object(
                &group.name,
                "unconstrained additionalProperties schema has no exact ferrule value type",
            ));
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

fn parse_object_alternatives(
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
    render(schema, &mut root);
    serde_json::to_string_pretty(&serde_json::Value::Object(root)).expect("schema is valid JSON")
}

/// Writes `node`'s shape (sans repetition) into `out`; repetition wraps it
/// in an array schema.
fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    if node.repeating {
        out.insert("type".into(), "array".into());
        let mut items = serde_json::Map::new();
        render_shape(node, &mut items);
        out.insert("items".into(), serde_json::Value::Object(items));
    } else {
        render_shape(node, out);
    }
}

fn render_shape(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            let name = match ty {
                ScalarType::String => "string",
                ScalarType::Int => "integer",
                ScalarType::Float => "number",
                ScalarType::Bool => "boolean",
            };
            out.insert("type".into(), name.into());
        }
        SchemaKind::Group {
            children,
            alternatives,
            dynamic,
        } => {
            out.insert("type".into(), "object".into());
            if !alternatives.is_empty() {
                let variants = alternatives
                    .iter()
                    .map(|alternative| {
                        let mut variant = serde_json::Map::new();
                        variant.insert("title".into(), alternative.name.clone().into());
                        variant.insert("type".into(), "object".into());
                        variant.insert("additionalProperties".into(), false.into());
                        let mut properties = serde_json::Map::new();
                        for member in &alternative.members {
                            if let Some(child) = children.iter().find(|child| child.name == *member)
                            {
                                let mut property = serde_json::Map::new();
                                render(child, &mut property);
                                if let Some(constraint) = alternative
                                    .constraints
                                    .iter()
                                    .find(|constraint| constraint.member == *member)
                                {
                                    property.insert(
                                        "const".into(),
                                        constraint_value_to_json(&constraint.value),
                                    );
                                }
                                properties.insert(
                                    child.name.clone(),
                                    serde_json::Value::Object(property),
                                );
                            }
                        }
                        variant.insert("properties".into(), properties.into());
                        if !alternative.required.is_empty() {
                            variant.insert("required".into(), alternative.required.clone().into());
                        }
                        serde_json::Value::Object(variant)
                    })
                    .collect();
                let keyword = match node.alternative_mode() {
                    GroupAlternativeMode::Exclusive => "oneOf",
                    GroupAlternativeMode::Inclusive => "anyOf",
                };
                out.insert(keyword.into(), serde_json::Value::Array(variants));
                return;
            }
            let mut props = serde_json::Map::new();
            for child in children {
                let mut prop = serde_json::Map::new();
                render(child, &mut prop);
                props.insert(child.name.clone(), serde_json::Value::Object(prop));
            }
            out.insert("properties".into(), serde_json::Value::Object(props));
            if let Some(dynamic) = dynamic {
                let mut additional = serde_json::Map::new();
                render(dynamic, &mut additional);
                out.insert(
                    "additionalProperties".into(),
                    serde_json::Value::Object(additional),
                );
            } else {
                out.insert("additionalProperties".into(), false.into());
            }
        }
    }
}

fn constraint_value_to_json(value: &GroupAlternativeConstraintValue) -> serde_json::Value {
    match value {
        GroupAlternativeConstraintValue::String(value) => value.clone().into(),
        GroupAlternativeConstraintValue::Int(value) => (*value).into(),
        GroupAlternativeConstraintValue::Float(value) => value.get().into(),
        GroupAlternativeConstraintValue::Bool(value) => (*value).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import_str(text: &str) -> SchemaNode {
        import_str_result(text).unwrap()
    }

    fn import_str_result(text: &str) -> Result<SchemaNode, JsonFormatError> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_json_schema_test_{}_{}.json",
            std::process::id(),
            text.len()
        ));
        std::fs::write(&path, text).unwrap();
        let schema = import(&path);
        std::fs::remove_file(&path).unwrap();
        schema
    }

    #[test]
    fn compatible_object_one_of_preserves_and_roundtrips_alternatives() {
        let schema = import_str(
            r##"{
  "title": "Address",
  "type": "object",
  "oneOf": [
    { "$ref": "#/definitions/domestic" },
    { "$ref": "#/definitions/international" }
  ],
  "definitions": {
    "domestic": {
      "type": "object",
      "additionalProperties": false,
      "required": ["name", "state"],
      "properties": {
        "name": { "type": "string" },
        "state": { "type": "string" }
      }
    },
    "international": {
      "additionalProperties": false,
      "properties": {
        "name": { "type": "string" },
        "postcode": { "type": "string" }
      },
      "required": ["name", "postcode"]
    }
  }
}"##,
        );
        let SchemaKind::Group {
            children,
            alternatives,
            ..
        } = &schema.kind
        else {
            panic!("oneOf should import as a group projection");
        };
        assert_eq!(
            children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            ["name", "state", "postcode"]
        );
        assert_eq!(
            alternatives
                .iter()
                .map(|alternative| alternative.name.as_str())
                .collect::<Vec<_>>(),
            ["domestic", "international"]
        );

        let path = std::env::temp_dir().join(format!(
            "ferrule_json_schema_one_of_roundtrip_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, export(&schema)).unwrap();
        let roundtrip = import(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(roundtrip, schema);
    }

    #[test]
    fn compatible_object_any_of_preserves_inclusive_matching_and_roundtrips() {
        let schema = import_str(
            r##"{
  "title": "Record",
  "anyOf": [
    { "$ref": "#/$defs/labeled" },
    { "$ref": "#/$defs/detailed" }
  ],
  "$defs": {
    "labeled": {
      "title": "labeled",
      "type": "object",
      "additionalProperties": false,
      "required": ["id", "label"],
      "properties": {
        "id": { "type": "integer" },
        "label": { "type": "string" }
      }
    },
    "detailed": {
      "title": "detailed",
      "type": "object",
      "additionalProperties": false,
      "required": ["id"],
      "properties": {
        "id": { "type": "integer" },
        "label": { "type": "string" },
        "note": { "type": "string" }
      }
    }
  }
}"##,
        );
        assert_eq!(schema.alternative_mode(), GroupAlternativeMode::Inclusive);
        let alternatives = schema.alternatives();
        let universally_required = alternatives[0]
            .required
            .iter()
            .filter(|field| {
                alternatives[1..]
                    .iter()
                    .all(|alternative| alternative.required.contains(field))
            })
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(universally_required, ["id"]);
        assert!(crate::from_str(r#"{"id":7,"label":"both"}"#, &schema).is_ok());
        assert!(crate::from_str(r#"{"id":7}"#, &schema).is_ok());
        assert!(matches!(
            crate::from_str("{}", &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));

        let exported = export(&schema);
        assert!(exported.contains("\"anyOf\""));
        assert!(!exported.contains("\"oneOf\""));
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_schema_any_of_roundtrip_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, exported).unwrap();
        let roundtrip = import(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(roundtrip, schema);
    }

    #[test]
    fn incompatible_object_any_of_is_rejected_actionably() {
        let conflicting = import_str_result(
            r#"{
  "title": "Conflict",
  "anyOf": [
    { "type": "object", "additionalProperties": false,
      "properties": { "value": { "type": "string" } } },
    { "type": "object", "additionalProperties": false,
      "properties": { "value": { "type": "integer" } } }
  ]
}"#,
        )
        .unwrap_err();
        assert!(
            conflicting
                .to_string()
                .contains("field `value` has incompatible schemas")
        );

        let mixed = import_str_result(
            r#"{
  "title": "Mixed",
  "anyOf": [
    { "type": "object", "additionalProperties": false, "properties": {} },
    { "type": "string" }
  ]
}"#,
        )
        .unwrap_err();
        assert!(mixed.to_string().contains("only object alternatives"));
    }

    #[test]
    fn incompatible_and_scalar_one_of_are_rejected() {
        let incompatible = import_str_result(
            r#"{
  "title": "Bad",
  "oneOf": [
    { "type": "object", "additionalProperties": false, "properties": { "value": { "type": "string" } } },
    { "type": "object", "additionalProperties": false, "properties": { "value": { "type": "integer" } } }
  ]
}"#,
        )
        .unwrap_err();
        assert!(incompatible.to_string().contains("incompatible schemas"));

        let scalar = import_str_result(
            r#"{
  "title": "Scalar",
  "oneOf": [{ "type": "string" }, { "type": "integer" }]
}"#,
        )
        .unwrap_err();
        assert!(scalar.to_string().contains("only object alternatives"));
    }

    #[test]
    fn required_scalar_const_discriminators_roundtrip_and_validate_instances() {
        let schema = import_str(
            r#"{
  "title": "Event",
  "oneOf": [
    { "title": "created", "type": "object", "additionalProperties": false,
      "required": ["kind", "value"],
      "properties": {
        "kind": { "type": "string", "const": "created" },
        "value": { "type": "string" }
      } },
    { "title": "deleted", "type": "object", "additionalProperties": false,
      "required": ["kind", "value"],
      "properties": {
        "kind": { "type": "string", "const": "deleted" },
        "value": { "type": "string" }
      } }
  ]
}"#,
        );
        assert_eq!(
            schema
                .alternatives()
                .iter()
                .map(|alternative| {
                    let constraint = &alternative.constraints[0];
                    (constraint.member.as_str(), &constraint.value)
                })
                .collect::<Vec<_>>(),
            [
                (
                    "kind",
                    &GroupAlternativeConstraintValue::String("created".into())
                ),
                (
                    "kind",
                    &GroupAlternativeConstraintValue::String("deleted".into())
                )
            ]
        );
        for text in [
            r#"{"kind":"created","value":"one"}"#,
            r#"{"kind":"deleted","value":"two"}"#,
        ] {
            let instance = crate::from_str(text, &schema).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    &crate::to_string(&schema, &instance).unwrap()
                )
                .unwrap(),
                serde_json::from_str::<serde_json::Value>(text).unwrap()
            );
        }
        for text in [
            r#"{"kind":"changed","value":"three"}"#,
            r#"{"value":"four"}"#,
        ] {
            assert!(matches!(
                crate::from_str(text, &schema),
                Err(JsonFormatError::NoMatchingAlternative { .. })
            ));
        }

        let exported = export(&schema);
        let exported_value: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(
            exported_value["oneOf"][0]["properties"]["kind"]["const"],
            "created"
        );
        assert_eq!(
            exported_value["oneOf"][1]["properties"]["kind"]["const"],
            "deleted"
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_schema_discriminator_roundtrip_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, exported).unwrap();
        let roundtrip = import(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(roundtrip, schema);
    }

    #[test]
    fn inclusive_alternatives_honor_required_scalar_const_discriminators() {
        let schema = import_str(
            r#"{
  "title":"Message",
  "anyOf":[
    {"type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":"text"}}},
    {"type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":"image"}}}
  ]
}"#,
        );
        assert_eq!(schema.alternative_mode(), GroupAlternativeMode::Inclusive);
        assert!(crate::from_str(r#"{"kind":"text"}"#, &schema).is_ok());
        assert!(matches!(
            crate::from_str(r#"{"kind":"audio"}"#, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
        let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
        assert_eq!(exported["anyOf"][1]["properties"]["kind"]["const"], "image");
    }

    #[test]
    fn bool_integer_and_number_const_discriminators_roundtrip() {
        let cases = [
            (
                "boolean",
                "true",
                "false",
                r#"{"kind":true,"value":"one"}"#,
                r#"{"kind":false,"value":"two"}"#,
                r#"{"kind":"true","value":"bad"}"#,
            ),
            (
                "integer",
                "7",
                "9",
                r#"{"kind":7,"value":"one"}"#,
                r#"{"kind":9,"value":"two"}"#,
                r#"{"kind":8,"value":"bad"}"#,
            ),
            (
                "number",
                "1.25",
                "2.5",
                r#"{"kind":1.25,"value":"one"}"#,
                r#"{"kind":2.5,"value":"two"}"#,
                r#"{"kind":3.75,"value":"bad"}"#,
            ),
        ];
        for (ty, first, second, first_instance, second_instance, rejected) in cases {
            let text = format!(
                r#"{{
  "title":"TypedEvent",
  "oneOf":[
    {{"title":"first","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{{"kind":{{"type":"{ty}","const":{first}}},"value":{{"type":"string"}}}}}},
    {{"title":"second","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{{"kind":{{"type":"{ty}","const":{second}}},"value":{{"type":"string"}}}}}}
  ]
}}"#
            );
            let schema = import_str(&text);
            for instance_text in [first_instance, second_instance] {
                let instance = crate::from_str(instance_text, &schema).unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(
                        &crate::to_string(&schema, &instance).unwrap()
                    )
                    .unwrap(),
                    serde_json::from_str::<serde_json::Value>(instance_text).unwrap()
                );
            }
            assert!(matches!(
                crate::from_str(rejected, &schema),
                Err(JsonFormatError::NoMatchingAlternative { .. })
            ));

            let path = std::env::temp_dir().join(format!(
                "ferrule_json_schema_typed_discriminator_{}_{}.json",
                ty,
                std::process::id()
            ));
            std::fs::write(&path, export(&schema)).unwrap();
            let roundtrip = import(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            assert_eq!(roundtrip, schema);
        }
    }

    #[test]
    fn const_discriminators_infer_scalar_types() {
        let schema = import_str(
            r#"{
  "title":"Implicit",
  "oneOf":[
    {"title":"yes","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":true}}},
    {"title":"no","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":false}}}
  ]
}"#,
        );
        assert!(matches!(
            schema.child("kind").map(|child| &child.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Bool
            })
        ));
        assert!(crate::from_str(r#"{"kind":true}"#, &schema).is_ok());
        let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
        assert_eq!(exported["oneOf"][0]["properties"]["kind"]["const"], true);
    }

    #[test]
    fn unsupported_const_discriminators_are_rejected_actionably() {
        for (property, required, expected) in [
            (r#"{"type":"string","const":"a"}"#, "", "must be required"),
            (
                r#"{"type":"string","const":1}"#,
                r#", "required":["kind"]"#,
                "does not match its declared scalar type",
            ),
            (
                r#"{"type":"string","const":null}"#,
                r#", "required":["kind"]"#,
                "cannot be null",
            ),
            (
                r#"{"type":"integer","const":9223372036854775808}"#,
                r#", "required":["kind"]"#,
                "signed 64-bit integer",
            ),
            (
                r#"{"type":"number","const":9007199254740993}"#,
                r#", "required":["kind"]"#,
                "finite exactly supported number",
            ),
        ] {
            let text = format!(
                r#"{{
  "title":"Unsupported",
  "oneOf":[
    {{"type":"object","additionalProperties":false{required},"properties":{{"kind":{property}}}}},
    {{"type":"object","additionalProperties":false,"required":["other"],"properties":{{"other":{{"type":"string"}}}}}}
  ]
}}"#
            );
            let error = import_str_result(&text).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }

        let ambiguous = import_str_result(
            r#"{
  "title":"Ambiguous",
  "oneOf":[
    {"title":"first","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"type":"boolean","const":true}}},
    {"title":"second","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"type":"boolean","const":true}}}
  ]
}"#,
        )
        .unwrap_err();
        assert!(
            ambiguous
                .to_string()
                .contains("alternatives are not distinguishable"),
            "{ambiguous}"
        );

        for property in [
            r#"{"type":"array","const":[],"items":{"type":"string"}}"#,
            r#"{"type":"object","const":{},"additionalProperties":false}"#,
        ] {
            let text = format!(
                r#"{{
  "title":"StructuredConst",
  "oneOf":[
    {{"type":"object","additionalProperties":false,"required":["kind"],"properties":{{"kind":{property}}}}},
    {{"type":"object","additionalProperties":false,"required":["other"],"properties":{{"other":{{"type":"string"}}}}}}
  ]
}}"#
            );
            let error = import_str_result(&text).unwrap_err();
            assert!(
                error.to_string().contains("const discriminator `kind`"),
                "{error}"
            );
        }
    }

    #[test]
    fn imports_nested_arrays_and_objects() {
        let schema = import_str(
            r#"{
  "title": "Orders",
  "type": "object",
  "properties": {
    "Date": { "type": "string" },
    "Order": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "Order_ID": { "type": "string" },
          "Total": { "type": "number" },
          "Line_Count": { "type": "integer" },
          "Rush": { "type": "boolean" }
        }
      }
    }
  }
}"#,
        );

        assert_eq!(schema.name, "Orders");
        assert!(!schema.repeating);
        assert!(!schema.child("Date").unwrap().repeating);

        let order = schema.child("Order").unwrap();
        assert!(order.repeating);
        assert!(matches!(
            order.child("Total").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Float
            }
        ));
        assert!(matches!(
            order.child("Line_Count").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
        assert!(matches!(
            order.child("Rush").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Bool
            }
        ));
    }

    #[test]
    fn resolves_local_refs_including_root_and_defs() {
        let schema = import_str(
            r##"{
  "$ref": "#/definitions/company",
  "definitions": {
    "company": {
      "title": "Company",
      "type": "object",
      "properties": {
        "Name": { "type": "string" },
        "Office": {
          "type": "array",
          "items": { "$ref": "#/$defs/office" }
        }
      }
    }
  },
  "$defs": {
    "office": {
      "type": "object",
      "properties": {
        "City": { "type": "string" },
        "Staff": { "type": "integer" }
      }
    }
  }
}"##,
        );

        assert_eq!(schema.name, "Company");
        let office = schema.child("Office").unwrap();
        assert!(office.repeating);
        assert!(matches!(
            office.child("Staff").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
    }

    #[test]
    fn cyclic_and_external_refs_degrade_to_string_scalars() {
        let schema = import_str(
            r##"{
  "title": "Tree",
  "type": "object",
  "properties": {
    "Label": { "type": "string" },
    "Child": { "$ref": "#/properties/Child" },
    "Remote": { "$ref": "other.json#/definitions/x" }
  }
}"##,
        );

        for field in ["Child", "Remote"] {
            assert!(matches!(
                schema.child(field).unwrap().kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            ));
        }
    }

    #[test]
    fn nullable_type_arrays_use_the_only_non_null_type() {
        let schema = import_str(
            r#"{
  "title": "Row",
  "type": "object",
  "properties": {
    "Count": { "type": ["integer", "null"] }
  }
}"#,
        );
        assert!(matches!(
            schema.child("Count").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
    }

    #[test]
    fn type_arrays_with_multiple_non_null_types_are_rejected() {
        let error = import_str_result(
            r#"{
  "title":"Ambiguous",
  "type":["string", "integer", "null"]
}"#,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("type arrays may contain only one non-null type")
        );
    }

    #[test]
    fn repeating_object_alternatives_are_rejected() {
        let error = import_str_result(
            r#"{
  "title":"Sequences",
  "oneOf":[
    {"type":"array","items":{"type":"object","additionalProperties":false,"properties":{}}},
    {"type":"array","items":{"type":"object","additionalProperties":false,"properties":{}}}
  ]
}"#,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("array alternatives are not supported")
        );
    }

    #[test]
    fn export_then_import_roundtrips() {
        let schema = SchemaNode::group(
            "Orders",
            vec![
                SchemaNode::scalar("Date", ScalarType::String),
                SchemaNode::group(
                    "Order",
                    vec![
                        SchemaNode::scalar("Qty", ScalarType::Int),
                        SchemaNode::scalar("Price", ScalarType::Float),
                        SchemaNode::scalar("Rush", ScalarType::Bool),
                    ],
                )
                .repeating(),
            ],
        );
        let text = export(&schema);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_schema_export_test_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, text).unwrap();
        let imported = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(imported, schema);
    }

    #[test]
    fn repeating_root_exports_as_top_level_array() {
        let schema =
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Name", ScalarType::String)])
                .repeating();
        let text = export(&schema);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["type"], "array");
        assert_eq!(value["items"]["type"], "object");

        let path = std::env::temp_dir().join(format!(
            "ferrule_json_schema_export_arr_test_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, export(&schema)).unwrap();
        let imported = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(imported, schema);
    }

    #[test]
    fn typed_additional_properties_roundtrip_as_dynamic_fields() {
        let schema = import_str(
            r#"{
  "title": "Metrics",
  "type": "object",
  "properties": { "source": { "type": "string" } },
  "additionalProperties": { "type": "number" }
}"#,
        );
        assert!(matches!(
            schema.dynamic_fields().map(|node| &node.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Float
            })
        ));
        let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
        assert_eq!(exported["additionalProperties"]["type"], "number");
    }

    #[test]
    fn typed_object_additional_properties_preserve_their_exact_shape() {
        let schema = import_str(
            r#"{
  "title": "Directory",
  "type": "object",
  "additionalProperties": {
    "type": "object",
    "properties": { "name": { "type": "string" } },
    "additionalProperties": false
  }
}"#,
        );
        let dynamic = schema.dynamic_fields().unwrap();
        assert!(matches!(dynamic.kind, SchemaKind::Group { .. }));
        assert_eq!(
            dynamic.child("name").map(|child| &child.kind),
            Some(&SchemaKind::Scalar {
                ty: ScalarType::String,
            })
        );

        let exported = export(&schema);
        let value: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(value["additionalProperties"]["additionalProperties"], false);
        assert_eq!(import_str(&exported), schema);
    }

    #[test]
    fn explicit_unconstrained_additional_properties_are_rejected() {
        for additional in ["true", "{}"] {
            let text = format!(
                r#"{{"title":"Open","type":"object","additionalProperties":{additional}}}"#
            );
            assert!(matches!(
                import_str_result(&text),
                Err(JsonFormatError::UnsupportedSchemaObject { reason, .. })
                    if reason.contains("unconstrained additionalProperties")
            ));
        }
    }

    #[test]
    fn closed_groups_export_explicit_closed_object_semantics() {
        let schema = SchemaNode::group(
            "Closed",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        );
        let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
        assert_eq!(exported["additionalProperties"], false);
    }
}
