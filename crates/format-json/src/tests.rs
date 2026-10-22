use super::*;

#[test]
fn recursive_groups_round_trip_without_losing_descendants() -> Result<(), JsonFormatError> {
    let schema = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("file", ScalarType::String).repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
        ],
    );
    let source = serde_json::json!({
        "name": "root",
        "file": ["a"],
        "directory": [{
            "name": "child",
            "file": ["b"],
            "directory": [{"name": "grandchild", "file": ["c"]}]
        }]
    });
    let parsed = from_str(&source.to_string(), &schema)?;
    let child = &parsed.field("directory").unwrap().as_repeated().unwrap()[0];
    let grandchild = &child.field("directory").unwrap().as_repeated().unwrap()[0];
    assert_eq!(
        grandchild.field("name").and_then(Instance::as_scalar),
        Some(&Value::String("grandchild".into()))
    );
    assert_eq!(
        grandchild.field("file").unwrap().as_repeated().unwrap()[0].as_scalar(),
        Some(&Value::String("c".into()))
    );
    let written = to_string(&schema, &parsed)?;
    assert_eq!(from_str(&written, &schema)?, parsed);
    let rendered: serde_json::Value = serde_json::from_str(&written)?;
    assert_eq!(
        rendered["directory"][0]["directory"][0]["name"],
        "grandchild"
    );
    Ok(())
}

#[test]
fn recursive_occurrence_keeps_its_local_name_and_repetition() -> Result<(), JsonFormatError> {
    let branch = SchemaNode::group(
        "Branch",
        vec![
            SchemaNode::scalar("value", ScalarType::Int),
            SchemaNode::recursive_group("child", "Branch").repeating(),
        ],
    );
    let schema = SchemaNode::group("root", vec![branch]);
    let source = r#"{"Branch":{"value":1,"child":[{"value":2,"child":[{"value":3}]}]}}"#;
    let parsed = from_str(source, &schema)?;
    let written = to_string(&schema, &parsed)?;
    assert_eq!(from_str(&written, &schema)?, parsed);
    let rendered: serde_json::Value = serde_json::from_str(&written)?;
    assert_eq!(rendered["Branch"]["child"][0]["child"][0]["value"], 3);
    Ok(())
}

#[test]
fn recursive_schema_rejects_missing_ambiguous_and_malformed_anchors() {
    let missing = SchemaNode::group("root", vec![SchemaNode::recursive_group("child", "absent")]);
    assert!(matches!(
        from_str("{}", &missing),
        Err(JsonFormatError::MissingRecursiveAnchor { .. })
    ));
    assert!(matches!(
        to_string(&missing, &Instance::Group((Vec::new()).into())),
        Err(JsonFormatError::MissingRecursiveAnchor { .. })
    ));

    let ambiguous = SchemaNode::group(
        "root",
        vec![
            SchemaNode::group("Branch", Vec::new()),
            SchemaNode::group("Branch", Vec::new()),
            SchemaNode::recursive_group("child", "Branch"),
        ],
    );
    assert!(matches!(
        from_str("{}", &ambiguous),
        Err(JsonFormatError::MissingRecursiveAnchor { .. })
    ));

    let mut malformed = SchemaNode::recursive_group("child", "root");
    if let SchemaKind::Group { children, .. } = &mut malformed.kind {
        children.push(SchemaNode::scalar("unexpected", ScalarType::String));
    }
    let malformed = SchemaNode::group("root", vec![malformed]);
    assert!(matches!(
        from_str("{}", &malformed),
        Err(JsonFormatError::InvalidRecursiveReference { .. })
    ));

    let mut constrained = SchemaNode::recursive_group("child", "root");
    constrained.property_count_range = ir::PropertyCountRange::new(1, None);
    let constrained = SchemaNode::group("root", vec![constrained]);
    assert!(matches!(
        from_str("{}", &constrained),
        Err(JsonFormatError::InvalidRecursiveReference { .. })
    ));
    assert!(matches!(
        to_string(&constrained, &Instance::Group((Vec::new()).into())),
        Err(JsonFormatError::InvalidRecursiveReference { .. })
    ));
}

#[test]
fn recursive_json_reads_and_writes_are_depth_bounded() {
    let schema = SchemaNode::group("node", vec![SchemaNode::recursive_group("node", "node")]);
    let mut value = serde_json::json!({});
    let mut instance = Instance::Group((Vec::new()).into());
    for _ in 0..=MAX_JSON_RECURSION_DEPTH {
        value = serde_json::json!({"node": value});
        instance = Instance::Group((vec![("node".into(), instance)]).into());
    }
    assert!(matches!(
        from_str(&value.to_string(), &schema),
        Err(JsonFormatError::RecursionLimit {
            limit: MAX_JSON_RECURSION_DEPTH
        })
    ));
    assert!(matches!(
        to_string(&schema, &instance),
        Err(JsonFormatError::RecursionLimit {
            limit: MAX_JSON_RECURSION_DEPTH
        })
    ));
}

#[test]
fn recursive_descendants_enforce_exact_unique_items() {
    let mut values = SchemaNode::scalar("values", ScalarType::Float).repeating();
    values.json_unique_items = true;
    let schema = SchemaNode::group(
        "node",
        vec![values, SchemaNode::recursive_group("node", "node")],
    );
    let duplicate = r#"{"node":{"node":{"values":[1,1.0]}}}"#;
    assert!(matches!(
        from_str(duplicate, &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(matches!(
        from_lines(duplicate, &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));

    // Raw decimal comparison must not collapse these distinct values to
    // the same f64 while checking a recursive descendant's uniqueItems.
    let distinct = r#"{"node":{"node":{"values":[1,1.0000000000000000000001]}}}"#;
    assert!(from_str(distinct, &schema).is_ok());
}

#[test]
fn json5_reads_and_writes_schema_shaped_documents() {
    let schema = schema();
    let source = "// source comment\n{Name:'Ada',Tag:[{Value:'x',Weight:0x10},],}";
    let instance = from_json5_str(source, &schema).unwrap();
    let rendered = to_json5_string(&schema, &instance).unwrap();
    assert!(rendered.contains("Name:"));
    assert!(rendered.contains("Weight:"));
    assert!(!rendered.contains("source comment"));
    assert_eq!(from_json5_str(&rendered, &schema).unwrap(), instance);
}

#[test]
fn json5_rejects_nonfinite_values_and_deep_nesting() {
    let schema = SchemaNode::group("Root", vec![SchemaNode::scalar("n", ScalarType::Float)]);
    for source in ["{n: Infinity}", "{n: -Infinity}", "{n: NaN}"] {
        assert!(from_json5_str(source, &schema).is_err(), "{source}");
    }
    let deep = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    assert!(matches!(
        from_json5_str(&deep, &schema),
        Err(JsonFormatError::Json5NestingLimit { limit: 128 })
    ));
    for separator in ['\u{2028}', '\u{2029}'] {
        let shallow = format!("// comment{separator}{{n: 1}}");
        assert!(from_json5_str(&shallow, &schema).is_ok());
        let deep = format!(
            "// comment{separator}{}0{}",
            "[".repeat(129),
            "]".repeat(129)
        );
        assert!(matches!(
            from_json5_str(&deep, &schema),
            Err(JsonFormatError::Json5NestingLimit { limit: 128 })
        ));
    }
}

#[test]
fn json5_checks_unique_items_on_decoded_arrays() {
    let mut schema = SchemaNode::scalar("items", ScalarType::Int).repeating();
    schema.json_unique_items = true;
    assert!(matches!(
        from_json5_str("[1, /* duplicate */ 1,]", &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    let mut float_schema = SchemaNode::scalar("floats", ScalarType::Float).repeating();
    float_schema.json_unique_items = true;
    assert!(matches!(
        from_json5_str("[1.0, 1.00]", &float_schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    for source in [
        "[0x10, 16]",
        "[+1, 1.0]",
        "[.1e1, 1.]",
        "[-0x0, 0]",
        "[1e+2, 100]",
    ] {
        assert!(
            matches!(
                from_json5_str(source, &float_schema),
                Err(JsonFormatError::UniqueItemsMismatch { .. })
            ),
            "{source}"
        );
    }
    // The decoded f64 values are the same, but the source decimals are
    // mathematically distinct. Exact lexical validation must allow them.
    assert!(from_json5_str("[1.00000000000000000001, 1.0]", &float_schema).is_ok());
    assert!(from_json5_str("[9007199254740992.0, 9007199254740993.0]", &float_schema).is_ok());
}

#[test]
fn json5_unique_items_compare_objects_without_key_order() {
    let mut schema = SchemaNode::group(
        "entry",
        vec![
            SchemaNode::scalar("ab", ScalarType::Int),
            SchemaNode::scalar("other", ScalarType::String),
        ],
    )
    .repeating();
    schema.json_unique_items = true;
    let source = "[{'ab':1,other:'x',},{other:'x',a\\u0062:0x1,}]";
    assert!(matches!(
        from_json5_str(source, &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
}

fn pattern_property_schema(
    sources: impl IntoIterator<Item = &'static str>,
) -> Result<SchemaNode, Box<dyn std::error::Error>> {
    let mut dynamic = SchemaNode::scalar("*", ScalarType::Int);
    dynamic.nullable = true;
    let selectors = ir::JsonPatternPropertyNames::new(sources)?;
    SchemaNode::group(
        "Object",
        vec![SchemaNode::scalar("fixed", ScalarType::String)],
    )
    .with_dynamic_fields(dynamic)
    .and_then(|schema| schema.with_json_pattern_property_names(selectors))
    .ok_or_else(|| "patternProperties test schema is valid".into())
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Tag",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Weight", ScalarType::Float),
                ],
            )
            .repeating(),
        ],
    )
}

fn alternative_schema() -> SchemaNode {
    SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("state", ScalarType::String),
            SchemaNode::scalar("postcode", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "domestic".into(),
            members: vec!["name".into(), "state".into()],
            required: vec!["name".into(), "state".into()],
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "international".into(),
            members: vec!["name".into(), "postcode".into()],
            required: vec!["name".into(), "postcode".into()],
            constraints: Vec::new(),
        },
    ])
    .unwrap()
}

#[test]
fn hybrid_open_objects_preserve_order_and_reject_duplicates() {
    let schema = SchemaNode::group("Object", vec![SchemaNode::scalar("id", ScalarType::Int)])
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
        .unwrap();
    let value = serde_json::json!({"before": "B", "id": 7, "after": "A"});
    let instance = read_node(&value, &schema).unwrap();
    let Instance::Group(fields) = &instance else {
        panic!("open object should read as a group")
    };
    assert_eq!(
        fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["before", "id", "after"]
    );
    assert_eq!(write_node(&schema, &instance).unwrap(), value);

    let duplicate = Instance::Group(
        (vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("first".into())),
            ),
            (
                "name".into(),
                Instance::Scalar(Value::String("second".into())),
            ),
        ])
        .into(),
    );
    assert!(matches!(
        write_node(&schema, &duplicate),
        Err(JsonFormatError::DuplicateProperty { ref property, .. }) if property == "name"
    ));
}

#[test]
fn pattern_properties_select_only_dynamic_names_and_preserve_fixed_precedence()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = pattern_property_schema(["^x-", "meta"])?;
    let value = serde_json::json!({
        "fixed": "uses its declared string schema",
        "x-value": 1,
        "x-meta": 2,
        "meta-value": null
    });
    let instance = read_node(&value, &schema)?;
    assert_eq!(write_node(&schema, &instance)?, value);

    assert!(matches!(
        read_node(&serde_json::json!({"other": 1}), &schema),
        Err(JsonFormatError::UnmatchedPatternProperty { ref property, .. })
            if property == "other"
    ));
    assert!(matches!(
        write_node(
            &schema,
            &Instance::Group((vec![(
                "other".into(),
                Instance::Scalar(Value::Int(1)),
            )]).into()),
        ),
        Err(JsonFormatError::UnmatchedPatternProperty { ref property, .. })
            if property == "other"
    ));
    assert_eq!(
        write_node(
            &schema,
            &Instance::Group((vec![("other".into(), Instance::Scalar(Value::Null),)]).into()),
        )?,
        serde_json::json!({})
    );
    Ok(())
}

#[test]
fn pattern_properties_apply_to_json_lines_with_one_shared_work_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = pattern_property_schema(["^x-"])?;
    let rows = from_lines("{\"x-a\":1}\n{\"x-b\":null}\n", &schema)?;
    assert_eq!(to_lines(&schema, &rows)?, "{\"x-a\":1}\n{\"x-b\":null}\n");
    assert!(matches!(
        from_lines("{\"x-a\":1}\n{\"other\":2}\n", &schema),
        Err(JsonFormatError::UnmatchedPatternProperty { ref property, .. })
            if property == "other"
    ));

    let expensive = pattern_property_schema(["^(a?){8000}$"])?;
    let key = "a".repeat(8_000);
    assert!(matches!(
        from_str(&format!("{{{key:?}:1}}"), &expensive),
        Err(JsonFormatError::PatternWorkLimit { ref name }) if name == "Object"
    ));
    Ok(())
}

#[test]
fn object_alternatives_validate_and_preserve_each_projection() {
    let schema = alternative_schema();
    for value in [
        serde_json::json!({"name": "A", "state": "WA"}),
        serde_json::json!({"name": "B", "postcode": "SW1"}),
    ] {
        let instance = read_node(&value, &schema).unwrap();
        assert_eq!(write_node(&schema, &instance).unwrap(), value);
    }
    assert!(matches!(
        read_node(&serde_json::json!({"name": "A"}), &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    assert!(matches!(
        read_node(
            &serde_json::json!({"name": "A", "state": "WA", "postcode": "SW1"}),
            &schema
        ),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    for invalid in [
        serde_json::json!({"name": "A", "state": null}),
        serde_json::json!({"name": "A", "state": "WA", "extra": true}),
    ] {
        assert!(matches!(
            read_node(&invalid, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
    }
}

#[test]
fn xsd_type_alternatives_accept_only_unique_member_set_refinement() {
    let mut derived = SchemaNode::group(
        "Contact",
        ["ID", "First", "Last", "Address"]
            .into_iter()
            .map(|name| SchemaNode::scalar(name, ScalarType::String))
            .collect(),
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "ContactType".into(),
            members: ["ID", "First", "Last"].map(str::to_owned).to_vec(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "ContactTypeWithAddress".into(),
            members: ["ID", "First", "Last", "Address"]
                .map(str::to_owned)
                .to_vec(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .unwrap();
    let base_value = serde_json::json!({"ID": "1", "First": "Loby", "Last": "Matise"});
    assert!(matches!(
        read_node(&base_value, &derived),
        Err(JsonFormatError::AmbiguousAlternative { .. })
    ));
    derived.xml_type_alternatives = true;
    let base_instance = read_node(&base_value, &derived).unwrap();
    assert_eq!(write_node(&derived, &base_instance).unwrap(), base_value);
    let extended = serde_json::json!({
        "ID": "2", "First": "Alex", "Last": "Martin", "Address": "Main Street"
    });
    let extended_instance = read_node(&extended, &derived).unwrap();
    assert_eq!(write_node(&derived, &extended_instance).unwrap(), extended);

    let mut incomparable = SchemaNode::group(
        "Contact",
        ["ID", "First", "Last"]
            .into_iter()
            .map(|name| SchemaNode::scalar(name, ScalarType::String))
            .collect(),
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "FirstContact".into(),
            members: ["ID", "First"].map(str::to_owned).to_vec(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "LastContact".into(),
            members: ["ID", "Last"].map(str::to_owned).to_vec(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .unwrap();
    incomparable.xml_type_alternatives = true;
    assert!(matches!(
        read_node(&serde_json::json!({"ID": "3"}), &incomparable),
        Err(JsonFormatError::AmbiguousAlternative { .. })
    ));
    assert!(matches!(
        write_node(
            &incomparable,
            &Instance::Group(
                (vec![("ID".into(), Instance::Scalar(Value::String("3".into())))]).into()
            )
        ),
        Err(JsonFormatError::AmbiguousAlternative { .. })
    ));
}

#[test]
fn object_alternatives_match_required_string_constraints() {
    let schema = SchemaNode::group(
        "Event",
        vec![
            SchemaNode::scalar("kind", ScalarType::String),
            SchemaNode::scalar("value", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "created".into(),
            members: vec!["kind".into(), "value".into()],
            required: vec!["kind".into(), "value".into()],
            constraints: vec![ir::GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::String("created".into()),
            }],
        },
        ir::GroupAlternative {
            name: "deleted".into(),
            members: vec!["kind".into(), "value".into()],
            required: vec!["kind".into(), "value".into()],
            constraints: vec![ir::GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::String("deleted".into()),
            }],
        },
    ])
    .unwrap();

    for value in [
        serde_json::json!({"kind": "created", "value": "one"}),
        serde_json::json!({"kind": "deleted", "value": "two"}),
    ] {
        let instance = read_node(&value, &schema).unwrap();
        assert_eq!(write_node(&schema, &instance).unwrap(), value);
    }
    for value in [
        serde_json::json!({"kind": "changed", "value": "three"}),
        serde_json::json!({"kind": null, "value": "four"}),
        serde_json::json!({"value": "five"}),
    ] {
        assert!(matches!(
            read_node(&value, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
    }
}

#[test]
fn text_io_roundtrips_nested_repeating_groups() {
    let tag = |v: &str, w: f64| {
        Instance::Group(
            (vec![
                ("Value".into(), Instance::Scalar(Value::String(v.into()))),
                ("Weight".into(), Instance::Scalar(Value::Float(w))),
            ])
            .into(),
        )
    };
    let instance = Instance::Group(
        (vec![
            (
                "Name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            (
                "Tag".into(),
                Instance::Repeated(vec![tag("a", 1.5), tag("b", 2.0)]),
            ),
        ])
        .into(),
    );

    let text = to_string(&schema(), &instance).unwrap();
    let read_back = from_str(&text, &schema()).unwrap();

    assert!(text.ends_with('\n'));
    assert_eq!(read_back, instance);
}

#[test]
fn text_io_supports_repeating_roots() {
    let schema = SchemaNode::scalar("Value", ScalarType::Int).repeating();
    let instance = Instance::Repeated(vec![
        Instance::Scalar(Value::Int(1)),
        Instance::Scalar(Value::Int(2)),
    ]);

    let text = to_string(&schema, &instance).unwrap();

    assert_eq!(from_str(&text, &schema).unwrap(), instance);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        serde_json::json!([1, 2])
    );
}

#[test]
fn scalar_union_writes_only_unambiguous_existing_scalar_coercions() -> Result<(), JsonFormatError> {
    let Some(types) = ScalarTypeSet::new([ScalarType::Float, ScalarType::String]) else {
        panic!("test scalar union must contain distinct types");
    };
    let float_or_string = SchemaNode::scalar_union("value", types);
    assert_eq!(
        write_node(&float_or_string, &Instance::Scalar(Value::Int(42)))?,
        serde_json::json!(42)
    );

    let Some(types) = ScalarTypeSet::new([ScalarType::Int, ScalarType::String]) else {
        panic!("test scalar union must contain distinct types");
    };
    let int_or_string = SchemaNode::scalar_union("value", types);
    assert_eq!(
        write_node(
            &int_or_string,
            &Instance::Scalar(Value::String("42".into()))
        )?,
        serde_json::json!("42")
    );

    let Some(types) = ScalarTypeSet::new([ScalarType::Float, ScalarType::Bool]) else {
        panic!("test scalar union must contain distinct types");
    };
    let float_or_bool = SchemaNode::scalar_union("value", types);
    assert!(matches!(
        write_node(
            &float_or_bool,
            &Instance::Scalar(Value::Int((1_i64 << f64::MANTISSA_DIGITS) + 1))
        ),
        Err(JsonFormatError::Shape {
            got: "int outside the exact f64 range",
            ..
        })
    ));
    let Some(types) = ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]) else {
        panic!("test scalar union must contain distinct types");
    };
    let ambiguous_numeric = SchemaNode::scalar_union("value", types);
    assert!(matches!(
        write_node(
            &ambiguous_numeric,
            &Instance::Scalar(Value::String("1".into()))
        ),
        Err(JsonFormatError::Shape {
            expected: "unambiguous declared scalar union",
            got: "string",
            ..
        })
    ));
    Ok(())
}

#[test]
fn json_lines_roundtrips_rows_without_an_enclosing_array() {
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("count", ScalarType::Int),
        ],
    );
    let text = "{\"name\":\"first\",\"count\":1}\n\n{\"name\":\"second\",\"count\":2}\n";

    let rows = from_lines(text, &schema).unwrap();
    assert_eq!(
        to_lines(&schema, &rows).unwrap(),
        text.replace("\n\n", "\n")
    );
    assert_eq!(rows.as_repeated().map(<[Instance]>::len), Some(2));
}

#[test]
fn leading_utf8_bom_is_accepted_for_json_documents_and_lines() {
    let scalar = SchemaNode::scalar("Value", ScalarType::Int);
    assert_eq!(
        from_str("\u{feff}42", &scalar).unwrap(),
        Instance::Scalar(Value::Int(42))
    );
    assert_eq!(
        from_lines("\u{feff}1\n2\n", &scalar).unwrap(),
        Instance::Repeated(vec![
            Instance::Scalar(Value::Int(1)),
            Instance::Scalar(Value::Int(2)),
        ])
    );
}

#[test]
fn to_string_preserves_flat_rows_for_a_non_repeating_root() {
    let schema = SchemaNode::group("Row", vec![SchemaNode::scalar("Name", ScalarType::String)]);
    let rows = Instance::Repeated(vec![
        Instance::Group(
            (vec![(
                "Name".into(),
                Instance::Scalar(Value::String("first".into())),
            )])
            .into(),
        ),
        Instance::Group(
            (vec![(
                "Name".into(),
                Instance::Scalar(Value::String("second".into())),
            )])
            .into(),
        ),
    ]);

    let text = to_string(&schema, &rows).unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        serde_json::json!([{"Name": "first"}, {"Name": "second"}])
    );
}

#[test]
fn missing_repeating_field_reads_as_empty() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_format_json_test_empty_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, r#"{ "Name": "Jane" }"#).unwrap();

    let instance = read(&path, &schema()).unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(instance.field("Tag"), Some(&Instance::Repeated(Vec::new())));
}

#[test]
fn absent_properties_read_as_null_and_are_omitted_on_write() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Nick", ScalarType::String),
            SchemaNode::group(
                "Extra",
                vec![SchemaNode::scalar("Note", ScalarType::String)],
            ),
        ],
    );
    let path = std::env::temp_dir().join(format!(
        "ferrule_format_json_test_optional_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, r#"{ "Name": "Jane" }"#).unwrap();

    let instance = read(&path, &schema).unwrap();
    assert_eq!(instance.field("Nick"), Some(&Instance::Scalar(Value::Null)));
    assert_eq!(
        instance.field("Extra"),
        Some(&Instance::Group((vec![]).into()))
    );

    // Writing the Null back omits the key instead of emitting `null`.
    write(&path, &schema, &instance).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!text.contains("Nick"), "{text}");
}

#[test]
fn explicit_null_requires_nullable_scalar_metadata() {
    let scalar = SchemaNode::scalar("Value", ScalarType::String);
    assert!(matches!(
        from_str("null", &scalar),
        Err(JsonFormatError::Shape {
            expected: "string",
            got: "null",
            ..
        })
    ));
    assert!(matches!(
        to_string(&scalar, &Instance::Scalar(Value::Null)),
        Err(JsonFormatError::Shape {
            expected: "string",
            got: "null",
            ..
        })
    ));

    let nullable = scalar.nullable().unwrap();
    let instance = from_str("null", &nullable).unwrap();
    assert_eq!(instance, Instance::Scalar(Value::json_null()));
    assert_eq!(to_string(&nullable, &instance).unwrap(), "null\n");
}

#[test]
fn nullable_object_properties_distinguish_absence_from_explicit_null() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Optional", ScalarType::String),
            SchemaNode::scalar("Nullable", ScalarType::String)
                .nullable()
                .unwrap(),
        ],
    );
    let instance = from_str("{}", &schema).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&to_string(&schema, &instance).unwrap()).unwrap();
    assert_eq!(value, serde_json::json!({}));

    let instance = from_str(r#"{"Nullable":null}"#, &schema).unwrap();
    assert_eq!(
        instance.field("Nullable"),
        Some(&Instance::Scalar(Value::json_null()))
    );
    let value: serde_json::Value =
        serde_json::from_str(&to_string(&schema, &instance).unwrap()).unwrap();
    assert_eq!(value, serde_json::json!({"Nullable": null}));
}

#[test]
fn null_only_omits_scalar_leaves() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group(
                "Object",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            SchemaNode::scalar("Items", ScalarType::String).repeating(),
        ],
    );

    for field in ["Object", "Items"] {
        let instance =
            Instance::Group((vec![(field.to_string(), Instance::Scalar(Value::Null))]).into());
        let error = write_node(&schema, &instance).unwrap_err();
        assert!(matches!(
            error,
            JsonFormatError::Shape { ref name, got: "null", .. } if name == field
        ));
    }
}

#[test]
fn wrong_shape_is_reported_with_field_name() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_format_json_test_bad_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, r#"{ "Name": 42, "Tag": [] }"#).unwrap();

    let err = read(&path, &schema()).unwrap_err();
    std::fs::remove_file(&path).unwrap();

    assert!(
        matches!(err, JsonFormatError::Shape { ref name, expected: "string", .. } if name == "Name")
    );
}

fn write_scalar_value(ty: ScalarType, value: Value) -> Result<serde_json::Value, JsonFormatError> {
    write_node(&SchemaNode::scalar("Field", ty), &Instance::Scalar(value))
}

#[test]
fn string_leaves_serialize_every_finite_scalar_as_json_text() {
    for (value, expected) in [
        (Value::Bool(true), "true"),
        (Value::Int(-42), "-42"),
        (Value::Float(2.5), "2.5"),
        (Value::String("value".into()), "value"),
    ] {
        assert_eq!(
            write_scalar_value(ScalarType::String, value).unwrap(),
            serde_json::Value::String(expected.into())
        );
    }
}

#[test]
fn typed_leaves_coerce_text_and_widen_integers_to_numbers() {
    assert_eq!(
        write_scalar_value(ScalarType::Int, Value::String(" 42 ".into())).unwrap(),
        serde_json::json!(42)
    );
    assert_eq!(
        write_scalar_value(ScalarType::Float, Value::Int(42)).unwrap(),
        serde_json::json!(42)
    );
    assert_eq!(
        write_scalar_value(ScalarType::Float, Value::String("2.5".into())).unwrap(),
        serde_json::json!(2.5)
    );
    assert_eq!(
        write_scalar_value(ScalarType::Bool, Value::String("true".into())).unwrap(),
        serde_json::json!(true)
    );
}

#[test]
fn singleton_integer_output_normalizes_exact_decimal_text_before_constraints() {
    use ir::{IntegerRange, JsonMultipleOf, JsonMultipleOfConstraints, NumericRange};

    for (text, expected) in [
        ("1.000", 1),
        (" 2e2 ", 200),
        ("9007199254740993.0", 9_007_199_254_740_993),
        ("-9223372036854775808.000", i64::MIN),
    ] {
        assert_eq!(
            write_scalar_value(ScalarType::Int, Value::String(text.into())).unwrap(),
            serde_json::json!(expected)
        );
    }
    for text in ["1.001", "1e-1", "9223372036854775808.0"] {
        assert!(write_scalar_value(ScalarType::Int, Value::String(text.into())).is_err());
    }

    let divisor = JsonMultipleOf::from_decimal_lexical("3").unwrap();
    let multiples = JsonMultipleOfConstraints::new([[divisor]]).unwrap();
    let constrained = SchemaNode::scalar("Field", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(
            IntegerRange::new(Some(6), Some(8)).unwrap(),
        ))
        .unwrap()
        .with_json_multiple_of(multiples)
        .unwrap();
    assert_eq!(
        write_node(
            &constrained,
            &Instance::Scalar(Value::String("6.000".into()))
        )
        .unwrap(),
        serde_json::json!(6)
    );
    for text in ["7.000", "9.000"] {
        assert!(write_node(&constrained, &Instance::Scalar(Value::String(text.into()))).is_err());
    }
    assert!(read_node(&serde_json::json!("6.000"), &constrained).is_err());

    let types = ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]).unwrap();
    let union = SchemaNode::scalar_union("Field", types);
    assert_eq!(
        write_node(&union, &Instance::Scalar(Value::String("1.000".into()))).unwrap(),
        serde_json::json!(1.0)
    );
}

#[test]
fn float_leaves_only_widen_integers_that_roundtrip_exactly() -> Result<(), JsonFormatError> {
    let schema = SchemaNode::scalar("Field", ScalarType::Float);
    for value in [
        1_i64 << f64::MANTISSA_DIGITS,
        (1_i64 << f64::MANTISSA_DIGITS) + 2,
        i64::MIN,
    ] {
        let encoded = write_node(&schema, &Instance::Scalar(Value::Int(value)))?;
        assert_eq!(
            read_node(&encoded, &schema)?,
            Instance::Scalar(Value::Float(value as f64))
        );
    }

    for value in [
        (1_i64 << f64::MANTISSA_DIGITS) + 1,
        -((1_i64 << f64::MANTISSA_DIGITS) + 1),
        i64::MAX,
    ] {
        let Err(error) = write_node(&schema, &Instance::Scalar(Value::Int(value))) else {
            panic!("inexact integer must not widen to float");
        };
        assert!(matches!(
            error,
            JsonFormatError::Shape {
                ref name,
                expected: "number",
                got: "int outside the exact f64 range"
            } if name == "Field"
        ));
    }
    Ok(())
}

#[test]
fn float_leaves_accept_exact_sparse_external_integers() -> Result<(), Box<dyn std::error::Error>> {
    let schema = SchemaNode::scalar("Field", ScalarType::Float);

    for text in [
        ((1_u64 << f64::MANTISSA_DIGITS) + 2).to_string(),
        i64::MIN.to_string(),
        (1_u64 << 63).to_string(),
    ] {
        let expected = text.parse::<f64>()?;
        assert_eq!(
            from_str(&text, &schema)?,
            Instance::Scalar(Value::Float(expected))
        );
    }

    let Err(error) = from_str(&((1_u64 << f64::MANTISSA_DIGITS) + 1).to_string(), &schema) else {
        panic!("inexact external integer must not narrow to float");
    };
    assert!(matches!(
        error,
        JsonFormatError::Shape {
            ref name,
            expected: "number",
            got: "integer outside the exact f64 range"
        } if name == "Field"
    ));

    assert_eq!(
        from_str("1.25", &schema)?,
        Instance::Scalar(Value::Float(1.25))
    );
    Ok(())
}

#[test]
fn incompatible_and_non_finite_values_return_typed_errors() {
    let incompatible = write_scalar_value(ScalarType::Int, Value::Float(2.0)).unwrap_err();
    assert!(matches!(
        incompatible,
        JsonFormatError::Shape {
            ref name,
            expected: "integer",
            got: "float"
        } if name == "Field"
    ));

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let error = write_scalar_value(ScalarType::Float, Value::Float(value)).unwrap_err();
        assert!(matches!(
            error,
            JsonFormatError::Shape {
                ref name,
                expected: "finite number",
                got: "non-finite float"
            } if name == "Field"
        ));
    }

    let wrong_shape = write_node(
        &SchemaNode::scalar("Field", ScalarType::Bool),
        &Instance::Group((Vec::new()).into()),
    )
    .unwrap_err();
    assert!(matches!(
        wrong_shape,
        JsonFormatError::Shape {
            ref name,
            expected: "bool",
            got: "object"
        } if name == "Field"
    ));

    let error = write_node(
        &SchemaNode::scalar("Field", ScalarType::Bool),
        &Instance::MappedSequence(Vec::new()),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        JsonFormatError::Shape {
            got: "mapped sequence",
            ..
        }
    ));
}

#[test]
fn json_output_normalizes_bounded_mapped_sequences() -> Result<(), Box<dyn std::error::Error>> {
    let scalar = SchemaNode::scalar("Field", ScalarType::String);
    let root = SchemaNode::group("Root", vec![scalar.clone()]);
    let item = |value: &str| Instance::Scalar(Value::String(value.into()));
    let field =
        |items| Instance::Group((vec![("Field".into(), Instance::MappedSequence(items))]).into());

    let omitted: serde_json::Value = serde_json::from_str(&to_string(&root, &field(vec![]))?)?;
    assert_eq!(omitted, serde_json::json!({}));
    let one: serde_json::Value =
        serde_json::from_str(&to_string(&root, &field(vec![item("one")]))?)?;
    assert_eq!(one, serde_json::json!({"Field": "one"}));
    let error = to_string(&root, &field(vec![item("one"), item("two")])).unwrap_err();
    assert!(matches!(
        error,
        JsonFormatError::Shape {
            ref name,
            expected: "one mapped item",
            got: "mapped sequence"
        } if name == "Field"
    ));

    let required = root
        .clone()
        .with_required_fields(vec!["Field".into()])
        .expect("valid required field");
    assert!(matches!(
        to_string(&required, &field(vec![])),
        Err(JsonFormatError::MissingRequiredProperty { ref property, .. }) if property == "Field"
    ));

    let repeating = scalar.repeating();
    for (items, expected) in [
        (vec![], serde_json::json!([])),
        (vec![item("one")], serde_json::json!(["one"])),
        (
            vec![item("one"), item("two")],
            serde_json::json!(["one", "two"]),
        ),
    ] {
        let actual: serde_json::Value =
            serde_json::from_str(&to_string(&repeating, &Instance::MappedSequence(items))?)?;
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[test]
fn root_row_arrays_validate_every_object_and_nested_assertion()
-> Result<(), Box<dyn std::error::Error>> {
    let range = ir::StringLengthRange::new(2, Some(5)).expect("valid length range");
    let name = SchemaNode::scalar("Name", ScalarType::String)
        .with_string_length_range(range)
        .expect("string length applies to string");
    let tags = SchemaNode::scalar("Tags", ScalarType::Int)
        .repeating()
        .with_json_unique_items()
        .expect("uniqueItems applies to arrays");
    let nested = SchemaNode::group("Nested", vec![]);
    let schema = SchemaNode::group("Row", vec![name, tags, nested])
        .with_required_fields(vec!["Name".into()])
        .expect("required field exists");

    assert_eq!(from_str("[]", &schema)?, Instance::Repeated(vec![]));
    let rows = from_str(
        r#"[{"Name":"Ada","Tags":[1,2]},{"Name":"Eve","Tags":[3]}]"#,
        &schema,
    )?;
    assert!(matches!(rows, Instance::Repeated(ref items) if items.len() == 2));
    assert!(matches!(
        from_str(r#"[{"Name":"Ada"},{}]"#, &schema),
        Err(JsonFormatError::MissingRequiredProperty { ref property, .. }) if property == "Name"
    ));
    assert!(matches!(
        from_str(r#"[{"Name":"Ada"},{"Name":"X"}]"#, &schema),
        Err(JsonFormatError::StringLengthMismatch { .. })
    ));
    assert!(matches!(
        from_str(r#"[{"Name":"Ada"},{"Name":"Eve","Nested":[]}]"#, &schema),
        Err(JsonFormatError::Shape { ref name, expected: "object", got: "array" })
            if name == "Nested"
    ));
    assert!(matches!(
        from_str(r#"[{"Name":"Ada"},{"Name":"Eve","Tags":[1,1]}]"#, &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(matches!(
        from_json5_str("[{Name: 'Ada'}, {Name: 'Eve', Tags: [1, 1]}]", &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    Ok(())
}
