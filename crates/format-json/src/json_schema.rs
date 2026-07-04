//! A deliberately small JSON Schema importer: enough to turn the common
//! `type: object/array/scalar` shapes into a [`SchemaNode`] tree. It reads
//! `properties` (in document order) and `items`, maps `integer`/`number`/
//! `boolean` to the corresponding scalar types, and resolves document-local
//! `$ref` pointers (`#/definitions/...`, `#/$defs/...`; cyclic or external
//! refs degrade to string scalars). It does not support `oneOf`/`anyOf`,
//! `additionalProperties`, or validation keywords -- the same "lite" scope
//! as the XSD importer.

use ir::{ScalarType, SchemaKind, SchemaNode};

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
    Ok(parse(name, &value, &value, &mut Vec::new()))
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
) -> SchemaNode {
    if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
        // Cyclic and external (non-`#/...`) refs degrade to string scalars.
        if active_refs.iter().any(|a| a == r) {
            return SchemaNode::scalar(name, ScalarType::String);
        }
        let Some(resolved) = resolve_ref(doc, r) else {
            return SchemaNode::scalar(name, ScalarType::String);
        };
        active_refs.push(r.to_string());
        let node = parse(name, resolved, doc, active_refs);
        active_refs.pop();
        return node;
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
            let children = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|props| {
                    props
                        .iter()
                        .map(|(child_name, child_schema)| {
                            parse(child_name, child_schema, doc, active_refs)
                        })
                        .collect()
                })
                .unwrap_or_default();
            SchemaNode::group(name, children)
        }
        Some("array") => {
            let items = schema
                .get("items")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            parse(name, &items, doc, active_refs).repeating()
        }
        Some("integer") => SchemaNode::scalar(name, ScalarType::Int),
        Some("number") => SchemaNode::scalar(name, ScalarType::Float),
        Some("boolean") => SchemaNode::scalar(name, ScalarType::Bool),
        _ => SchemaNode::scalar(name, ScalarType::String),
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
        SchemaKind::Group { children } => {
            out.insert("type".into(), "object".into());
            let mut props = serde_json::Map::new();
            for child in children {
                let mut prop = serde_json::Map::new();
                render(child, &mut prop);
                props.insert(child.name.clone(), serde_json::Value::Object(prop));
            }
            out.insert("properties".into(), serde_json::Value::Object(props));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import_str(text: &str) -> SchemaNode {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_json_schema_test_{}_{}.json",
            std::process::id(),
            text.len()
        ));
        std::fs::write(&path, text).unwrap();
        let schema = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        schema
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
}
