//! A deliberately small JSON Schema importer: enough to turn the common
//! `type: object/array/scalar` shapes into a [`SchemaNode`] tree. It reads
//! `properties` (in document order) and `items`, and maps `integer`/
//! `number`/`boolean` to the corresponding scalar types. It does not
//! support `$ref`, `oneOf`/`anyOf`, `additionalProperties`, or validation
//! keywords -- the same "lite" scope as the XSD importer.

use ir::{ScalarType, SchemaNode};

use crate::JsonFormatError;

/// Imports the root of a JSON Schema file as a [`SchemaNode`]. The root
/// node is named by the schema's `title`, falling back to `"root"`.
pub fn import(path: &std::path::Path) -> Result<SchemaNode, JsonFormatError> {
    let text = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let name = value
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("root");
    Ok(parse(name, &value))
}

fn parse(name: &str, schema: &serde_json::Value) -> SchemaNode {
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("object") => {
            let children = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|props| {
                    props
                        .iter()
                        .map(|(child_name, child_schema)| parse(child_name, child_schema))
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
            parse(name, &items).repeating()
        }
        Some("integer") => SchemaNode::scalar(name, ScalarType::Int),
        Some("number") => SchemaNode::scalar(name, ScalarType::Float),
        Some("boolean") => SchemaNode::scalar(name, ScalarType::Bool),
        _ => SchemaNode::scalar(name, ScalarType::String),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::SchemaKind;

    #[test]
    fn imports_nested_arrays_and_objects() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_json_schema_test_{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
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
        )
        .unwrap();

        let schema = import(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

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
}
