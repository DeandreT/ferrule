//! A scalar array's container null and item null are independent (#924):
//! the array may be `null` while each item keeps its own value domain.

use format_json::{JsonFormatError, PreparedSchema, from_json5_str, from_str, json_schema};
use ir::{Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};
use serde_json::{Value as Json, json};

fn schema_document(outer: bool, item: bool) -> Json {
    json!({"title":"Items", "type": if outer { json!(["array", "null"]) } else { json!("array") },
        "items":{"type": if item { json!(["integer", "null"]) } else { json!("integer") }}})
}

fn json_null() -> Instance {
    Instance::Scalar(Value::json_null())
}

type Expected = Result<Instance, (&'static str, &'static str)>;

fn cases(outer: bool, item: bool) -> Vec<(&'static str, Expected)> {
    let int = |value| Instance::Scalar(Value::Int(value));
    vec![
        (
            "null",
            if outer {
                Ok(json_null())
            } else {
                Err(("array", "null"))
            },
        ),
        ("[]", Ok(Instance::Repeated(vec![]))),
        ("[1]", Ok(Instance::Repeated(vec![int(1)]))),
        (
            "[null]",
            if item {
                Ok(Instance::Repeated(vec![json_null()]))
            } else {
                Err(("integer", "null"))
            },
        ),
        (
            "[1,null]",
            if item {
                Ok(Instance::Repeated(vec![int(1), json_null()]))
            } else {
                Err(("integer", "null"))
            },
        ),
        ("{}", Err(("array", "object"))),
        ("[true]", Err(("integer", "bool"))),
    ]
}

fn assert_shape(actual: Result<Instance, JsonFormatError>, expected: (&str, &str), context: &str) {
    match actual {
        Err(JsonFormatError::Shape {
            name,
            expected: actual_expected,
            got,
        }) => assert_eq!(
            (name.as_str(), actual_expected, got),
            ("Items", expected.0, expected.1),
            "{context}"
        ),
        other => panic!("{context}: expected {expected:?}; actual {other:?}"),
    }
}

/// Every reader and both writers of one schema, which must agree.
fn assert_cases(schema: &SchemaNode, outer: bool, item: bool) {
    let prepared = PreparedSchema::new(schema.clone());
    for (input, expected) in cases(outer, item) {
        let context = format!("{outer}/{item}/{input}");
        let actual = [
            from_str(input, schema),
            from_json5_str(input, schema),
            prepared.from_str(input),
        ];
        match expected {
            Ok(expected) => {
                for actual in actual {
                    assert_eq!(actual.unwrap(), expected, "{context}");
                }
                let literal: Json = serde_json::from_str(input).unwrap();
                assert_eq!(
                    format_json::to_value(schema, &expected).unwrap(),
                    literal,
                    "{context}"
                );
                assert_eq!(prepared.to_value(&expected).unwrap(), literal, "{context}");
                assert_eq!(
                    format_json::to_string(schema, &expected).unwrap(),
                    serde_json::to_string_pretty(&literal).unwrap() + "\n",
                    "{context}"
                );
            }
            Err(expected) => {
                for actual in actual {
                    assert_shape(actual, expected, &context);
                }
            }
        }
    }
}

#[test]
fn scalar_array_nullability_four_combinations_keep_independent_values() {
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let schema = json_schema::import_str(&schema_document(outer, item).to_string()).unwrap();
        assert_eq!(
            (schema.container_nullable, schema.nullable),
            (outer, item),
            "{outer}/{item}"
        );
        assert_eq!(schema.json_object_item_nullable, None);
        assert_eq!(
            json_schema::import_str(&json_schema::export(&schema).unwrap()).unwrap(),
            schema
        );
        assert_cases(&schema, outer, item);
    }
}

#[test]
fn writers_refuse_a_null_scalar_item_outside_its_value_domain() {
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let schema = json_schema::import_str(&schema_document(outer, item).to_string()).unwrap();
        let int = Instance::Scalar(Value::Int(1));
        for instance in [
            Instance::Repeated(vec![json_null()]),
            Instance::Repeated(vec![int, json_null()]),
        ] {
            let owned = format_json::to_value(&schema, &instance);
            let borrowed = format_json::to_string(&schema, &instance);
            if item {
                assert!(owned.is_ok() && borrowed.is_ok(), "{outer}/{item}");
                continue;
            }
            for error in [owned.unwrap_err(), borrowed.unwrap_err()] {
                assert!(
                    matches!(
                        &error,
                        JsonFormatError::Shape { name, expected: "integer", got: "json null" }
                            if name == "Items"
                    ),
                    "{outer}/{item}: {error:?}"
                );
            }
        }
        let whole = format_json::to_value(&schema, &json_null());
        assert_eq!(whole.is_ok(), outer, "{outer}/{item}: {whole:?}");
    }
}

#[test]
fn nested_scalar_unions_and_saved_descriptors_keep_the_item_domain() {
    // A property of an object, where a missing nullable array stays absent.
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let document = json!({"title":"Envelope", "type":"object",
            "properties":{"Items":schema_document(outer, item)}, "additionalProperties":false});
        let nested = json_schema::import_str(&document.to_string()).unwrap();
        let missing = if outer {
            Instance::Scalar(Value::Null)
        } else {
            Instance::Repeated(vec![])
        };
        assert_eq!(
            from_str("{}", &nested).unwrap(),
            Instance::Group(vec![("Items".into(), missing)].into())
        );
        for (input, expected) in cases(outer, item) {
            let actual = from_str(&format!("{{\"Items\":{input}}}"), &nested);
            match expected {
                Ok(expected) => assert_eq!(
                    actual.unwrap(),
                    Instance::Group(vec![("Items".into(), expected)].into())
                ),
                Err(expected) => assert_shape(actual, expected, input),
            }
        }
    }

    // Scalar-union items use the same value layer.
    let types = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap();
    let union = SchemaNode::scalar_union("Items", types)
        .repeating()
        .nullable_container()
        .unwrap();
    assert_eq!(from_str("null", &union).unwrap(), json_null());
    assert_eq!(
        from_str(r#"[1,"a"]"#, &union).unwrap(),
        Instance::Repeated(vec![
            Instance::Scalar(Value::Int(1)),
            Instance::Scalar(Value::String("a".into())),
        ])
    );
    assert!(matches!(
        from_str("[null]", &union),
        Err(JsonFormatError::Shape { got: "null", .. })
    ));
    let mut nullable_union = union.nullable().unwrap();
    nullable_union.container_nullable = true;
    assert_eq!(
        from_str("[null]", &nullable_union).unwrap(),
        Instance::Repeated(vec![json_null()])
    );

    // A saved descriptor already separates the array and item flags.
    let saved = r#"{"name":"Items","repeating":true,"container_nullable":true,"kind":{"kind":"scalar","ty":"int"}}"#;
    let saved: SchemaNode = serde_json::from_str(saved).unwrap();
    assert_cases(&saved, true, false);
}
