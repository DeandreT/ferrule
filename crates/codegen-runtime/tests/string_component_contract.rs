//! Independently authored general-contract tests, not vendor mapping qualification.
use codegen_runtime::{
    JsonSourceError, JsonSourceKind, JsonSourceLimits, MapForceJsonString,
    MapForceStringComponentError as Error, MapForceStringInputFallback, Value,
};
use std::error::Error as _;

fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn parse(value: &str) -> MapForceJsonString {
    MapForceJsonString::parse(&text(value), JsonSourceLimits::default()).unwrap()
}

#[test]
fn ordered_string_selection_has_independent_root_presence() {
    for value in ["{}", r#"{"extra":true}"#] {
        let parsed = parse(value);
        assert!(parsed.root_object().is_some());
        assert_eq!(parsed.first_string("key"), None);
    }
    for value in ["null", "true", "7", r#""north""#, "[]", "[{}]"] {
        let parsed = parse(value);
        assert!(parsed.root_object().is_none());
        assert_eq!(parsed.strings("key").count(), 0);
    }
    let parsed = parse(
        r#"{"Key":"ignored","key":null,"key":9,"key":false,"key":{},"key":[],"key":"","key":"south"}"#,
    );
    assert_eq!(parsed.strings("key").collect::<Vec<_>>(), ["", "south"]);
    assert_eq!(parsed.first_string("key"), Some(""));
    assert_eq!(parsed.strings("Key").collect::<Vec<_>>(), ["ignored"]);
    let parsed = parse(r#"{"key":17,"key":"north","key":"south"}"#);
    assert_eq!(parsed.first_string("key"), Some("north"));
}

#[test]
fn complete_syntax_is_eager_but_numeric_conversion_is_explicit() {
    let parsed = parse(r#"{"extra":{"number":1e1000},"key":"north"}"#);
    assert_eq!(parsed.first_string("key"), Some("north"));
    let extra = parsed.root().members_named("extra").next().unwrap();
    let number = extra.members_named("number").next().unwrap();
    assert_eq!(number.kind(), JsonSourceKind::Number);
    assert_eq!(number.number_text(), Some("1e1000"));
    assert!(number.as_decimal_number().is_err());
    for value in [
        "",
        "{} {}",
        "{key:1}",
        "{\"extra\":NaN}",
        "{\"key\":1,}",
        "{/*x*/}",
    ] {
        assert!(matches!(
            MapForceJsonString::parse(&text(value), Default::default()),
            Err(Error::Json(JsonSourceError::Syntax(_)))
        ));
    }
    for value in [
        Value::Null,
        Value::Bool(true),
        Value::Int(4),
        text("\u{feff}{}"),
        text("{\"key\":\"\u{1}\"}"),
        text("{\"key\":\"\\uD800\"}"),
    ] {
        assert!(MapForceJsonString::parse(&value, Default::default()).is_err());
    }
}

#[test]
fn json_errors_expose_the_original_source_cause() {
    let error = MapForceJsonString::parse(&text("{"), Default::default()).unwrap_err();
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<JsonSourceError>()
        .unwrap();
    assert!(matches!(source, JsonSourceError::Syntax(_)));
    assert!(source.source().is_some());
    let limits = JsonSourceLimits {
        document_bytes: 1,
        ..Default::default()
    };
    let error = MapForceJsonString::parse(&text("{}"), limits).unwrap_err();
    assert!(matches!(
        error.source().unwrap().downcast_ref::<JsonSourceError>(),
        Some(JsonSourceError::DocumentBytes(1))
    ));
    for error in [
        Error::NullInput,
        Error::InputType,
        Error::UnsupportedText,
        Error::TableLimit,
        Error::DuplicateKey,
    ] {
        assert!(error.source().is_none());
    }
}

#[test]
fn parser_limits_use_exact_utf8_bytes_depth_and_value_counts() {
    let input = text(r#"{"key":["é🧭",null]}"#);
    let bytes = match &input {
        Value::String(s) => s.len(),
        _ => unreachable!(),
    };
    let exact = JsonSourceLimits {
        document_bytes: bytes,
        container_depth: 2,
        values: 4,
    };
    assert!(MapForceJsonString::parse(&input, exact).is_ok());
    for limits in [
        JsonSourceLimits {
            document_bytes: bytes - 1,
            ..exact
        },
        JsonSourceLimits {
            container_depth: 1,
            ..exact
        },
        JsonSourceLimits { values: 3, ..exact },
    ] {
        assert!(MapForceJsonString::parse(&input, limits).is_err());
    }
    assert!(
        MapForceJsonString::parse(
            &text("7"),
            JsonSourceLimits {
                document_bytes: 1,
                container_depth: 0,
                values: 1
            }
        )
        .is_ok()
    );
    assert!(
        MapForceJsonString::parse(
            &text("7"),
            JsonSourceLimits {
                document_bytes: 1,
                container_depth: 0,
                values: 0
            }
        )
        .is_err()
    );
    let maximum = JsonSourceLimits::default();
    assert!(MapForceJsonString::parse(&text("{}"), maximum).is_ok());
    for limits in [
        JsonSourceLimits {
            document_bytes: maximum.document_bytes + 1,
            ..maximum
        },
        JsonSourceLimits {
            container_depth: maximum.container_depth + 1,
            ..maximum
        },
        JsonSourceLimits {
            values: maximum.values + 1,
            ..maximum
        },
    ] {
        assert!(matches!(
            MapForceJsonString::parse(&text("{}"), limits),
            Err(Error::Json(JsonSourceError::InvalidLimits))
        ));
    }
}

#[test]
fn fallback_is_exact_and_distinguishes_absence_empty_and_null() {
    let map = MapForceStringInputFallback::new(&[("north", "up"), ("south", "down")]).unwrap();
    for (input, expected) in [
        ("north", "up"),
        ("south", "down"),
        ("east", "east"),
        ("North", "North"),
        (" north", " north"),
        ("north ", "north "),
        ("", ""),
        ("é🧭", "é🧭"),
    ] {
        assert_eq!(map.apply(&text(input)).unwrap(), expected);
    }
    assert_eq!(map.apply_present(None).unwrap(), None);
    assert_eq!(
        map.apply_present(Some(&text(""))).unwrap(),
        Some(String::new())
    );
    assert!(matches!(
        map.apply_present(Some(&Value::Null)),
        Err(Error::NullInput)
    ));
    for value in [Value::Bool(true), Value::Int(3)] {
        assert!(matches!(map.apply(&value), Err(Error::InputType)));
    }
    assert!(MapForceStringInputFallback::new(&[("north", "up"), ("North", "upper")]).is_ok());
    assert!(matches!(
        MapForceStringInputFallback::new(&[("north", "up"), ("north", "down")]),
        Err(Error::DuplicateKey)
    ));
    assert_eq!(
        MapForceStringInputFallback::new(&[])
            .unwrap()
            .apply(&text("north"))
            .unwrap(),
        "north"
    );
}

#[test]
fn fallback_table_limits_count_entries_and_total_utf8_bytes_exactly() {
    let keys = (0..256).map(|i| format!("key{i}")).collect::<Vec<_>>();
    let mut entries = keys
        .iter()
        .map(|s| (s.as_str(), "value"))
        .collect::<Vec<_>>();
    assert!(MapForceStringInputFallback::new(&entries).is_ok());
    entries.push(("another", "value"));
    assert!(matches!(
        MapForceStringInputFallback::new(&entries),
        Err(Error::TableLimit)
    ));
    let below = format!("{}a", "é".repeat(32_767));
    let exact = "é".repeat(32_768);
    let over = format!("{exact}a");
    assert!(MapForceStringInputFallback::new(&[(&below, "")]).is_ok());
    assert!(MapForceStringInputFallback::new(&[(&exact, "")]).is_ok());
    assert!(matches!(
        MapForceStringInputFallback::new(&[(&over, "")]),
        Err(Error::TableLimit)
    ));
    let supplementary = "🧭".repeat(16_384);
    assert!(MapForceStringInputFallback::new(&[("", &supplementary)]).is_ok());
    assert!(matches!(
        MapForceStringInputFallback::new(&[("a", &supplementary)]),
        Err(Error::TableLimit)
    ));
    let value = "é".repeat(32_764);
    assert!(MapForceStringInputFallback::new(&[("north", &value), ("s", "é")]).is_ok());
    assert!(matches!(
        MapForceStringInputFallback::new(&[("north", &value), ("s", "éa")]),
        Err(Error::TableLimit)
    ));
}
