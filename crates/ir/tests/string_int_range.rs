use ir::{IntegerRange, NumericRange, ScalarType, ScalarTypeSet, SchemaNode};

fn union(types: &[ScalarType]) -> SchemaNode {
    SchemaNode::scalar_union(
        "value",
        ScalarTypeSet::new(types.iter().copied()).expect("literal union"),
    )
}

#[test]
fn integer_interval_metadata_admits_only_exact_string_int() {
    let range = NumericRange::Integer(IntegerRange::new(Some(2), Some(5)).expect("literal range"));
    for (types, accepted) in [
        (vec![ScalarType::String, ScalarType::Int], true),
        (vec![ScalarType::Int, ScalarType::String], true),
        (vec![ScalarType::String, ScalarType::Float], false),
        (
            vec![ScalarType::String, ScalarType::Int, ScalarType::Float],
            false,
        ),
        (vec![ScalarType::Int, ScalarType::Bool], false),
    ] {
        let mut schema = union(&types);
        schema.numeric_range = Some(range);
        let original = (
            schema.is_string_int_union(),
            schema.numeric_range_is_valid(),
            schema.metadata_is_valid(),
        );
        eprintln!("complete schema={schema:#?}; original={original:#?}");
        assert_eq!(original, (accepted, accepted, accepted));
        let original = schema.clone().with_numeric_range(range);
        eprintln!("complete builder original={original:#?}");
        assert_eq!(original.is_some(), accepted);
    }
    let mut schema = union(&[ScalarType::String, ScalarType::Int]);
    schema.numeric_range = Some(range);
    schema.fixed = Some("3".into());
    assert!(!schema.numeric_range_is_valid());
    assert!(!schema.metadata_is_valid());
    schema.fixed = None;
    schema.json_any = true;
    assert!(!schema.numeric_range_is_valid());
    assert!(!schema.metadata_is_valid());
}

#[test]
fn old_serde_and_exact_sparse_integer_endpoints_keep_their_representation() {
    let old = r#"{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}"#;
    let original = serde_json::from_str::<SchemaNode>(old);
    eprintln!("complete old decode original={original:#?}");
    let schema = original.expect("legacy unconstrained union");
    assert!(schema.numeric_range.is_none());
    assert_eq!(serde_json::to_string(&schema).expect("old encode"), old);
    let exact = r#"{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9007199254740993,"maximum":9223372036854775807}},"kind":{"kind":"scalar_union","types":["string","int"]}}"#;
    let original = serde_json::from_str::<SchemaNode>(exact);
    eprintln!("complete exact decode original={original:#?}");
    let schema = original.expect("exact integer union descriptor");
    assert_eq!(serde_json::to_string(&schema).expect("exact encode"), exact);
    assert_eq!(
        schema.numeric_range,
        IntegerRange::new(Some(9007199254740993), Some(i64::MAX)).map(NumericRange::Integer)
    );
    for malformed in [
        r#"{"name":"value","numeric_range":{"kind":"integer","bounds":{}},"kind":{"kind":"scalar_union","types":["string","int"]}}"#,
        r#"{"name":"value","numeric_range":{"kind":"integer","bounds":{"minimum":5,"maximum":2}},"kind":{"kind":"scalar_union","types":["string","int"]}}"#,
        r#"{"name":"value","numeric_range":{"kind":"number","bounds":{"minimum":{"value":2.0}}},"kind":{"kind":"scalar_union","types":["string","int"]}}"#,
    ] {
        let original = serde_json::from_str::<SchemaNode>(malformed);
        eprintln!("complete malformed original={original:#?}; input={malformed}");
        assert!(original.is_err());
    }
}

#[test]
fn range_does_not_change_nullable_item_and_container_metadata() {
    let mut schema = union(&[ScalarType::String, ScalarType::Int]).repeating();
    schema.numeric_range =
        IntegerRange::new(Some(i64::MIN), Some(i64::MAX)).map(NumericRange::Integer);
    schema.nullable = true;
    schema.container_nullable = true;
    let original = schema.metadata_is_valid();
    eprintln!("complete repeated nullable schema={schema:#?}; original={original}");
    assert!(original);
    let bytes = serde_json::to_vec(&schema).expect("complete encoding");
    let original = serde_json::from_slice::<SchemaNode>(&bytes);
    eprintln!("complete repeated nullable decode original={original:#?}");
    assert_eq!(original.expect("complete decode"), schema);
}
