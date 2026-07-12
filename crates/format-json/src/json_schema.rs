//! A deliberately small JSON Schema importer: enough to turn the common
//! `type: object/array/scalar` shapes into a [`SchemaNode`] tree. It reads
//! `properties` (in document order) and `items`, maps `integer`/`number`/
//! `boolean` to the corresponding scalar types, and resolves document-local
//! `$ref` pointers (`#/definitions/...`, `#/$defs/...`; cyclic or external
//! refs degrade to string scalars). Compatible closed-object `oneOf` unions
//! and typed `additionalProperties` schemas are preserved. An omitted or
//! false `additionalProperties` is treated as closed; explicitly
//! unconstrained `true`/`{}` schemas, `anyOf`, and validation keywords remain
//! outside this "lite" subset.

use ir::{GroupAlternative, ScalarType, SchemaKind, SchemaNode};

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
    if schema.get("anyOf").is_some() {
        return Err(unsupported_union(name, "anyOf is not supported"));
    }
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
        return parse_object_alternatives(name, schema, alternatives, doc, active_refs);
    }
    // Nullable unions like ["string", "null"] use the first real type.
    let ty = match schema.get("type") {
        Some(serde_json::Value::Array(types)) => types
            .iter()
            .filter_map(|t| t.as_str())
            .find(|t| *t != "null"),
        other => other.and_then(|t| t.as_str()),
    };
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
        _ if schema.get("properties").is_some() => {
            let children = parse_properties(schema, doc, active_refs)?;
            attach_dynamic_fields(SchemaNode::group(name, children), schema, doc, active_refs)
        }
        _ => Ok(SchemaNode::scalar(name, ScalarType::String)),
    }
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
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let alternatives = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
        .ok_or_else(|| unsupported_union(name, "oneOf must contain at least two alternatives"))?;
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
            .unwrap_or_else(|| format!("oneOf{index}"));
        let parsed = parse(&alternative_name, alternative_schema, doc, active_refs)?;
        let SchemaKind::Group {
            children: variant_children,
            ..
        } = parsed.kind
        else {
            return Err(unsupported_union(
                name,
                "only object alternatives are supported",
            ));
        };
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
        if metadata.iter().any(|previous: &GroupAlternative| {
            previous.members == members && previous.required == required
        }) {
            return Err(unsupported_union(
                name,
                "alternatives are not distinguishable by supported object fields and requirements",
            ));
        }
        metadata.push(GroupAlternative {
            name: alternative_name,
            members,
            required,
        });
    }
    SchemaNode::group(name, merged)
        .with_alternatives(metadata)
        .ok_or_else(|| unsupported_union(name, "alternative metadata is internally inconsistent"))
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
                out.insert("oneOf".into(), serde_json::Value::Array(variants));
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

        let discriminator = import_str_result(
            r#"{
  "title": "Discriminator",
  "oneOf": [
    { "type": "object", "additionalProperties": false, "required": ["kind"],
      "properties": { "kind": { "const": "a" } } },
    { "type": "object", "additionalProperties": false, "required": ["kind"],
      "properties": { "kind": { "const": "b" } } }
  ]
}"#,
        )
        .unwrap_err();
        assert!(discriminator.to_string().contains("not distinguishable"));
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
    fn nullable_type_arrays_use_the_first_real_type() {
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
