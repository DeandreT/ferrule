use crate::*;

#[test]
fn value_json_roundtrip_picks_the_right_variant() {
    assert_eq!(serde_json::from_str::<Value>("42").unwrap(), Value::Int(42));
    assert_eq!(
        serde_json::from_str::<Value>("1.5").unwrap(),
        Value::Float(1.5)
    );
    assert_eq!(
        serde_json::from_str::<Value>("true").unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        serde_json::from_str::<Value>("\"hi\"").unwrap(),
        Value::String("hi".to_string())
    );
    assert_eq!(serde_json::from_str::<Value>("null").unwrap(), Value::Null);
    let json_null = serde_json::to_string(&Value::json_null()).unwrap();
    assert_eq!(json_null, r#"{"$json_null":true}"#);
    assert_eq!(
        serde_json::from_str::<Value>(&json_null).unwrap(),
        Value::json_null()
    );
    assert!(serde_json::from_str::<Value>(r#"{"$json_null":false}"#).is_err());
    let nil = serde_json::to_string(&Value::xml_nil()).unwrap();
    assert_eq!(nil, r#"{"$xml_nil":true}"#);
    assert_eq!(
        serde_json::from_str::<Value>(&nil).unwrap(),
        Value::xml_nil()
    );
    assert!(serde_json::from_str::<Value>(r#"{"$xml_nil":false}"#).is_err());
}

#[test]
fn scalar_union_types_are_canonical_validated_and_backward_compatible()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(types) = ScalarTypeSet::new([ScalarType::Bool, ScalarType::String, ScalarType::Int])
    else {
        panic!("test scalar union must contain distinct types");
    };
    assert!(types.contains(ScalarType::String));
    assert!(types.contains(ScalarType::Int));
    assert!(types.contains(ScalarType::Bool));
    assert!(!types.contains(ScalarType::Float));
    assert_eq!(
        types.iter().collect::<Vec<_>>(),
        vec![ScalarType::String, ScalarType::Int, ScalarType::Bool]
    );
    assert!(ScalarTypeSet::new([ScalarType::String]).is_none());
    assert!(ScalarTypeSet::new([ScalarType::String, ScalarType::String]).is_none());

    let union = SchemaNode::scalar_union("value", types);
    assert!(union.is_scalar());
    assert!(union.accepts_scalar_type(ScalarType::Bool));
    assert!(!union.accepts_scalar_type(ScalarType::Float));
    assert!(union.clone().with_fixed("ready").is_none());
    let encoded = serde_json::to_string(&union)?;
    assert!(encoded.contains(r#""kind":{"kind":"scalar_union","types":["string","int","bool"]}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, union);

    let legacy: SchemaNode =
        serde_json::from_str(r#"{"name":"value","kind":{"kind":"scalar","ty":"string"}}"#)?;
    assert_eq!(
        legacy.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );
    for invalid in [
        r#"{"name":"value","kind":{"kind":"scalar_union","types":[]}}"#,
        r#"{"name":"value","kind":{"kind":"scalar_union","types":["string"]}}"#,
        r#"{"name":"value","kind":{"kind":"scalar_union","types":["string","string"]}}"#,
        r#"{"name":"value","fixed":"ready","kind":{"kind":"scalar_union","types":["string","bool"]}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    let Some(fixed) = SchemaNode::scalar("value", ScalarType::String).with_fixed("ready") else {
        panic!("ordinary scalar fixed metadata should remain valid");
    };
    assert!(fixed.fixed_is_valid());
    assert_eq!(fixed.fixed.as_deref(), Some("ready"));
    assert!(
        SchemaNode::group("value", Vec::new())
            .with_fixed("ready")
            .is_none()
    );
    assert!(
        SchemaNode::scalar("value", ScalarType::String)
            .with_default("ready")
            .is_some_and(|node| node.with_fixed("ready").is_none())
    );

    let discriminator = SchemaNode::scalar_union("kind", types);
    let alternatives = vec![
        GroupAlternative {
            name: "text".into(),
            members: vec!["kind".into()],
            required: vec!["kind".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::String("ready".into()),
            }],
        },
        GroupAlternative {
            name: "numeric".into(),
            members: vec!["kind".into()],
            required: vec!["kind".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::Int(7),
            }],
        },
    ];
    assert!(
        SchemaNode::group("event", vec![discriminator.clone()])
            .with_alternatives(alternatives.clone())
            .is_some()
    );
    let mut invalid = alternatives;
    let Some(value) = FiniteF64::new(7.5) else {
        panic!("test float should be finite");
    };
    invalid[1].constraints[0].value = GroupAlternativeConstraintValue::Float(value);
    assert!(
        SchemaNode::group("event", vec![discriminator])
            .with_alternatives(invalid)
            .is_none()
    );
    Ok(())
}

#[test]
fn numeric_ranges_are_typed_nonempty_and_serde_validated() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(integer) = IntegerRange::new(Some(-4), Some(9)) else {
        panic!("ordered integer range is valid");
    };
    assert!(integer.contains(-4));
    assert!(integer.contains(9));
    assert!(!integer.contains(10));
    assert!(IntegerRange::new(None, None).is_none());
    assert!(IntegerRange::new(Some(2), Some(1)).is_none());

    let Some(zero) = FiniteF64::new(0.0) else {
        panic!("zero is finite");
    };
    let Some(ten) = FiniteF64::new(10.0) else {
        panic!("ten is finite");
    };
    let Some(number) = NumberRange::new(
        Some(NumberBound::exclusive(zero)),
        Some(NumberBound::inclusive(ten)),
    ) else {
        panic!("ordered number range is valid");
    };
    assert!(!number.contains(0.0));
    assert!(number.contains(0.5));
    assert!(number.contains(10.0));
    assert!(
        NumberRange::new(
            Some(NumberBound::exclusive(zero)),
            Some(NumberBound::inclusive(zero)),
        )
        .is_none()
    );

    let Some(schema) = SchemaNode::scalar_fixed("Count", ScalarType::Int, "7")
        .with_numeric_range(NumericRange::Integer(integer))
    else {
        panic!("fixed integer inside its range is valid");
    };
    let encoded = serde_json::to_string(&schema)?;
    assert!(
        encoded
            .contains(r#""numeric_range":{"kind":"integer","bounds":{"minimum":-4,"maximum":9}}"#)
    );
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
    assert!(
        SchemaNode::scalar_fixed("Count", ScalarType::Int, "10")
            .with_numeric_range(NumericRange::Integer(integer))
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Count", ScalarType::String)
            .with_numeric_range(NumericRange::Integer(integer))
            .is_none()
    );

    for invalid in [
        r#"{"name":"x","numeric_range":{"kind":"integer","bounds":{}},"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":1}},"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","numeric_range":{"kind":"integer","bounds":{"minimum":1}},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","fixed":"0","numeric_range":{"kind":"integer","bounds":{"minimum":1}},"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.0,"exclusive":true},"maximum":{"value":1.0}}},"kind":{"kind":"scalar","ty":"float"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }

    Ok(())
}

#[test]
fn json_multiple_of_constraints_are_typed_exact_and_serde_validated()
-> Result<(), Box<dyn std::error::Error>> {
    let divisor = JsonMultipleOf::from_decimal_lexical("2.5")
        .ok_or("test multipleOf divisor is representable")?;
    let constraints = JsonMultipleOfConstraints::new([[divisor]])?;

    let integer = SchemaNode::scalar("Count", ScalarType::Int)
        .with_json_multiple_of(constraints.clone())
        .ok_or("integer scalar accepts multipleOf")?;
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&serde_json::to_string(&integer)?)?,
        integer
    );
    assert!(
        SchemaNode::scalar_fixed("Count", ScalarType::Int, "5")
            .with_json_multiple_of(constraints.clone())
            .is_some()
    );
    assert!(
        SchemaNode::scalar_fixed("Count", ScalarType::Int, "6")
            .with_json_multiple_of(constraints.clone())
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Text", ScalarType::String)
            .with_json_multiple_of(constraints.clone())
            .is_none()
    );
    assert!(
        SchemaNode::group("Record", Vec::new())
            .with_json_multiple_of(constraints.clone())
            .is_none()
    );
    let arbitrary = SchemaNode::scalar("Any", ScalarType::String)
        .json_any()
        .ok_or("plain arbitrary JSON scalar is valid")?;
    assert!(
        arbitrary
            .with_json_multiple_of(constraints.clone())
            .is_none()
    );

    let types = ScalarTypeSet::new([ScalarType::String, ScalarType::Int])
        .ok_or("test scalar union is heterogeneous")?;
    assert!(
        SchemaNode::scalar_union("Value", types)
            .with_json_multiple_of(constraints)
            .is_some()
    );
    for invalid in [
        r#"{"name":"x","json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","fixed":"3","json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","json_any":true,"json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"string"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }

    Ok(())
}

#[test]
fn json_allowed_values_are_canonical_typed_and_serde_validated()
-> Result<(), Box<dyn std::error::Error>> {
    let values = JsonAllowedValues::new([
        JsonAllowedValue::String("ready".to_string()),
        JsonAllowedValue::JsonNull,
        JsonAllowedValue::String("pending".to_string()),
    ])?;
    let schema = SchemaNode::scalar("Status", ScalarType::String)
        .with_json_allowed_values(values.clone())
        .ok_or("string enum values match the scalar domain")?;
    assert!(schema.nullable);
    assert!(schema.json_allowed_values_are_valid());
    assert!(schema.json_allowed_values_tree_is_valid());
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(
        r#""json_allowed_values":[{"type":"json_null"},{"type":"string","value":"pending"},{"type":"string","value":"ready"}]"#
    ));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

    let numeric_values = JsonAllowedValues::new([
        JsonAllowedValue::Int(7),
        JsonAllowedValue::Float(FiniteF64::new(7.5).ok_or("test enum number must be finite")?),
    ])?;
    let numeric = SchemaNode::scalar("Amount", ScalarType::Float)
        .with_json_allowed_values(numeric_values)
        .ok_or("number enums admit exact integer values")?;
    assert!(!numeric.nullable);
    assert!(numeric.json_allowed_values_are_valid());
    assert!(numeric.clone().repeating().json_allowed_values_are_valid());

    let mixed_values = JsonAllowedValues::new([
        JsonAllowedValue::Int(1),
        JsonAllowedValue::String("one".to_string()),
    ])?;
    let mixed_types = ScalarTypeSet::new([ScalarType::String, ScalarType::Float])
        .ok_or("test scalar enum union has distinct types")?;
    assert!(
        SchemaNode::scalar_union("Mixed", mixed_types)
            .with_json_allowed_values(mixed_values.clone())
            .is_some()
    );
    assert!(
        SchemaNode::scalar("Text", ScalarType::String)
            .with_json_allowed_values(mixed_values)
            .is_none()
    );

    assert!(schema.clone().with_fixed("ready").is_none());
    assert!(
        SchemaNode::scalar_fixed("Status", ScalarType::String, "ready")
            .with_json_allowed_values(values.clone())
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Status", ScalarType::String)
            .json_any()
            .and_then(|schema| schema.with_json_allowed_values(values.clone()))
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Status", ScalarType::String)
            .with_json_allowed_values(values.clone())
            .and_then(SchemaNode::json_any)
            .is_none()
    );

    let mut nested = SchemaNode::group("Root", vec![schema.clone()]);
    assert!(nested.json_allowed_values_tree_is_valid());
    let SchemaKind::Group { children, .. } = &mut nested.kind else {
        return Err("test root must be a group".into());
    };
    let Some(child) = children.iter_mut().find(|child| child.name == "Status") else {
        return Err("test child must exist".into());
    };
    child.nullable = false;
    assert!(!nested.json_allowed_values_tree_is_valid());

    for invalid in [
        r#"{"name":"x","json_allowed_values":[{"type":"json_null"},{"type":"string","value":"a"}],"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","nullable":true,"json_allowed_values":[{"type":"string","value":"a"},{"type":"string","value":"b"}],"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","fixed":"a","json_allowed_values":[{"type":"string","value":"a"},{"type":"string","value":"b"}],"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","json_allowed_values":[{"type":"float","value":1.5},{"type":"float","value":2.5}],"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","json_allowed_values":[{"type":"bool","value":false},{"type":"bool","value":true}],"kind":{"kind":"group","children":[]}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    Ok(())
}

#[test]
fn item_count_ranges_require_repeating_nodes_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(range) = ItemCountRange::new(2, Some(5)) else {
        panic!("ordered item-count range is valid");
    };
    assert!(range.contains_len(2));
    assert!(range.contains_len(5));
    assert!(!range.contains_len(1));
    assert!(ItemCountRange::new(0, None).is_none());
    assert!(ItemCountRange::new(3, Some(2)).is_none());

    let Some(schema) = SchemaNode::scalar("Item", ScalarType::String)
        .repeating()
        .with_item_count_range(range)
    else {
        panic!("item-count metadata is valid on a repeating node");
    };
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(r#""item_count_range":{"minimum":2,"maximum":5}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
    assert!(
        SchemaNode::scalar("Item", ScalarType::String)
            .with_item_count_range(range)
            .is_none()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"x","item_count_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"x","repeating":true,"item_count_range":{},"kind":{"kind":"scalar","ty":"string"}}"#,
        )
        .is_err()
    );
    for invalid in [
        r#"{"name":"x","repeating":true,"item_count_range":{"minimum":-1},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","repeating":true,"item_count_range":{"maximum":1.5},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","repeating":true,"item_count_range":{"minimum":1,"maxmium":3},"kind":{"kind":"scalar","ty":"string"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    let permissive_null_maximum = r#"{"name":"x","repeating":true,"item_count_range":{"minimum":1,"maximum":null},"kind":{"kind":"scalar","ty":"string"}}"#;
    assert!(serde_json::from_str::<SchemaNode>(permissive_null_maximum).is_ok());
    Ok(())
}

#[test]
fn contains_constraints_require_array_owners_and_roundtrip_predicate_schemas()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(range) = ItemCountRange::new(1, Some(2)) else {
        panic!("test contains range is valid");
    };
    let Some(constraints) = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::schema(SchemaNode::scalar("candidate", ScalarType::Int)),
        range,
    )]) else {
        panic!("test contains constraints are valid");
    };
    let schema = SchemaNode::scalar("items", ScalarType::Int)
        .repeating()
        .with_json_contains(constraints.clone())
        .ok_or("repeating contains owner is valid")?;
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(r#""json_contains":[{"predicate":{"kind":"schema""#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
    assert!(
        SchemaNode::scalar("item", ScalarType::Int)
            .with_json_contains(constraints)
            .is_none()
    );
    Ok(())
}

#[test]
fn property_count_ranges_require_feasible_groups_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(range) = PropertyCountRange::new(1, Some(2)) else {
        panic!("ordered property-count range is valid");
    };
    assert!(range.contains_len(1));
    assert!(range.contains_len(2));
    assert!(!range.contains_len(0));
    assert!(PropertyCountRange::new(0, None).is_none());
    assert!(PropertyCountRange::new(3, Some(2)).is_none());

    let Some(schema) = SchemaNode::group(
        "Object",
        vec![
            SchemaNode::scalar("first", ScalarType::String),
            SchemaNode::scalar("second", ScalarType::String),
        ],
    )
    .with_required_fields(vec!["first".into()])
    .and_then(|schema| schema.with_property_count_range(range)) else {
        panic!("property-count metadata is feasible on the test group");
    };
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(r#""property_count_range":{"minimum":1,"maximum":2}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

    assert!(
        SchemaNode::scalar("value", ScalarType::String)
            .with_property_count_range(range)
            .is_none()
    );
    let Some(at_least_three) = PropertyCountRange::new(3, None) else {
        panic!("positive lower bound is constrained");
    };
    assert!(
        SchemaNode::group(
            "closed",
            vec![
                SchemaNode::scalar("first", ScalarType::String),
                SchemaNode::scalar("second", ScalarType::String),
            ],
        )
        .with_property_count_range(at_least_three)
        .is_none()
    );
    let Some(at_most_one) = PropertyCountRange::new(0, Some(1)) else {
        panic!("finite upper bound is constrained");
    };
    assert!(
        SchemaNode::group(
            "required",
            vec![
                SchemaNode::scalar("first", ScalarType::String),
                SchemaNode::scalar("second", ScalarType::String),
            ],
        )
        .with_required_fields(vec!["first".into(), "second".into()])
        .and_then(|schema| schema.with_property_count_range(at_most_one))
        .is_none()
    );

    for invalid in [
        r#"{"name":"x","property_count_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","property_count_range":{},"kind":{"kind":"group","children":[]}}"#,
        r#"{"name":"x","property_count_range":{"minimum":-1},"kind":{"kind":"group","children":[]}}"#,
        r#"{"name":"x","property_count_range":{"maximum":1.5},"kind":{"kind":"group","children":[]}}"#,
        r#"{"name":"x","property_count_range":{"minimum":1,"maxmium":3},"kind":{"kind":"group","children":[]}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    Ok(())
}

#[test]
fn property_dependencies_are_group_scoped_feasible_and_transactional()
-> Result<(), Box<dyn std::error::Error>> {
    let dependencies = JsonPropertyDependencies::new(std::collections::BTreeMap::from([
        ("a".into(), vec!["b".into()]),
        ("b".into(), vec!["c".into()]),
    ]))?;
    let schema = SchemaNode::group(
        "Object",
        vec![
            SchemaNode::scalar("a", ScalarType::String),
            SchemaNode::scalar("b", ScalarType::String),
            SchemaNode::scalar("c", ScalarType::String),
        ],
    )
    .with_json_property_dependencies(dependencies.clone())
    .ok_or("dependency rules fit the closed object")?;
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(r#""json_property_dependencies":{"a":["b"],"b":["c"]}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
    assert!(
        SchemaNode::scalar("value", ScalarType::String)
            .with_json_property_dependencies(dependencies.clone())
            .is_none()
    );

    let maximum_two =
        PropertyCountRange::new(0, Some(2)).ok_or("finite property maximum is valid")?;
    assert!(
        schema
            .clone()
            .with_required_fields(vec!["a".into()])
            .and_then(|schema| schema.with_property_count_range(maximum_two))
            .is_none()
    );
    let mut transactional = schema
        .clone()
        .with_property_count_range(maximum_two)
        .ok_or("optional dependency triggers fit the property maximum")?;
    assert!(!transactional.set_required_fields(vec!["a".into()]));
    assert!(transactional.required_fields().is_empty());

    let open_dependencies = JsonPropertyDependencies::new(std::collections::BTreeMap::from([(
        "a".into(),
        vec!["missing".into()],
    )]))?;
    let open = SchemaNode::group("Open", vec![SchemaNode::scalar("a", ScalarType::String)])
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .and_then(|schema| schema.with_required_fields(vec!["a".into()]))
        .and_then(|schema| schema.with_json_property_dependencies(open_dependencies))
        .ok_or("open objects can satisfy runtime-named dependency targets")?;
    let mut cannot_close = open;
    assert!(!cannot_close.set_dynamic_fields(None));
    assert!(cannot_close.dynamic_fields().is_some());

    let invalid = r#"{"name":"x","json_property_dependencies":{"a":["b"]},"kind":{"kind":"scalar","ty":"string"}}"#;
    assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    Ok(())
}

#[test]
fn property_name_constraints_are_group_scoped_feasible_and_transactional()
-> Result<(), Box<dyn std::error::Error>> {
    let allowed =
        JsonPropertyNameSet::new(["a".to_string(), "b".to_string(), "extra".to_string()])?;
    let patterns = JsonPatternConstraints::new([[r#"^(a|b|extra)$"#]])?;
    let constraints = JsonPropertyNameConstraints::schema(
        Some(allowed),
        StringLengthRange::new(1, Some(5)),
        Some(patterns),
        JsonFormatAnnotations::default(),
    )
    .ok_or("property-name constraints are not tautological")?;
    let schema = SchemaNode::group(
        "Object",
        vec![
            SchemaNode::scalar("a", ScalarType::String),
            SchemaNode::scalar("b", ScalarType::String),
            SchemaNode::scalar("blocked", ScalarType::String),
        ],
    )
    .with_required_fields(vec!["a".into()])
    .and_then(|schema| schema.with_json_property_names(constraints.clone()))
    .ok_or("required property is admitted")?;
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(r#""json_property_names":{"kind":"schema""#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

    let minimum_three =
        PropertyCountRange::new(3, None).ok_or("property minimum is constrained")?;
    assert!(
        schema
            .clone()
            .with_property_count_range(minimum_three)
            .is_none()
    );
    let mut transactional = schema;
    assert!(!transactional.set_required_fields(vec!["blocked".into()]));
    assert_eq!(transactional.required_fields(), ["a"]);

    let empty = SchemaNode::group("Empty", Vec::new())
        .with_json_property_names(JsonPropertyNameConstraints::never())
        .ok_or("empty object can reject every property name")?;
    assert!(
        empty
            .clone()
            .with_property_count_range(minimum_three)
            .is_none()
    );
    assert!(
        SchemaNode::scalar("value", ScalarType::String)
            .with_json_property_names(JsonPropertyNameConstraints::never())
            .is_none()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"x","json_property_names":{"kind":"never"},"kind":{"kind":"scalar","ty":"string"}}"#
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn unique_items_require_repeating_nodes_and_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        SchemaNode::scalar("Item", ScalarType::String)
            .with_json_unique_items()
            .is_none()
    );
    let schema = SchemaNode::group("Item", vec![SchemaNode::scalar("value", ScalarType::Int)])
        .repeating()
        .with_json_unique_items()
        .ok_or_else(|| std::io::Error::other("repeating unique-items test schema is valid"))?;
    let encoded = serde_json::to_string(&schema)?;
    assert!(encoded.contains(r#""json_unique_items":true"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

    let legacy = r#"{"name":"x","repeating":true,"kind":{"kind":"scalar","ty":"string"}}"#;
    assert!(!serde_json::from_str::<SchemaNode>(legacy)?.json_unique_items);
    let invalid = r#"{"name":"x","json_unique_items":true,"kind":{"kind":"scalar","ty":"string"}}"#;
    assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    Ok(())
}

#[test]
fn string_length_ranges_require_string_capable_domains_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(range) = StringLengthRange::new(1, Some(3)) else {
        panic!("test range is valid");
    };
    assert!(StringLengthRange::new(0, None).is_none());
    assert!(StringLengthRange::new(4, Some(3)).is_none());

    let Some(string) =
        SchemaNode::scalar("Value", ScalarType::String).with_string_length_range(range)
    else {
        panic!("string-length metadata matches a string scalar");
    };
    let encoded = serde_json::to_string(&string)?;
    assert!(encoded.contains(r#""string_length_range":{"minimum":1,"maximum":3}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, string);

    let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union is valid");
    };
    assert!(
        SchemaNode::scalar_union("Value", types)
            .repeating()
            .with_string_length_range(range)
            .is_some()
    );
    assert!(
        SchemaNode::scalar("Value", ScalarType::Int)
            .with_string_length_range(range)
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Value", ScalarType::String)
            .with_string_length_range(range)
            .and_then(SchemaNode::json_any)
            .is_none()
    );
    assert!(
        SchemaNode::scalar_fixed("Value", ScalarType::String, "")
            .with_string_length_range(range)
            .is_none()
    );

    for invalid in [
        r#"{"name":"x","string_length_range":{"minimum":1},"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","json_any":true,"string_length_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","fixed":"","string_length_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","string_length_range":{},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","string_length_range":{"minimum":-1},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","string_length_range":{"maximum":1.0},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","string_length_range":{"minimum":1,"maximum":0},"kind":{"kind":"scalar","ty":"string"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    Ok(())
}

#[test]
fn json_pattern_constraints_require_string_capable_domains_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let patterns = JsonPatternConstraints::new([
        ["^A".to_string(), "Z$".to_string()],
        ["^B$".to_string(), "^B$".to_string()],
    ])?;
    let Some(string) =
        SchemaNode::scalar("Value", ScalarType::String).with_json_patterns(patterns.clone())
    else {
        panic!("pattern metadata matches a string scalar");
    };
    let encoded = serde_json::to_string(&string)?;
    assert!(encoded.contains(r#""json_patterns":{"any_of":[["^A","Z$"],["^B$"]]}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, string);

    assert!(
        SchemaNode::scalar_fixed("Value", ScalarType::String, "ABZ")
            .with_json_patterns(patterns.clone())
            .is_some()
    );
    assert!(
        SchemaNode::scalar_fixed("Value", ScalarType::String, "B")
            .with_json_patterns(patterns.clone())
            .is_some()
    );
    assert!(
        SchemaNode::scalar_fixed("Value", ScalarType::String, "C")
            .with_json_patterns(patterns.clone())
            .is_none()
    );

    let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union is valid");
    };
    assert!(
        SchemaNode::scalar_union("Value", types)
            .repeating()
            .with_json_patterns(patterns.clone())
            .is_some()
    );
    assert!(
        SchemaNode::scalar("Value", ScalarType::Int)
            .with_json_patterns(patterns.clone())
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Value", ScalarType::String)
            .with_json_patterns(patterns)
            .and_then(SchemaNode::json_any)
            .is_none()
    );

    for invalid in [
        r#"{"name":"x","json_patterns":{"any_of":[["A"]]},"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","json_any":true,"json_patterns":{"any_of":[["A"]]},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","json_patterns":{"any_of":[]},"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","json_patterns":{"any_of":[["A","A"]]},"kind":{"kind":"scalar","ty":"string"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    let mismatched: SchemaNode = serde_json::from_str(
        r#"{"name":"x","fixed":"C","json_patterns":{"any_of":[["^A$"]]},"kind":{"kind":"scalar","ty":"string"}}"#,
    )?;
    assert!(!mismatched.json_pattern_budget_is_valid());
    Ok(())
}

#[test]
fn json_pattern_plan_budgets_are_global_deduplicated_and_include_dynamic_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut distinct_children = Vec::new();
    for index in 0..MAX_DISTINCT_JSON_PATTERNS {
        let patterns = JsonPatternConstraints::new([[format!("^value-{index}$")]])?;
        let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
            .with_json_patterns(patterns)
        else {
            panic!("pattern metadata matches a string child");
        };
        distinct_children.push(child);
    }
    let within_budget = SchemaNode::group("Root", distinct_children.clone());
    assert!(within_budget.json_pattern_budget_is_valid());

    let overflow_patterns = JsonPatternConstraints::new([["^overflow$"]])?;
    let Some(overflow) =
        SchemaNode::scalar("*", ScalarType::String).with_json_patterns(overflow_patterns)
    else {
        panic!("overflow pattern metadata is locally valid");
    };
    let Some(with_dynamic_overflow) = within_budget.clone().with_dynamic_fields(overflow) else {
        panic!("dynamic field metadata is structurally valid");
    };
    let nested_overflow = SchemaNode::group("Envelope", vec![with_dynamic_overflow]);
    assert!(!nested_overflow.json_pattern_budget_is_valid());

    let mut property_name_groups = Vec::new();
    for index in 0..=MAX_DISTINCT_JSON_PATTERNS {
        let patterns = JsonPatternConstraints::new([[format!("^property-{index}$")]])?;
        let constraints = JsonPropertyNameConstraints::schema(
            None,
            None,
            Some(patterns),
            JsonFormatAnnotations::default(),
        )
        .ok_or("property-name pattern is constraining")?;
        let Some(group) = SchemaNode::group(format!("object-{index}"), Vec::new())
            .with_json_property_names(constraints)
        else {
            panic!("one property-name pattern is locally valid");
        };
        property_name_groups.push(group);
    }
    assert!(
        !SchemaNode::group("PropertyNames", property_name_groups).json_pattern_budget_is_valid()
    );

    let mut source_heavy_children = Vec::new();
    for (index, marker) in ['b', 'c', 'd', 'e', 'f'].into_iter().enumerate() {
        let source = format!("[{}{marker}]", "a".repeat(60_000));
        let patterns = JsonPatternConstraints::new([[source]])?;
        let Some(child) = SchemaNode::scalar(format!("source-{index}"), ScalarType::String)
            .with_json_patterns(patterns)
        else {
            panic!("large character-class pattern is locally valid");
        };
        source_heavy_children.push(child);
    }
    assert!(
        !SchemaNode::group("SourceHeavy", source_heavy_children).json_pattern_budget_is_valid()
    );

    let mut instruction_heavy_children = Vec::new();
    for index in 0..14 {
        let source = format!("{}{index}", "a".repeat(5_000));
        let patterns = JsonPatternConstraints::new([[source]])?;
        let Some(child) = SchemaNode::scalar(format!("instruction-{index}"), ScalarType::String)
            .with_json_patterns(patterns)
        else {
            panic!("long literal pattern is locally valid");
        };
        instruction_heavy_children.push(child);
    }
    assert!(
        !SchemaNode::group("InstructionHeavy", instruction_heavy_children)
            .json_pattern_budget_is_valid()
    );

    let repeated_patterns = JsonPatternConstraints::new([["^shared$"]])?;
    let mut repeated_children = Vec::new();
    for index in 0..=MAX_DISTINCT_JSON_PATTERNS {
        let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
            .with_json_patterns(repeated_patterns.clone())
        else {
            panic!("shared pattern metadata matches a string child");
        };
        repeated_children.push(child);
    }
    let Some(shared_dynamic) =
        SchemaNode::scalar("*", ScalarType::String).with_json_patterns(repeated_patterns)
    else {
        panic!("shared dynamic pattern metadata is valid");
    };
    let Some(shared_root) =
        SchemaNode::group("Root", repeated_children).with_dynamic_fields(shared_dynamic)
    else {
        panic!("shared dynamic root is structurally valid");
    };
    assert!(shared_root.json_pattern_budget_is_valid());

    let costly_source = format!("^{}$", "a".repeat(6_000));
    let costly_patterns = JsonPatternConstraints::new([[costly_source]])?;
    let costly_fixed = || {
        SchemaNode::scalar_fixed("fixed", ScalarType::String, "a".repeat(6_000))
            .with_json_patterns(costly_patterns.clone())
            .ok_or("costly fixed pattern remains locally valid")
    };
    let within_fixed_work = SchemaNode::group("Fixed", vec![costly_fixed()?, costly_fixed()?]);
    assert!(within_fixed_work.json_pattern_budget_is_valid());
    let over_fixed_work = SchemaNode::group(
        "Fixed",
        vec![costly_fixed()?, costly_fixed()?, costly_fixed()?],
    );
    assert!(!over_fixed_work.json_pattern_budget_is_valid());
    Ok(())
}

#[test]
fn repeated_expansion_heavy_pattern_metadata_deserializes_before_one_root_compile()
-> Result<(), Box<dyn std::error::Error>> {
    let patterns = JsonPatternConstraints::new([["a{16000}"]])?;
    let mut children = Vec::new();
    for index in 0..1_024 {
        let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
            .with_json_patterns(patterns.clone())
        else {
            panic!("expansion-heavy pattern metadata matches a string child");
        };
        children.push(child);
    }
    let schema = SchemaNode::group("Root", children);
    let encoded = serde_json::to_string(&schema)?;
    let decoded: SchemaNode = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, schema);
    assert!(decoded.json_pattern_budget_is_valid());
    Ok(())
}

#[test]
fn json_format_annotations_require_string_capable_non_arbitrary_domains()
-> Result<(), Box<dyn std::error::Error>> {
    let formats =
        JsonFormatAnnotations::new([String::new(), "email".to_string(), "custom".to_string()])?;
    let Some(string) =
        SchemaNode::scalar("Value", ScalarType::String).with_json_formats(formats.clone())
    else {
        panic!("string format metadata is valid");
    };
    assert_eq!(string.json_formats.as_slice(), ["", "email", "custom"]);
    let encoded = serde_json::to_string(&string)?;
    assert!(encoded.contains(r#""json_formats":["","email","custom"]"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, string);
    assert!(
        SchemaNode::scalar("Value", ScalarType::Int)
            .with_json_formats(formats.clone())
            .is_none()
    );

    let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union contains distinct types");
    };
    assert!(
        SchemaNode::scalar_union("Value", types)
            .repeating()
            .with_json_formats(formats.clone())
            .is_some()
    );
    let mut arbitrary = SchemaNode::scalar("Value", ScalarType::String);
    arbitrary.json_any = true;
    arbitrary.json_formats = formats;
    assert!(!arbitrary.metadata_is_valid());

    for invalid in [
        r#"{"name":"x","json_formats":["email"],"kind":{"kind":"scalar","ty":"int"}}"#,
        r#"{"name":"x","json_any":true,"json_formats":["email"],"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","json_formats":["email","email"],"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"x","json_formats":"email","kind":{"kind":"scalar","ty":"string"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }
    Ok(())
}

#[test]
fn json_nullability_is_scalar_only_and_serde_defaulted() {
    let nullable = SchemaNode::scalar("value", ScalarType::String)
        .nullable()
        .unwrap();
    let encoded = serde_json::to_string(&nullable).unwrap();
    assert!(encoded.contains("\"nullable\":true"));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        nullable
    );

    let old_json = r#"{"name":"value","kind":{"kind":"scalar","ty":"string"}}"#;
    let old = serde_json::from_str::<SchemaNode>(old_json).unwrap();
    assert!(!old.nullable);
    assert!(SchemaNode::group("object", Vec::new()).nullable().is_none());
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"object","nullable":true,"kind":{"kind":"group","children":[]}}"#
        )
        .is_err()
    );
}

#[test]
fn json_container_nullability_and_arbitrary_values_are_validated() {
    let object = SchemaNode::group("object", Vec::new())
        .nullable_container()
        .unwrap();
    let array = SchemaNode::scalar("items", ScalarType::Int)
        .repeating()
        .nullable_container()
        .unwrap();
    let any = SchemaNode::scalar("*", ScalarType::String)
        .json_any()
        .unwrap();
    for schema in [&object, &array, &any] {
        let encoded = serde_json::to_string(schema).unwrap();
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            *schema
        );
    }

    let old: SchemaNode =
        serde_json::from_str(r#"{"name":"object","kind":{"kind":"group","children":[]}}"#).unwrap();
    assert!(!old.container_nullable);
    assert!(!old.json_any);
    assert!(
        SchemaNode::scalar("value", ScalarType::String)
            .nullable_container()
            .is_none()
    );
    assert!(SchemaNode::group("value", Vec::new()).json_any().is_none());
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"object","json_any":true,"kind":{"kind":"group","children":[]}}"#
        )
        .is_err()
    );
}

#[test]
fn arbitrary_json_rejects_bypassed_scalar_metadata_in_either_builder_order()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(any) = SchemaNode::scalar("*", ScalarType::String).json_any() else {
        panic!("plain arbitrary JSON schema should be valid");
    };
    assert!(any.clone().nullable().is_none());
    assert!(any.clone().with_fixed("value").is_none());
    assert!(any.clone().with_default("value").is_none());
    assert!(
        any.clone()
            .with_value_generation(ValueGeneration::MaxNumber)
            .is_none()
    );

    assert!(
        SchemaNode::scalar("*", ScalarType::String)
            .nullable()
            .and_then(SchemaNode::json_any)
            .is_none()
    );
    assert!(
        SchemaNode::scalar_fixed("*", ScalarType::String, "value")
            .json_any()
            .is_none()
    );
    assert!(
        SchemaNode::scalar("*", ScalarType::String)
            .with_default("value")
            .and_then(SchemaNode::json_any)
            .is_none()
    );
    assert!(
        SchemaNode::scalar("*", ScalarType::String)
            .with_value_generation(ValueGeneration::MaxNumber)
            .and_then(SchemaNode::json_any)
            .is_none()
    );

    for invalid in [
        r#"{"name":"*","json_any":true,"nullable":true,"kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"*","json_any":true,"fixed":"value","kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"*","json_any":true,"default":"value","kind":{"kind":"scalar","ty":"string"}}"#,
        r#"{"name":"*","json_any":true,"value_generation":"max_number","kind":{"kind":"scalar","ty":"string"}}"#,
    ] {
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
    }

    let Some(count) = ItemCountRange::new(1, Some(3)) else {
        panic!("test wrapper item count is valid");
    };
    let Some(match_count) = ItemCountRange::new(1, None) else {
        panic!("test contains count is valid");
    };
    let Some(contains) = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::schema(SchemaNode::scalar("match", ScalarType::Int)),
        match_count,
    )]) else {
        panic!("test contains metadata is valid");
    };
    let wrapped = any
        .repeating()
        .with_item_count_range(count)
        .and_then(|schema| schema.with_json_contains(contains))
        .and_then(SchemaNode::with_json_unique_items)
        .and_then(SchemaNode::nullable_container)
        .ok_or("arbitrary JSON arrays allow wrapper-only constraints")?;
    assert!(wrapped.metadata_is_valid());
    let encoded = serde_json::to_string(&wrapped)?;
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, wrapped);
    Ok(())
}
