use format_json::{JsonFormatError, from_str, json_schema, to_string, to_value};
use ir::{Instance, ScalarType, SchemaNode, Value};
use serde_json::{Value as Json, json};

fn schema_document(outer: bool, item: bool) -> Json {
    json!({"title":"Items", "type": if outer { json!(["array", "null"]) } else { json!("array") },
        "items":{"type": if item { json!(["object", "null"]) } else { json!("object") },
            "properties":{}, "additionalProperties":false}})
}

fn empty_object() -> Instance {
    Instance::Group(Vec::new().into())
}

type Expected = Result<Instance, (&'static str, &'static str)>;
type Case = (&'static str, Expected);

fn cases(outer: bool, item: bool) -> Vec<Case> {
    vec![
        (
            "null",
            if outer {
                Ok(Instance::Scalar(Value::json_null()))
            } else {
                Err(("array", "null"))
            },
        ),
        ("[]", Ok(Instance::Repeated(vec![]))),
        ("{}", Err(("array", "object"))),
        (
            "[null]",
            if item {
                Ok(Instance::Repeated(vec![Instance::Scalar(
                    Value::json_null(),
                )]))
            } else {
                Err(("object", "null"))
            },
        ),
        ("[{}]", Ok(Instance::Repeated(vec![empty_object()]))),
        (
            "[null,{}]",
            if item {
                Ok(Instance::Repeated(vec![
                    Instance::Scalar(Value::json_null()),
                    empty_object(),
                ]))
            } else {
                Err(("object", "null"))
            },
        ),
        ("[false]", Err(("object", "bool"))),
        ("[1]", Err(("object", "number"))),
        ("[\"null\"]", Err(("object", "string"))),
    ]
}

#[test]
fn object_array_nullability_four_combinations_have_complete_independent_values() {
    let originals = [(false, false), (false, true), (true, false), (true, true)]
        .into_iter()
        .map(|(outer, item)| {
            let document = schema_document(outer, item);
            let schema = json_schema::import_str(&document.to_string()).unwrap();
            let results = cases(outer, item)
                .into_iter()
                .map(|(input, expected)| {
                    let actual = from_str(input, &schema);
                    (input, expected, actual)
                })
                .collect::<Vec<_>>();
            (outer, item, schema, results)
        })
        .collect::<Vec<_>>();
    eprintln!("object-item-nullability originals: {originals:#?}");
    for (_, _, schema, results) in originals {
        for (input, expected, actual) in results {
            match (expected, actual) {
                (Ok(expected), Ok(actual)) => {
                    assert_eq!(actual, expected, "{input}");
                    let expected_json: Json = serde_json::from_str(input).unwrap();
                    assert_eq!(to_value(&schema, &expected).unwrap(), expected_json);
                    let borrowed = to_string(&schema, &expected).unwrap();
                    assert_eq!(
                        borrowed,
                        serde_json::to_string_pretty(&expected_json).unwrap() + "\n"
                    );
                }
                (
                    Err((expected, got)),
                    Err(JsonFormatError::Shape {
                        name,
                        expected: actual_expected,
                        got: actual_got,
                    }),
                ) => {
                    assert_eq!(
                        (name.as_str(), actual_expected, actual_got),
                        ("Items", expected, got)
                    );
                }
                (expected, actual) => panic!("{input}: expected {expected:?}; actual {actual:?}"),
            }
        }
    }
}

#[test]
fn object_item_schema_codec_export_and_nested_presence_remain_exact() {
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let document = schema_document(outer, item);
        let schema = json_schema::import_str(&document.to_string()).unwrap();
        assert_eq!(schema.container_nullable, outer);
        assert_eq!(
            schema.json_object_item_nullable,
            (outer || item).then_some(item)
        );
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&schema).unwrap()).unwrap(),
            schema
        );
        assert_eq!(
            json_schema::import_str(&json_schema::export(&schema).unwrap()).unwrap(),
            schema
        );
        let nested_document = json!({"title":"Envelope", "type":"object", "properties":{"Items":document}, "additionalProperties":false});
        let nested = json_schema::import_str(&nested_document.to_string()).unwrap();
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
            let source = format!("{{\"Items\":{input}}}");
            match expected {
                Ok(expected) => assert_eq!(
                    from_str(&source, &nested).unwrap(),
                    Instance::Group(vec![("Items".into(), expected)].into())
                ),
                Err((expected, got)) => assert!(
                    matches!(from_str(&source, &nested), Err(JsonFormatError::Shape { name, expected: actual_expected, got: actual_got }) if name == "Items" && actual_expected == expected && actual_got == got)
                ),
            }
        }
        let mut required_document = nested_document;
        required_document["required"] = json!(["Items"]);
        let required = json_schema::import_str(&required_document.to_string()).unwrap();
        assert!(
            matches!(from_str("{}", &required), Err(JsonFormatError::MissingRequiredProperty { object, property }) if object == "Envelope" && property == "Items")
        );
    }
}

#[test]
fn nested_array_wrappers_keep_object_item_and_inner_container_flags() {
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let document =
            json!({"title":"Items", "type":"array", "items":schema_document(outer, item)});
        let schema = json_schema::import_str(&document.to_string()).unwrap();
        assert!(schema.json_array_wrapper);
        assert_eq!(schema.json_object_item_nullable, None);
        let inner = schema.json_array_wrapper_item().unwrap();
        assert_eq!(inner.container_nullable, outer);
        assert_eq!(
            inner.json_object_item_nullable,
            (outer || item).then_some(item)
        );
        assert_eq!(
            json_schema::import_str(&json_schema::export(&schema).unwrap()).unwrap(),
            schema
        );
        for (input, expected) in cases(outer, item) {
            let input = format!("[{input}]");
            let original = from_str(&input, &schema);
            eprintln!("nested object-item original {outer}/{item}/{input}: {original:?}");
            match expected {
                Ok(expected) => {
                    let expected = Instance::Repeated(vec![Instance::Group(
                        vec![("item".into(), expected)].into(),
                    )]);
                    assert_eq!(original.unwrap(), expected);
                    let literal: Json = serde_json::from_str(&input).unwrap();
                    assert_eq!(to_value(&schema, &expected).unwrap(), literal);
                    assert_eq!(
                        to_string(&schema, &expected).unwrap(),
                        serde_json::to_string_pretty(&literal).unwrap() + "\n"
                    );
                }
                Err((expected, got)) => assert!(
                    matches!(original, Err(JsonFormatError::Shape { name, expected: actual_expected, got: actual_got }) if name == "item" && actual_expected == expected && actual_got == got)
                ),
            }
        }
    }
}

#[test]
fn object_item_legacy_descriptors_and_invalid_metadata_keep_their_gates() {
    let old = r#"{"name":"Items","repeating":true,"container_nullable":true,"kind":{"kind":"group","children":[]}}"#;
    let legacy: SchemaNode = serde_json::from_str(old).unwrap();
    assert_eq!(legacy.json_object_item_nullable, None);
    assert_eq!(serde_json::to_string(&legacy).unwrap(), old);
    assert_eq!(
        from_str("[null]", &legacy).unwrap(),
        Instance::Repeated(vec![Instance::Scalar(Value::json_null())])
    );
    assert_eq!(
        to_string(
            &legacy,
            &Instance::Repeated(vec![Instance::Scalar(Value::json_null())])
        )
        .unwrap(),
        "[\n  null\n]\n"
    );
    let legacy_export = json_schema::export(&legacy).unwrap();
    let reimported = json_schema::import_str(&legacy_export).unwrap();
    assert!(reimported.container_nullable);
    assert_eq!(reimported.json_object_item_nullable, Some(true));
    for (input, expected) in cases(true, true) {
        let legacy_actual = from_str(input, &legacy);
        let reimported_actual = from_str(input, &reimported);
        eprintln!(
            "legacy export/reimport original {input}: {legacy_actual:?}/{reimported_actual:?}"
        );
        match expected {
            Ok(expected) => {
                assert_eq!(legacy_actual.unwrap(), expected);
                assert_eq!(reimported_actual.unwrap(), expected);
                assert_eq!(
                    to_value(&legacy, &expected).unwrap(),
                    to_value(&reimported, &expected).unwrap()
                );
                assert_eq!(
                    to_string(&legacy, &expected).unwrap(),
                    to_string(&reimported, &expected).unwrap()
                );
            }
            Err((expected, got)) => {
                for original in [legacy_actual, reimported_actual] {
                    assert!(
                        matches!(original, Err(JsonFormatError::Shape { name, expected: actual_expected, got: actual_got }) if name == "Items" && actual_expected == expected && actual_got == got)
                    );
                }
            }
        }
    }
    let ordinary = json_schema::import_str(&schema_document(false, false).to_string()).unwrap();
    assert!(
        !serde_json::to_string(&ordinary)
            .unwrap()
            .contains("json_object_item_nullable")
    );
    for value in [json!(null), json!(0), json!("false"), json!([])] {
        let mut descriptor: Json = serde_json::from_str(old).unwrap();
        descriptor["json_object_item_nullable"] = value;
        assert!(serde_json::from_value::<SchemaNode>(descriptor).is_err());
    }
    let mut malformed = SchemaNode::group("Items", vec![]);
    malformed.json_object_item_nullable = Some(false);
    assert!(
        matches!(from_str("{}", &malformed), Err(JsonFormatError::InvalidObjectItemNullabilityMetadata { name }) if name == "Items")
    );
    assert!(
        matches!(json_schema::export(&malformed), Err(JsonFormatError::InvalidObjectItemNullabilityMetadata { name }) if name == "Items")
    );
    let mut scalar = SchemaNode::scalar("Items", ScalarType::Int).repeating();
    scalar.json_object_item_nullable = Some(true);
    assert!(!scalar.metadata_is_valid());
    let mut wrapper = SchemaNode::group(
        "Items",
        vec![SchemaNode::scalar("item", ScalarType::Int).repeating()],
    )
    .repeating()
    .with_json_array_wrapper()
    .unwrap();
    wrapper.json_object_item_nullable = Some(false);
    assert!(!wrapper.metadata_is_valid());
    assert!(!wrapper.json_array_wrapper_is_valid());
    assert!(wrapper.json_array_wrapper_item().is_none());
    for nullable in [false, true] {
        let mut invalid_wrapper = SchemaNode::group(
            "Items",
            vec![SchemaNode::scalar("item", ScalarType::Int).repeating()],
        )
        .repeating();
        invalid_wrapper.json_object_item_nullable = Some(nullable);
        assert!(invalid_wrapper.with_json_array_wrapper().is_none());
    }
}

#[test]
fn positional_object_arrays_keep_both_nullability_domains() {
    let mut originals = Vec::new();
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        for modern in [false, true] {
            let mut document = schema_document(outer, item);
            let item_schema = document["items"].clone();
            if modern {
                document["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
                document["prefixItems"] = json!([item_schema.clone(), item_schema]);
                document["items"] = json!(false);
            } else {
                document["$schema"] = json!("http://json-schema.org/draft-07/schema#");
                document["items"] = json!([item_schema.clone(), item_schema]);
                document["additionalItems"] = json!(false);
            }
            let schema = json_schema::import_str(&document.to_string()).unwrap();
            let controls = cases(outer, item)
                .into_iter()
                .map(|(input, expected)| (input, expected, from_str(input, &schema)))
                .collect::<Vec<_>>();
            originals.push((outer, item, modern, schema, controls));
        }
    }
    eprintln!("positional object-item originals: {originals:#?}");
    for (outer, item, modern, schema, controls) in originals {
        assert_eq!(schema.container_nullable, outer, "{modern}");
        assert_eq!(
            schema.json_object_item_nullable,
            (outer || item).then_some(item),
            "{modern}"
        );
        assert_eq!(
            json_schema::import_str(&json_schema::export(&schema).unwrap()).unwrap(),
            schema
        );
        for (input, expected, actual) in controls {
            match expected {
                Ok(expected) => {
                    assert_eq!(actual.unwrap(), expected);
                    let literal: Json = serde_json::from_str(input).unwrap();
                    assert_eq!(to_value(&schema, &expected).unwrap(), literal);
                    assert_eq!(
                        to_string(&schema, &expected).unwrap(),
                        serde_json::to_string_pretty(&literal).unwrap() + "\n"
                    );
                }
                Err((expected, got)) => assert!(
                    matches!(actual, Err(JsonFormatError::Shape { name, expected: actual_expected, got: actual_got }) if name == "Items" && actual_expected == expected && actual_got == got)
                ),
            }
        }
    }
}

#[test]
fn object_item_constraints_and_all_of_keep_separate_domains() {
    let schema = json_schema::import_str(r#"{"title":"Items","type":["array","null"],"minItems":1,"uniqueItems":true,"items":{"type":["object","null"],"properties":{"id":{"type":"integer"}},"required":["id"],"additionalProperties":false}}"#).unwrap();
    assert_eq!(
        from_str("null", &schema).unwrap(),
        Instance::Scalar(Value::json_null())
    );
    assert_eq!(
        from_str("[null]", &schema).unwrap(),
        Instance::Repeated(vec![Instance::Scalar(Value::json_null())])
    );
    assert!(matches!(
        from_str("[]", &schema),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
    assert!(
        matches!(from_str("[{}]", &schema), Err(JsonFormatError::MissingRequiredProperty { object, property }) if object == "Items" && property == "id")
    );
    assert!(matches!(
        from_str("[null,null]", &schema),
        Err(JsonFormatError::UniqueItemsMismatch {
            first_index: 1,
            duplicate_index: 2,
            ..
        })
    ));
    let intersection = json!({"title":"Items", "allOf":[schema_document(true, true), schema_document(true, false)]});
    let intersection = json_schema::import_str(&intersection.to_string()).unwrap();
    assert!(intersection.container_nullable);
    assert_eq!(intersection.json_object_item_nullable, Some(false));
    assert!(matches!(
        from_str("[null]", &intersection),
        Err(JsonFormatError::Shape {
            expected: "object",
            got: "null",
            ..
        })
    ));
    let invalid_items = Instance::Repeated(vec![Instance::Scalar(Value::json_null())]);
    assert!(matches!(
        to_value(&intersection, &invalid_items),
        Err(JsonFormatError::Shape {
            expected: "object",
            got: "json null",
            ..
        })
    ));
    assert!(matches!(
        to_string(&intersection, &invalid_items),
        Err(JsonFormatError::Shape {
            expected: "object",
            got: "json null",
            ..
        })
    ));
}
