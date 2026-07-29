use ir::{FiniteF64, JsonAllowedValue, SchemaNode, Value};

use super::ConstraintValidationError;

pub(super) fn validate_instance_value(
    schema: &SchemaNode,
    value: &Value,
) -> Result<(), ConstraintValidationError> {
    let Some(allowed) = &schema.json_allowed_values else {
        return Ok(());
    };
    if matches!(value, Value::Null | Value::XmlNil(_)) || allowed.matches(value) {
        return Ok(());
    }
    Err(mismatch(schema))
}

pub(super) fn validate_json_value(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), ConstraintValidationError> {
    let Some(allowed) = &schema.json_allowed_values else {
        return Ok(());
    };
    let matches = match value {
        serde_json::Value::String(value) => allowed
            .values()
            .iter()
            .any(|candidate| matches!(candidate, JsonAllowedValue::String(expected) if expected == value)),
        serde_json::Value::Number(value) => json_number(value)
            .as_ref()
            .is_some_and(|value| allowed.contains(value)),
        serde_json::Value::Bool(value) => allowed.contains(&JsonAllowedValue::Bool(*value)),
        serde_json::Value::Null => allowed.contains_json_null(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    };
    if matches {
        Ok(())
    } else {
        Err(mismatch(schema))
    }
}

fn json_number(value: &serde_json::Number) -> Option<JsonAllowedValue> {
    if let Some(value) = value.as_i64() {
        return Some(JsonAllowedValue::Int(value));
    }
    value
        .as_f64()
        .and_then(FiniteF64::new)
        .map(JsonAllowedValue::Float)
}

fn mismatch(schema: &SchemaNode) -> ConstraintValidationError {
    ConstraintValidationError::AllowedValueMismatch {
        name: schema.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use ir::{Instance, JsonAllowedValues, ScalarType, ScalarTypeSet, SchemaNode, Value};

    use super::super::{JsonBoundaryError, parse_json, serialize_json};

    use super::*;

    fn finite(value: f64) -> Result<FiniteF64, &'static str> {
        FiniteF64::new(value).ok_or("test value must be finite")
    }

    #[test]
    fn validates_exact_typed_membership_null_and_absence() -> Result<(), Box<dyn std::error::Error>>
    {
        let types = ScalarTypeSet::new([ScalarType::Int, ScalarType::Float])
            .ok_or("test scalar union must have two types")?;
        let allowed = JsonAllowedValues::new([
            JsonAllowedValue::JsonNull,
            JsonAllowedValue::Float(finite(1.5)?),
            JsonAllowedValue::Int(9_007_199_254_740_993),
        ])?;
        let field = SchemaNode::scalar_union("Value", types)
            .with_json_allowed_values(allowed)
            .ok_or("test allowed values must match the scalar union")?;
        let encoded = serde_json::to_string(&field)?;

        assert_eq!(
            parse_json(&encoded, "9007199254740993")?,
            Instance::Scalar(Value::Int(9_007_199_254_740_993))
        );
        assert_eq!(
            parse_json(&encoded, "1.5")?,
            Instance::Scalar(Value::Float(1.5))
        );
        assert_eq!(
            parse_json(&encoded, "null")?,
            Instance::Scalar(Value::json_null())
        );
        assert!(matches!(
            parse_json(&encoded, "9007199254740992"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("allowed values")
        ));

        let document = SchemaNode::group("Document", vec![field]);
        assert!(parse_json(&serde_json::to_string(&document)?, "{}").is_ok());
        Ok(())
    }

    #[test]
    fn validates_repeated_values_and_normalized_output() -> Result<(), Box<dyn std::error::Error>> {
        let labels = JsonAllowedValues::new([
            JsonAllowedValue::String("A".to_string()),
            JsonAllowedValue::String("B".to_string()),
        ])?;
        let labels = SchemaNode::scalar("Label", ScalarType::String)
            .with_json_allowed_values(labels)
            .ok_or("test allowed values must match strings")?
            .repeating();
        let encoded = serde_json::to_string(&labels)?;
        assert!(parse_json(&encoded, r#"["A","B"]"#).is_ok());
        assert!(matches!(
            parse_json(&encoded, r#"["A","C"]"#),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("allowed values")
        ));

        let amounts = JsonAllowedValues::new([
            JsonAllowedValue::Int(9_007_199_254_740_993),
            JsonAllowedValue::Float(finite(1.5)?),
        ])?;
        let amounts = SchemaNode::scalar("Amount", ScalarType::Float)
            .with_json_allowed_values(amounts)
            .ok_or("test allowed values must match numbers")?;
        let encoded = serde_json::to_string(&amounts)?;
        assert_eq!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("1.5".to_string()))
            )?,
            "1.5\n"
        );
        assert!(matches!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("9007199254740993".to_string()))
            ),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("allowed values")
        ));
        Ok(())
    }

    #[test]
    fn rejects_noncanonical_embedded_allowed_values() {
        let malformed = r#"{
            "name":"Value",
            "json_allowed_values":[
                {"type":"int","value":2},
                {"type":"int","value":1}
            ],
            "kind":{"kind":"scalar","ty":"int"}
        }"#;
        assert!(matches!(
            parse_json(malformed, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { .. })
        ));
    }
}
