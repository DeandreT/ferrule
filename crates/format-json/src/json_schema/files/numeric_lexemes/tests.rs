fn parse(bytes: &[u8]) -> Result<serde_json::Value, crate::JsonFormatError> {
    let super::ParsedDocument { value, lexemes } = super::parse(bytes)?;
    let document = super::LoadedSchema::new(value, lexemes);
    Ok(document.schema_view(&document)?.into_owned())
}

#[test]
fn final_decoded_duplicate_keys_keep_original_property_order() -> Result<(), crate::JsonFormatError> {
    let text = r#"{
        "type":"integer", "minimum":1.0, "max\u0069mum":4e0,
        "min\u0069mum":3e0, "multipleOf":2.0
    }"#;
    let original: serde_json::Value = serde_json::from_str(text)?;
    let normalized = parse(text.as_bytes())?;
    assert_eq!(normalized["minimum"].as_i64(), Some(3));
    assert_eq!(normalized["maximum"].as_i64(), Some(4));
    assert_eq!(normalized["multipleOf"], original["multipleOf"]);
    assert!(normalized["multipleOf"].as_number().is_some_and(|n| n.is_f64()));
    assert_eq!(
        normalized.as_object().map(|o| o.keys().collect::<Vec<_>>()),
        original.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    let last_fraction = parse(br#"{"minimum":2.0,"min\u0069mum":1.00000000000000001}"#)?;
    assert!(last_fraction["minimum"].as_number().is_some_and(|n| n.is_f64()));
    Ok(())
}

#[test]
fn opaque_and_non_bound_numbers_keep_the_original_value() -> Result<(), crate::JsonFormatError> {
    let text = r#"{
        "minimum":2.0, "multipleOf":2.0, "minItems":2.0,
        "minLength":2.0, "maxProperties":2.0,
        "default":{"minimum":2.0,"properties":{"x":{"minimum":2.0}}},
        "examples":[{"maximum":2.0}], "enum":[{"exclusiveMinimum":2.0}],
        "const":{"exclusiveMaximum":2.0},
        "unknown":{"$defs":{"x":{"minimum":2.0}}}
    }"#;
    let mut original: serde_json::Value = serde_json::from_str(text)?;
    let normalized = parse(text.as_bytes())?;
    assert_eq!(normalized["minimum"].as_i64(), Some(2));
    original["minimum"] = serde_json::Value::from(2);
    assert_eq!(normalized, original);
    Ok(())
}

#[test]
fn only_reference_loader_schema_positions_normalize() -> Result<(), crate::JsonFormatError> {
    for keyword in ["$defs", "definitions", "dependentSchemas", "patternProperties", "properties"] {
        // A property called minimum is a schema-map name, not a numeric bound.
        let text = format!(r#"{{"{keyword}":{{"minimum":{{"minimum":2.0}}}}}}"#);
        assert_eq!(parse(text.as_bytes())?[keyword]["minimum"]["minimum"].as_i64(), Some(2));
        let malformed = format!(r#"{{"{keyword}":{{"minimum":2.0}}}}"#);
        assert!(parse(malformed.as_bytes())?[keyword]["minimum"].as_number().is_some_and(|n| n.is_f64()));
    }
    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        let text = format!(r#"{{"{keyword}":[{{"maximum":2e0}}]}}"#);
        assert_eq!(parse(text.as_bytes())?[keyword][0]["maximum"].as_i64(), Some(2));
    }
    for keyword in [
        "additionalItems", "additionalProperties", "contains", "contentSchema", "else", "if", "not",
        "propertyNames", "then", "unevaluatedItems", "unevaluatedProperties",
    ] {
        let text = format!(r#"{{"{keyword}":{{"exclusiveMinimum":2.0}}}}"#);
        assert_eq!(parse(text.as_bytes())?[keyword]["exclusiveMinimum"].as_i64(), Some(2));
    }
    for items in [r#"{"maximum":2.0}"#, r#"[{"maximum":2.0}]"#] {
        let text = format!(r#"{{"items":{items}}}"#);
        let value = parse(text.as_bytes())?;
        let item = if value["items"].is_array() { &value["items"][0] } else { &value["items"] };
        assert_eq!(item["maximum"].as_i64(), Some(2));
    }
    let text = br#"{"dependencies":{"shape":{"maximum":2.0},"names":["minimum"],"opaque":2.0}}"#;
    let original: serde_json::Value = serde_json::from_slice(text)?;
    let value = parse(text)?;
    assert_eq!(value["dependencies"]["shape"]["maximum"].as_i64(), Some(2));
    assert_eq!(value["dependencies"]["names"], original["dependencies"]["names"]);
    assert_eq!(value["dependencies"]["opaque"], original["dependencies"]["opaque"]);
    Ok(())
}

#[test]
fn fractional_and_out_of_token_domain_bounds_keep_original_numbers() -> Result<(), crate::JsonFormatError> {
    for token in [
        "1.00000000000000001", "0.99999999999999999", "9007199254740992.5",
        "4503599627370496.5", "1e-1000", "18446744073709551616.0", "-9223372036854775809.0",
    ] {
        let text = format!(r#"{{"minimum":{token}}}"#);
        let original: serde_json::Value = serde_json::from_str(&text)?;
        assert_eq!(parse(text.as_bytes())?, original, "{token}");
    }
    Ok(())
}

#[test]
fn original_json_grammar_overflow_and_depth_errors_remain_authoritative() {
    let deep = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    for text in [
        r#"{"minimum":01.0}"#,
        r#"{"minimum":1.0,}"#,
        r#"{"minimum":1e9999999999999999999999999999}"#,
        deep.as_str(),
    ] {
        let original = serde_json::from_str::<serde_json::Value>(text).unwrap_err();
        let crate::JsonFormatError::Json(actual) = parse(text.as_bytes()).unwrap_err() else {
            panic!("the original parser error should be returned");
        };
        assert_eq!(actual.to_string(), original.to_string());
    }
}

#[test]
fn exact_decimal_work_does_not_expand_large_exponents() {
    let token = format!("1{}e-100000", "0".repeat(100_000));
    assert_eq!(super::exact_integer(&token).and_then(|n| n.as_i64()), Some(1));
    assert_eq!(super::exact_integer("-0.000e9999999999999999999999999999").and_then(|n| n.as_i64()), Some(0));
    assert!(super::exact_integer("1e9999999999999999999999999999").is_none());
    assert!(super::exact_integer("1e-9999999999999999999999999999").is_none());
}

#[test]
fn reached_opaque_targets_gain_views_while_original_and_unreferenced_data_stay_exact()
-> Result<(), crate::JsonFormatError> {
    let text = br#"{"default":{"A/B~C":{"type":"integer","minimum":2.0,"maximum":5e0}},"minimum":1.0}"#;
    let super::ParsedDocument { value, lexemes } = super::parse(text)?;
    let original = value.clone();
    let document = super::LoadedSchema::new(value, lexemes);
    let root_view = document.schema_view(&document)?;
    assert_eq!(root_view["minimum"].as_i64(), Some(1));
    assert_eq!(root_view["default"], original["default"]);
    let target = document.pointer("/default/A~1B~0C").expect("escaped target exists");
    let target_view = document.schema_view(target)?;
    assert_eq!(target_view["minimum"].as_i64(), Some(2));
    assert_eq!(target_view["maximum"].as_i64(), Some(5));
    assert_eq!(&*document, &original);
    assert!(target["minimum"].as_number().is_some_and(|n| n.is_f64()));
    Ok(())
}

#[test]
fn cloned_actual_schema_children_have_already_normalized_bound_numbers()
-> Result<(), crate::JsonFormatError> {
    let text = br#"{"minimum":1.0,"properties":{"x":{"type":"integer","minimum":2.0}},"allOf":[{"maximum":5e0}],"default":{"minimum":7.0}}"#;
    let super::ParsedDocument { value, lexemes } = super::parse(text)?;
    let original = value.clone();
    let document = super::LoadedSchema::new(value, lexemes);
    let view = document.schema_view(&document)?;
    assert!(matches!(view, std::borrow::Cow::Owned(_)));
    assert_eq!(view["properties"]["x"]["minimum"].as_i64(), Some(2));
    assert_eq!(view["allOf"][0]["maximum"].as_i64(), Some(5));
    let child = document.schema_view(&view["properties"]["x"])?;
    assert!(matches!(child, std::borrow::Cow::Borrowed(_)));
    assert_eq!(child["minimum"].as_i64(), Some(2));
    assert_eq!(view["default"], original["default"]);
    assert_eq!(&*document, &original);
    Ok(())
}
