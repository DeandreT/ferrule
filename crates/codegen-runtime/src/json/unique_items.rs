use format_json::json_schema::unique_items::{
    UniqueItemsValidationError, validate_unique_json_items,
};
use ir::{SchemaKind, SchemaNode};

use super::JsonBoundaryError;

#[derive(Debug)]
pub(super) enum ValidationError {
    Limit {
        name: String,
        resource: &'static str,
        max: usize,
    },
    Mismatch {
        name: String,
        first_index: usize,
        duplicate_index: usize,
    },
}

pub(super) fn schema_contains_assertion(schema: &SchemaNode) -> bool {
    if schema.json_unique_items {
        return true;
    }
    let SchemaKind::Group {
        children, dynamic, ..
    } = &schema.kind
    else {
        return false;
    };
    children.iter().any(schema_contains_assertion)
        || dynamic.as_deref().is_some_and(schema_contains_assertion)
}

pub(super) fn validate_document(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), ValidationError> {
    if schema.repeating {
        if let serde_json::Value::Array(items) = value {
            validate_array(schema, items)?;
            for item in items {
                validate_node(schema, item)?;
            }
        }
        return Ok(());
    }
    // Generated root scopes can serialize flat rows against one row schema.
    if let serde_json::Value::Array(items) = value {
        for item in items {
            validate_node(schema, item)?;
        }
        return Ok(());
    }
    validate_node(schema, value)
}

fn validate_array(schema: &SchemaNode, items: &[serde_json::Value]) -> Result<(), ValidationError> {
    if !schema.json_unique_items {
        return Ok(());
    }
    validate_unique_json_items(items).map_err(|error| match error {
        UniqueItemsValidationError::Duplicate {
            first_index,
            duplicate_index,
        } => ValidationError::Mismatch {
            name: schema.name.clone(),
            first_index,
            duplicate_index,
        },
        UniqueItemsValidationError::Limit { resource, max } => ValidationError::Limit {
            name: schema.name.clone(),
            resource,
            max,
        },
    })
}

fn validate_node(schema: &SchemaNode, value: &serde_json::Value) -> Result<(), ValidationError> {
    let (
        SchemaKind::Group {
            children, dynamic, ..
        },
        serde_json::Value::Object(fields),
    ) = (&schema.kind, value)
    else {
        return Ok(());
    };
    for (name, value) in fields {
        let child = children
            .iter()
            .find(|child| child.name == *name)
            .or(dynamic.as_deref());
        if let Some(child) = child {
            validate_document(child, value)?;
        }
    }
    Ok(())
}

pub(super) fn output_error(error: ValidationError) -> JsonBoundaryError {
    JsonBoundaryError::InvalidOutput {
        message: error.message(),
    }
}

impl ValidationError {
    fn message(self) -> String {
        match self {
            Self::Limit {
                name,
                resource,
                max,
            } => format!(
                "JSON uniqueItems validation for `{name}` exceeds the {max} {resource} limit"
            ),
            Self::Mismatch {
                name,
                first_index,
                duplicate_index,
            } => format!(
                "`{name}` requires unique JSON array items, but indexes {first_index} and {duplicate_index} are equal"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use ir::{Instance, MAX_JSON_UNIQUE_ITEMS, ScalarType, SchemaNode, Value};

    use super::super::{JsonBoundaryError, parse_json, serialize_json};

    fn unique_rows() -> Result<SchemaNode, &'static str> {
        SchemaNode::group(
            "Row",
            vec![
                SchemaNode::scalar("Count", ScalarType::Float),
                SchemaNode::scalar("Tags", ScalarType::Int).repeating(),
                SchemaNode::group(
                    "Meta",
                    vec![SchemaNode::scalar("Label", ScalarType::String)],
                ),
            ],
        )
        .repeating()
        .with_json_unique_items()
        .ok_or("test row array accepts uniqueItems")
    }

    #[test]
    fn raw_input_uses_exact_structural_json_equality() -> Result<(), Box<dyn std::error::Error>> {
        let encoded = serde_json::to_string(&unique_rows()?)?;
        let duplicate = r#"[
            {"Count":1,"Tags":[1,2],"Meta":{"Label":"A"}},
            {"Meta":{"Label":"A"},"Tags":[1,2],"Count":1.0}
        ]"#;
        assert!(matches!(
            parse_json(&encoded, duplicate),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("indexes 1 and 2")
        ));
        let signed_zero_duplicate = r#"[
            {"Count":-0.0,"Tags":[],"Meta":{"Label":"zero"}},
            {"Count":0,"Tags":[],"Meta":{"Label":"zero"}}
        ]"#;
        assert!(matches!(
            parse_json(&encoded, signed_zero_duplicate),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("indexes 1 and 2")
        ));

        let ordered_arrays_and_exact_strings = r#"[
            {"Count":1,"Tags":[1,2],"Meta":{"Label":"A"}},
            {"Meta":{"Label":"a"},"Tags":[2,1],"Count":1.0}
        ]"#;
        assert!(parse_json(&encoded, ordered_arrays_and_exact_strings).is_ok());

        let distinct_large_decimals = r#"[
            {"Count":9007199254740993.0,"Tags":[],"Meta":{"Label":"x"}},
            {"Count":9007199254740992,"Tags":[],"Meta":{"Label":"x"}}
        ]"#;
        assert!(parse_json(&encoded, distinct_large_decimals).is_ok());
        Ok(())
    }

    #[test]
    fn normalized_output_is_checked_after_scalar_coercion() -> Result<(), Box<dyn std::error::Error>>
    {
        let schema = SchemaNode::scalar("Amount", ScalarType::Float)
            .repeating()
            .with_json_unique_items()
            .ok_or("test number array accepts uniqueItems")?;
        let encoded = serde_json::to_string(&schema)?;
        let duplicate = Instance::Repeated(vec![
            Instance::Scalar(Value::String("9007199254740993.0".to_string())),
            Instance::Scalar(Value::String("9007199254740992".to_string())),
        ]);
        assert!(matches!(
            serialize_json(&encoded, &duplicate),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("indexes 1 and 2")
        ));
        let distinct = Instance::Repeated(vec![
            Instance::Scalar(Value::String("1".to_string())),
            Instance::Scalar(Value::String("2.0".to_string())),
        ]);
        assert_eq!(
            serialize_json(&encoded, &distinct)?,
            "[\n  1.0,\n  2.0\n]\n"
        );
        Ok(())
    }

    #[test]
    fn malformed_embedded_unique_items_domain_is_schema_error() {
        let malformed = r#"{
            "name":"Value",
            "json_unique_items":true,
            "kind":{"kind":"scalar","ty":"string"}
        }"#;
        assert!(matches!(
            parse_json(malformed, r#""value""#),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { .. })
        ));
    }

    #[test]
    fn item_budget_maps_to_a_typed_input_limit() -> Result<(), Box<dyn std::error::Error>> {
        let schema = SchemaNode::scalar("Value", ScalarType::String)
            .repeating()
            .with_json_unique_items()
            .ok_or("test string array accepts uniqueItems")?;
        let mut document = String::with_capacity((MAX_JSON_UNIQUE_ITEMS + 1) * 5 + 2);
        document.push('[');
        for index in 0..=MAX_JSON_UNIQUE_ITEMS {
            if index != 0 {
                document.push(',');
            }
            document.push_str("null");
        }
        document.push(']');
        let error = parse_json(&serde_json::to_string(&schema)?, &document);
        assert!(matches!(
            error,
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("1000000 array items limit")
        ));
        Ok(())
    }
}
