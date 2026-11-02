use ir::*;

use super::routes;

#[test]
fn ignored_out_of_variant_fields_keep_original_semantics_in_both_orders() {
    for irrelevant in [
        r#""children":false"#,
        r#""ty":{}"#,
        r#""types":"invalid""#,
        r#""dynamic":[false]"#,
        r#""alternatives":{"invalid":true}"#,
        r#""children":[{"name":false,"kind":null}]"#,
        r#""$serde_json::private::Number":"1.5""#,
        r#""ignored":{"$serde_json::private::Number":"1.5"}"#,
    ] {
        let (tag, fields) = if irrelevant.starts_with("\"ty\"") {
            ("group", r#""children":[]"#)
        } else {
            ("scalar", r#""ty":"float""#)
        };
        for text in [
            format!(r#"{{"kind":"{tag}",{fields},{irrelevant}}}"#),
            format!(r#"{{{irrelevant},{fields},"kind":"{tag}"}}"#),
        ] {
            let decoded = if irrelevant.starts_with("\"$serde_json::private::Number\"") {
                // Value's own first-key recognition can erase this *object*
                // before IR, so exercise its live routes and an explicit
                // Value::Object separately, never parse the object via Value.
                super::live_routes::<SchemaKind>(&text)
            } else {
                routes::<SchemaKind>(&text)
            };
            for actual in decoded {
                match actual {
                    SchemaKind::Scalar { ty } => assert_eq!(ty, ScalarType::Float),
                    SchemaKind::Group { children, .. } => assert!(children.is_empty()),
                    _ => panic!("unexpected kind"),
                }
            }
        }
    }
    let explicit =
        serde_json::json!({"$serde_json::private::Number":"1.5", "kind":"scalar", "ty":"float"});
    assert!(matches!(
        serde_json::from_value::<SchemaKind>(explicit).unwrap(),
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    for text in [
        r#"{"kind":"scalar","ty":"float","children":null,"children":false}"#,
        r#"{"children":null,"children":false,"ty":"float","kind":"scalar"}"#,
    ] {
        assert!(serde_json::from_str::<SchemaKind>(text).is_ok());
    }
    for text in [
        r#"{"kind":"number","ignored":false,"bounds":{"minimum":{"value":1.5,"extra":false},"extra":true}}"#,
        r#"{"bounds":{"extra":false,"minimum":{"extra":true,"value":1.5}},"ignored":true,"kind":"number"}"#,
    ] {
        for value in routes::<NumericRange>(text) {
            assert!(matches!(value, NumericRange::Number(_)));
        }
    }
}

#[test]
fn selected_duplicates_unknown_fields_types_and_invariants_still_reject() {
    for text in [
        r#"{"kind":"scalar","ty":"float","ty":"float"}"#,
        r#"{"ty":"float","ty":"float","kind":"scalar"}"#,
        r#"{"kind":"group","children":[],"children":[]}"#,
        r#"{"kind":"scalar","kind":"scalar","ty":"float"}"#,
        r#"{"kind":"scalar_union","types":["float","float"]}"#,
    ] {
        assert!(serde_json::from_str::<SchemaKind>(text).is_err(), "{text}");
    }
    for text in [
        r#"{"kind":"number","bounds":{"minimum":{"value":1.5,"value":2.5}}}"#,
        r#"{"bounds":{"minimum":{"value":1.5}},"bounds":{"minimum":{"value":2.5}},"kind":"number"}"#,
        r#"{"kind":"number","bounds":{"minimum":{"value":1.5,"exclusive":true},"maximum":{"value":1.5}}}"#,
        r#"{"kind":"integer","bounds":{"minimum":9223372036854775808}}"#,
        r#"{"bounds":{"minimum":-9223372036854775809},"kind":"integer"}"#,
        r#"{"bounds":{"minimum":18446744073709551616},"kind":"integer"}"#,
        r#"{"bounds":{"minimum":1.5},"kind":"integer"}"#,
        r#"{"bounds":{"minimum":{"value":1e400}},"kind":"number"}"#,
        r#"{"kind":"number","bounds":{"minimum":{"value":1e400}}}"#,
    ] {
        assert!(
            serde_json::from_str::<NumericRange>(text).is_err(),
            "{text}"
        );
    }
    for text in [
        r#"{"kind":"schema","schema":{"name":"x","kind":{"kind":"scalar","ty":"float"}},"unknown":true}"#,
        r#"{"kind":"schema","schema":null}"#,
    ] {
        assert!(
            serde_json::from_str::<JsonSchemaPredicate>(text).is_err(),
            "{text}"
        );
    }
    // Serde's existing internal-tag unit variant ignores these fields even
    // with deny_unknown_fields; preserve that behavior, not a stricter policy.
    for text in [
        r#"{"unknown":true,"kind":"never"}"#,
        r#"{"kind":"never","unknown":true}"#,
    ] {
        assert!(matches!(
            serde_json::from_str::<JsonSchemaPredicate>(text).unwrap(),
            JsonSchemaPredicate::Never
        ));
    }
    for text in [
        r#"{"type":"float","value":1.5,"unknown":true}"#,
        r#"{"value":1.5,"value":2.5,"type":"float"}"#,
    ] {
        assert!(
            serde_json::from_str::<JsonAllowedValue>(text).is_err(),
            "{text}"
        );
    }
    for text in [
        r#"[{"type":"float","value":1.0},{"type":"float","value":2.5}]"#,
        r#"[{"type":"float","value":2.5},{"type":"float","value":1.5}]"#,
        r#"[{"type":"float","value":1.5},{"type":"float","value":1.5}]"#,
    ] {
        assert!(
            serde_json::from_str::<JsonAllowedValues>(text).is_err(),
            "{text}"
        );
    }
    let mut invalid = super::metadata::amount("wrong-type");
    invalid.kind = SchemaKind::Scalar {
        ty: ScalarType::String,
    };
    assert!(serde_json::from_value::<SchemaNode>(serde_json::to_value(&invalid).unwrap()).is_err());
    let mut over_limit = vec![serde_json::json!({"type":"json_null"})];
    for i in 0..MAX_JSON_ALLOWED_VALUES {
        over_limit.push(serde_json::json!({"type":"int","value":i}));
    }
    assert!(
        serde_json::from_value::<JsonAllowedValues>(serde_json::Value::Array(over_limit)).is_err()
    );
}

#[test]
fn ignored_overflow_and_large_integer_float_capture_do_not_admit_integer_overflow() {
    let text = r#"{"kind":"scalar","ty":"float","ignored":1e400}"#;
    assert_eq!(
        serde_json::from_str::<SchemaKind>(text).is_ok(),
        cfg!(feature = "arbitrary_precision")
    );
    for number in [
        "18446744073709551616",
        "-170141183460469231731687303715884105728",
    ] {
        let integer = format!(r#"{{"bounds":{{"minimum":{number}}},"kind":"integer"}}"#);
        let value: serde_json::Value = serde_json::from_str(&integer).unwrap();
        assert!(serde_json::from_value::<NumericRange>(value).is_err());
    }
    for number in [i64::MIN, i64::MAX] {
        let integer = format!(r#"{{"bounds":{{"minimum":{number}}},"kind":"integer"}}"#);
        for actual in routes::<NumericRange>(&integer) {
            let NumericRange::Integer(range) = actual else {
                panic!("integer range required");
            };
            assert_eq!(range.minimum(), Some(number));
        }
    }
}
