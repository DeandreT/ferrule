use ir::{Instance, SchemaKind, SchemaNode, Value};

use crate::FunctionError;

const FUNCTION: &str = "json_serialize_object";
const PARSE_FUNCTION: &str = "json_parse_field";
const OBJECT_FUNCTION: &str = "json_parse_object_exists";

pub(super) fn array_length(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "json_parse_array_length";
    if args.len() != 3 {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 3,
            got: args.len(),
        });
    }
    let array = parse_array(args, FUNCTION)?;
    i64::try_from(array.len())
        .map(Value::Int)
        .map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "array length exceeds the supported range",
        })
}

pub(super) fn array_field(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "json_parse_array_field";
    if args.len() != 5 {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 5,
            got: args.len(),
        });
    }
    let array = parse_array(args, FUNCTION)?;
    let Value::String(path) = &args[3] else {
        return Err(FunctionError::TypeMismatch {
            function: FUNCTION,
            got: args[3].type_name(),
        });
    };
    let path: Vec<String> =
        serde_json::from_str(path).map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "field path descriptor is invalid",
        })?;
    let index = match args[4] {
        Value::Int(index) => index
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok()),
        _ => {
            return Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got: args[4].type_name(),
            });
        }
    };
    let item = index
        .and_then(|index| array.get(index))
        .ok_or(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "array index is out of range",
        })?;
    scalar_at(item, &path)
        .cloned()
        .ok_or(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "field path does not resolve to a scalar",
        })
}

fn parse_array(args: &[Value], function: &'static str) -> Result<Vec<Instance>, FunctionError> {
    let mut text = Vec::with_capacity(3);
    for value in &args[..3] {
        let Value::String(value) = value else {
            return Err(FunctionError::TypeMismatch {
                function,
                got: value.type_name(),
            });
        };
        text.push(value);
    }
    let schema: SchemaNode =
        serde_json::from_str(text[1]).map_err(|_| FunctionError::InvalidArgument {
            function,
            message: "schema descriptor is invalid",
        })?;
    let path: Vec<String> =
        serde_json::from_str(text[2]).map_err(|_| FunctionError::InvalidArgument {
            function,
            message: "array path descriptor is invalid",
        })?;
    let mut current =
        format_json::from_str(text[0], &schema).map_err(|_| FunctionError::InvalidArgument {
            function,
            message: "input does not match the JSON schema",
        })?;
    for segment in path {
        current = current
            .field(&segment)
            .cloned()
            .ok_or(FunctionError::InvalidArgument {
                function,
                message: "array path does not resolve to an array",
            })?;
    }
    match current {
        Instance::Repeated(items) => Ok(items),
        _ => Err(FunctionError::InvalidArgument {
            function,
            message: "array path does not resolve to an array",
        }),
    }
}

pub(super) fn object_exists(args: &[Value]) -> Result<Value, FunctionError> {
    let [input, schema, path] = args else {
        return Err(FunctionError::ArityMismatch {
            function: OBJECT_FUNCTION,
            expected: 3,
            got: args.len(),
        });
    };
    let Value::String(schema) = schema else {
        return Err(FunctionError::TypeMismatch {
            function: OBJECT_FUNCTION,
            got: schema.type_name(),
        });
    };
    let Value::String(path) = path else {
        return Err(FunctionError::TypeMismatch {
            function: OBJECT_FUNCTION,
            got: path.type_name(),
        });
    };
    let Value::String(input) = input else {
        if matches!(input, Value::Null | Value::JsonNull(_)) {
            return Ok(Value::Bool(false));
        }
        return Err(FunctionError::TypeMismatch {
            function: OBJECT_FUNCTION,
            got: input.type_name(),
        });
    };
    let schema: SchemaNode =
        serde_json::from_str(schema).map_err(|_| FunctionError::InvalidArgument {
            function: OBJECT_FUNCTION,
            message: "schema descriptor is invalid",
        })?;
    let path: Vec<String> =
        serde_json::from_str(path).map_err(|_| FunctionError::InvalidArgument {
            function: OBJECT_FUNCTION,
            message: "field path descriptor is invalid",
        })?;
    let object_path_error = || FunctionError::InvalidArgument {
        function: OBJECT_FUNCTION,
        message: "field path does not resolve to an ordinary object",
    };
    let mut selected = &schema;
    for segment in &path {
        if !ordinary_object(selected) {
            return Err(object_path_error());
        }
        selected = selected.child(segment).ok_or_else(object_path_error)?;
    }
    if !ordinary_object(selected) {
        return Err(object_path_error());
    }
    let parsed =
        format_json::from_str(input, &schema).map_err(|_| FunctionError::InvalidArgument {
            function: OBJECT_FUNCTION,
            message: "input does not match the JSON schema",
        })?;
    if matches!(parsed, Instance::Repeated(_) | Instance::DocumentSet(_)) {
        return Err(FunctionError::InvalidArgument {
            function: OBJECT_FUNCTION,
            message: "input does not match the JSON schema",
        });
    }
    let mut current = &parsed;
    for segment in &path {
        let Some(child) = current.field(segment) else {
            return Ok(Value::Bool(false));
        };
        current = child;
    }
    match current {
        Instance::Group(_) => Ok(Value::Bool(true)),
        Instance::Scalar(Value::Null | Value::JsonNull(_)) => Ok(Value::Bool(false)),
        _ => Err(object_path_error()),
    }
}

fn ordinary_object(schema: &SchemaNode) -> bool {
    matches!(schema.kind, SchemaKind::Group { .. })
        && !schema.repeating
        && !schema.json_array_wrapper
        && !schema.json_any
        && schema.dynamic_fields().is_none()
        && schema.alternatives().is_empty()
}

pub(super) fn parse_field(args: &[Value]) -> Result<Value, FunctionError> {
    let [input, schema, path] = args else {
        return Err(FunctionError::ArityMismatch {
            function: PARSE_FUNCTION,
            expected: 3,
            got: args.len(),
        });
    };
    let Value::String(schema) = schema else {
        return Err(FunctionError::TypeMismatch {
            function: PARSE_FUNCTION,
            got: schema.type_name(),
        });
    };
    let Value::String(path) = path else {
        return Err(FunctionError::TypeMismatch {
            function: PARSE_FUNCTION,
            got: path.type_name(),
        });
    };
    let Value::String(input) = input else {
        if matches!(input, Value::Null | Value::JsonNull(_)) {
            return Ok(Value::Null);
        }
        return Err(FunctionError::TypeMismatch {
            function: PARSE_FUNCTION,
            got: input.type_name(),
        });
    };
    let schema: SchemaNode =
        serde_json::from_str(schema).map_err(|_| FunctionError::InvalidArgument {
            function: PARSE_FUNCTION,
            message: "schema descriptor is invalid",
        })?;
    let path: Vec<String> =
        serde_json::from_str(path).map_err(|_| FunctionError::InvalidArgument {
            function: PARSE_FUNCTION,
            message: "field path descriptor is invalid",
        })?;
    let parsed =
        format_json::from_str(input, &schema).map_err(|_| FunctionError::InvalidArgument {
            function: PARSE_FUNCTION,
            message: "input does not match the JSON schema",
        })?;
    scalar_at(&parsed, &path)
        .cloned()
        .ok_or(FunctionError::InvalidArgument {
            function: PARSE_FUNCTION,
            message: "field path does not resolve to a scalar",
        })
}

fn scalar_at<'a>(instance: &'a Instance, path: &[String]) -> Option<&'a Value> {
    let mut current = instance;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}

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
        ("string" | "integer" | "number" | "boolean", Value::JsonNull(_)) => {
            Ok(serde_json::Value::Null)
        }
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
    use ir::{ScalarType, SchemaNode, Value};

    use crate::FunctionError;

    use super::{array_field, array_length, object_exists, parse_field, serialize_object};

    #[test]
    fn object_presence_keeps_empty_objects_and_omits_missing_or_null_objects() {
        let schema = SchemaNode::group(
            "Payload",
            vec![
                SchemaNode::group(
                    "Header",
                    vec![SchemaNode::scalar("Label", ScalarType::String)],
                )
                .nullable_container()
                .unwrap(),
                SchemaNode::scalar("Unused", ScalarType::Int),
            ],
        );
        let descriptor = serde_json::to_string(&schema).unwrap();
        for (input, path, present) in [
            (r#"{}"#, "[]", true),
            (r#"{"Header":{}}"#, r#"["Header"]"#, true),
            (r#"{}"#, r#"["Header"]"#, false),
            (r#"{"Header":null}"#, r#"["Header"]"#, false),
        ] {
            assert_eq!(
                object_exists(&[
                    Value::String(input.into()),
                    Value::String(descriptor.clone()),
                    Value::String(path.into()),
                ]),
                Ok(Value::Bool(present)),
                "{input}: {path}"
            );
        }
        assert_eq!(
            object_exists(&[
                Value::String(r#"{"Header":{},"Unused":"wrong"}"#.into()),
                Value::String(descriptor),
                Value::String(r#"["Header"]"#.into()),
            ]),
            Err(FunctionError::InvalidArgument {
                function: "json_parse_object_exists",
                message: "input does not match the JSON schema",
            })
        );
    }

    #[test]
    fn object_presence_refuses_scalar_and_array_paths_and_preserves_null_short_circuit() {
        let descriptor = serde_json::to_string(&SchemaNode::group(
            "Payload",
            vec![
                SchemaNode::scalar("Label", ScalarType::String),
                SchemaNode::group("Rows", vec![]).repeating(),
            ],
        ))
        .unwrap();
        for path in [r#"["Label"]"#, r#"["Rows"]"#, r#"["Missing"]"#] {
            assert_eq!(
                object_exists(&[
                    Value::String(r#"{"Label":"ok","Rows":[]}"#.into()),
                    Value::String(descriptor.clone()),
                    Value::String(path.into()),
                ]),
                Err(FunctionError::InvalidArgument {
                    function: "json_parse_object_exists",
                    message: "field path does not resolve to an ordinary object",
                })
            );
        }
        for input in [Value::Null, Value::json_null()] {
            assert_eq!(
                object_exists(&[
                    input,
                    Value::String("bad schema".into()),
                    Value::String("bad path".into())
                ]),
                Ok(Value::Bool(false))
            );
        }
        for (schema, path, message) in [
            ("bad schema", "bad path", "schema descriptor is invalid"),
            (
                descriptor.as_str(),
                "bad path",
                "field path descriptor is invalid",
            ),
            (
                descriptor.as_str(),
                "[]",
                "input does not match the JSON schema",
            ),
        ] {
            assert_eq!(
                object_exists(&[
                    Value::String("bad input".into()),
                    Value::String(schema.into()),
                    Value::String(path.into()),
                ]),
                Err(FunctionError::InvalidArgument {
                    function: "json_parse_object_exists",
                    message
                })
            );
        }
    }

    #[test]
    fn array_projection_validates_descriptors_indices_and_complete_input() {
        let schema = SchemaNode::group(
            "Payload",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Qty", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        );
        let args = vec![
            Value::String(r#"{"Rows":[{"Code":"A","Qty":2},{"Code":"B","Qty":3}]}"#.into()),
            Value::String(serde_json::to_string(&schema).unwrap()),
            Value::String(r#"["Rows"]"#.into()),
            Value::String(r#"["Qty"]"#.into()),
            Value::Int(2),
        ];
        assert_eq!(array_length(&args[..3]), Ok(Value::Int(2)));
        assert_eq!(array_field(&args), Ok(Value::Int(3)));
        for index in [i64::MIN, -1, 0, 3, i64::MAX] {
            let mut invalid = args.clone();
            invalid[4] = Value::Int(index);
            assert_eq!(
                array_field(&invalid),
                Err(FunctionError::InvalidArgument {
                    function: "json_parse_array_field",
                    message: "array index is out of range",
                })
            );
        }
        for (position, value, message) in [
            (0, "not JSON", "input does not match the JSON schema"),
            (
                0,
                r#"{"Rows":[{"Qty":2},{"Qty":"bad"}]}"#,
                "input does not match the JSON schema",
            ),
            (1, "bad schema", "schema descriptor is invalid"),
            (2, "bad path", "array path descriptor is invalid"),
            (2, "[]", "array path does not resolve to an array"),
            (3, "bad path", "field path descriptor is invalid"),
            (3, "[]", "field path does not resolve to a scalar"),
        ] {
            let mut invalid = args.clone();
            invalid[position] = Value::String(value.into());
            assert_eq!(
                array_field(&invalid),
                Err(FunctionError::InvalidArgument {
                    function: "json_parse_array_field",
                    message,
                })
            );
        }
        for position in 0..5 {
            let mut invalid = args.clone();
            invalid[position] = Value::Null;
            assert!(matches!(
                array_field(&invalid),
                Err(FunctionError::TypeMismatch {
                    function: "json_parse_array_field",
                    ..
                })
            ));
        }
    }

    #[test]
    fn parses_a_nested_field_with_schema_types() {
        let schema = SchemaNode::group(
            "payload",
            vec![SchemaNode::group(
                "Leaves",
                vec![SchemaNode::scalar("Total", ScalarType::Float)],
            )],
        );
        let result = parse_field(&[
            Value::String(r#"{"Leaves":{"Total":3.5}}"#.into()),
            Value::String(serde_json::to_string(&schema).unwrap()),
            Value::String(r#"["Leaves","Total"]"#.into()),
        ]);
        assert_eq!(result, Ok(Value::Float(3.5)));
    }

    #[test]
    fn rejects_json_that_does_not_match_the_schema() {
        let schema = SchemaNode::scalar("count", ScalarType::Int);
        let result = parse_field(&[
            Value::String(r#""not an integer""#.into()),
            Value::String(serde_json::to_string(&schema).unwrap()),
            Value::String("[]".into()),
        ]);
        assert!(matches!(result, Err(FunctionError::InvalidArgument { .. })));
    }

    #[test]
    fn parse_field_accepts_raw_function_inputs_beyond_generated_host_limits() {
        const HOST_SCHEMA_BYTES: usize = 1024 * 1024;
        const HOST_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

        let mut large_schema = SchemaNode::scalar("", ScalarType::Int);
        let overhead = serde_json::to_string(&large_schema).unwrap().len();
        large_schema.name = "N".repeat(HOST_SCHEMA_BYTES + 1 - overhead);
        let descriptor = serde_json::to_string(&large_schema).unwrap();
        assert_eq!(descriptor.len(), HOST_SCHEMA_BYTES + 1);
        assert_eq!(
            parse_field(&[
                Value::String("42".into()),
                Value::String(descriptor),
                Value::String("[]".into()),
            ]),
            Ok(Value::Int(42)),
        );

        let ordinary_schema =
            serde_json::to_string(&SchemaNode::scalar("Value", ScalarType::Int)).unwrap();
        let large_input = format!("42{}", " ".repeat(HOST_DOCUMENT_BYTES));
        assert_eq!(large_input.len(), HOST_DOCUMENT_BYTES + 2);
        assert_eq!(
            parse_field(&[
                Value::String(large_input),
                Value::String(ordinary_schema),
                Value::String("[]".into()),
            ]),
            Ok(Value::Int(42)),
        );

        let mut row = SchemaNode::group("Rows", vec![SchemaNode::scalar("v", ScalarType::Int)]);
        row.repeating = true;
        let schema = SchemaNode::group(
            "Root",
            vec![row, SchemaNode::scalar("Count", ScalarType::Int)],
        );
        let descriptor = serde_json::to_string(&schema).unwrap();
        let mut input = String::with_capacity(8_000_032);
        input.push_str(r#"{"Rows":[{"v":1}"#);
        for _ in 1..=1_000_000 {
            input.push_str(r#",{"v":1}"#);
        }
        input.push_str(r#"],"Count":7}"#);
        assert_eq!(input.len(), 8_000_028);
        assert_eq!(
            parse_field(&[
                Value::String(input),
                Value::String(descriptor),
                Value::String(r#"["Count"]"#.into()),
            ]),
            Ok(Value::Int(7)),
        );
    }

    #[test]
    fn parse_field_preserves_schema_path_input_error_order_and_null_short_circuit() {
        let valid = serde_json::to_string(&SchemaNode::scalar("Value", ScalarType::Int)).unwrap();
        let error = |input: Value, schema: &str, path: &str| {
            parse_field(&[
                input,
                Value::String(schema.into()),
                Value::String(path.into()),
            ])
        };
        assert_eq!(
            error(
                Value::String("not JSON".into()),
                "not a schema",
                "not a path"
            ),
            Err(FunctionError::InvalidArgument {
                function: "json_parse_field",
                message: "schema descriptor is invalid",
            }),
        );
        assert_eq!(
            error(Value::String("not JSON".into()), &valid, "not a path"),
            Err(FunctionError::InvalidArgument {
                function: "json_parse_field",
                message: "field path descriptor is invalid",
            }),
        );
        assert_eq!(
            error(Value::String("not JSON".into()), &valid, "[]"),
            Err(FunctionError::InvalidArgument {
                function: "json_parse_field",
                message: "input does not match the JSON schema",
            }),
        );
        for input in [Value::Null, Value::json_null()] {
            assert_eq!(error(input, "not a schema", "not a path"), Ok(Value::Null));
        }
    }

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
