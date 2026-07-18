use ir::Value;

use crate::FunctionError;

const FUNCTION: &str = "json_serialize_object";

pub(super) fn serialize_object(args: &[Value]) -> Result<Value, FunctionError> {
    if args.is_empty() || !args.len().is_multiple_of(3) {
        return Err(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "expected path, scalar type, and value triples",
        });
    }

    let mut root = serde_json::Map::new();
    let (fields, _) = args.as_chunks::<3>();
    for field in fields {
        let [Value::String(path), Value::String(scalar_type), value] = field else {
            return Err(FunctionError::InvalidArgument {
                function: FUNCTION,
                message: "paths and scalar types must be strings",
            });
        };
        if matches!(value, Value::Null) {
            continue;
        }
        let path: Vec<String> =
            serde_json::from_str(path).map_err(|_| FunctionError::InvalidArgument {
                function: FUNCTION,
                message: "path descriptors must be JSON string arrays",
            })?;
        if path.is_empty() {
            return Err(FunctionError::InvalidArgument {
                function: FUNCTION,
                message: "property paths cannot be empty",
            });
        }
        let value = scalar_value(scalar_type, value)?;
        insert(&mut root, &path, value)?;
    }

    serde_json::to_string(&serde_json::Value::Object(root))
        .map(Value::String)
        .map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "constructed object could not be serialized",
        })
}

fn scalar_value(scalar_type: &str, value: &Value) -> Result<serde_json::Value, FunctionError> {
    let mismatch = || FunctionError::TypeMismatch {
        function: FUNCTION,
        got: value.type_name(),
    };
    match (scalar_type, value) {
        ("string", Value::String(value)) => Ok(serde_json::Value::String(value.clone())),
        ("string", Value::Bool(value)) => Ok(serde_json::Value::String(value.to_string())),
        ("string", Value::Int(value)) => Ok(serde_json::Value::String(value.to_string())),
        ("string", Value::Float(value)) if value.is_finite() => {
            Ok(serde_json::Value::String(value.to_string()))
        }
        ("integer", Value::Int(value)) => Ok(serde_json::Value::Number((*value).into())),
        ("integer", Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value < -(i64::MIN as f64) =>
        {
            Ok(serde_json::Value::Number((*value as i64).into()))
        }
        ("integer", Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| serde_json::Value::Number(value.into()))
            .map_err(|_| mismatch()),
        ("number", Value::Int(value)) => Ok(serde_json::Value::Number((*value).into())),
        ("number", Value::Float(value)) if value.is_finite() => {
            serde_json::Number::from_f64(*value)
                .map(serde_json::Value::Number)
                .ok_or_else(mismatch)
        }
        ("number", Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number)
            .ok_or_else(mismatch),
        ("boolean", Value::Bool(value)) => Ok(serde_json::Value::Bool(*value)),
        ("boolean", Value::String(value)) => match value.trim() {
            "true" | "1" => Ok(serde_json::Value::Bool(true)),
            "false" | "0" => Ok(serde_json::Value::Bool(false)),
            _ => Err(mismatch()),
        },
        _ => Err(mismatch()),
    }
}

fn insert(
    object: &mut serde_json::Map<String, serde_json::Value>,
    path: &[String],
    value: serde_json::Value,
) -> Result<(), FunctionError> {
    let Some((name, rest)) = path.split_first() else {
        return Err(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "property paths cannot be empty",
        });
    };
    if rest.is_empty() {
        if object.insert(name.clone(), value).is_some() {
            return Err(FunctionError::InvalidArgument {
                function: FUNCTION,
                message: "property paths must be unique",
            });
        }
        return Ok(());
    }
    let child = object
        .entry(name.clone())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let serde_json::Value::Object(child) = child else {
        return Err(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "property path conflicts with a scalar property",
        });
    };
    insert(child, rest, value)
}

#[cfg(test)]
mod tests {
    use ir::Value;

    use super::serialize_object;

    #[test]
    fn serializes_nested_typed_properties_and_omits_nulls() {
        let value = serialize_object(&[
            Value::String(r#"["Shares"]"#.into()),
            Value::String("number".into()),
            Value::String("3.5".into()),
            Value::String(r#"["Leaves","Total"]"#.into()),
            Value::String("integer".into()),
            Value::Int(7),
            Value::String(r#"["Leaves","Used"]"#.into()),
            Value::String("integer".into()),
            Value::Null,
        ]);
        assert_eq!(
            value,
            Ok(Value::String(
                r#"{"Shares":3.5,"Leaves":{"Total":7}}"#.into()
            ))
        );
    }

    #[test]
    fn serializes_exact_integral_float_properties_as_integers() {
        assert_eq!(
            serialize_object(&[
                Value::String(r#"["Shares"]"#.into()),
                Value::String("integer".into()),
                Value::Float(42.0),
            ]),
            Ok(Value::String(r#"{"Shares":42}"#.into()))
        );

        for value in [
            Value::Float(42.5),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
            Value::Float(i64::MAX as f64),
        ] {
            assert!(
                serialize_object(&[
                    Value::String(r#"["Shares"]"#.into()),
                    Value::String("integer".into()),
                    value,
                ])
                .is_err()
            );
        }
    }
}
