use ir::{Decimal96, JsonNull, Value, XmlNil};

#[test]
fn decimal_values_keep_exact_identity_and_tagged_wire() {
    for text in [
        "0",
        "0.1",
        "-1.25",
        "9007199254740993",
        "79228162514264337593543950335",
        "0.0000000000000000000000000001",
    ] {
        let decimal = Decimal96::parse_exact(text).unwrap();
        let value = Value::decimal(decimal);
        assert_eq!(value.type_name(), "decimal");
        assert_eq!(value.as_decimal(), Some(&decimal));
        assert!(!value.is_null_like());
        let expected = format!(r#"{{"$decimal96":"{text}"}}"#);
        assert_eq!(serde_json::to_string(&value).unwrap(), expected);
        assert_eq!(serde_json::from_str::<Value>(&expected).unwrap(), value);
        assert_eq!(value.clone(), value);
    }
    assert_ne!(
        Value::decimal(Decimal96::parse_exact("1").unwrap()),
        Value::Int(1)
    );
    assert_ne!(
        Value::decimal(Decimal96::parse_exact("0.1").unwrap()),
        Value::Float(0.1)
    );
    assert!(Value::String("0.1".into()).as_decimal().is_none());
    assert!(matches!(
        serde_json::from_str::<Value>("0.1").unwrap(),
        Value::Float(_)
    ));
}

#[test]
fn malformed_or_noncanonical_decimal_tags_are_refused() {
    for text in [
        "+1",
        "01",
        "-0",
        "1.0",
        "1e0",
        "NaN",
        "",
        "79228162514264337593543950336",
    ] {
        assert!(serde_json::from_str::<Value>(&format!(r#"{{"$decimal96":"{text}"}}"#)).is_err());
    }
    for text in [
        r#"{"$decimal96":1}"#,
        r#"{"$decimal96":"1","extra":true}"#,
        r#"{"$decimal96":"1","$decimal96":"2"}"#,
    ] {
        assert!(serde_json::from_str::<Value>(text).is_err());
    }
}

#[test]
fn boxed_decimal_does_not_expand_existing_scalar_layout() {
    #[allow(dead_code)]
    enum PreviousValue {
        Null,
        JsonNull(JsonNull),
        Bool(bool),
        Int(i64),
        Float(f64),
        String(String),
        XmlNil(XmlNil),
    }
    assert_eq!(
        std::mem::size_of::<Value>(),
        std::mem::size_of::<PreviousValue>()
    );
    assert_eq!(
        std::mem::align_of::<Value>(),
        std::mem::align_of::<PreviousValue>()
    );
}
