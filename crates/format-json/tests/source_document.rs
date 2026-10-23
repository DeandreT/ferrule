use format_json::source_document::{
    JsonSourceDocument, JsonSourceError, JsonSourceKind, JsonSourceLimits, JsonSourceValue,
};
use serde_json::{Value, json};

fn view(value: &JsonSourceValue) -> Value {
    match value.kind() {
        JsonSourceKind::Null => json!(["null"]),
        JsonSourceKind::Boolean => json!(["boolean", value.as_bool().unwrap()]),
        JsonSourceKind::Number => json!(["number", value.number_text().unwrap()]),
        JsonSourceKind::String => json!(["string", value.as_str().unwrap()]),
        JsonSourceKind::Array => {
            json!(["array", value.items().iter().map(view).collect::<Vec<_>>()])
        }
        JsonSourceKind::Object => json!([
            "object",
            value
                .members()
                .iter()
                .map(|(name, value)| json!([name, view(value)]))
                .collect::<Vec<_>>()
        ]),
    }
}

#[test]
fn complete_literal_source_corpus() {
    let corpus: Value =
        serde_json::from_str(include_str!("../../../conformance/json-source/cases.json")).unwrap();
    for case in corpus["valid"].as_array().unwrap() {
        let document = JsonSourceDocument::parse(
            "  supplied/雪.json  ",
            case["input"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
        assert_eq!(document.identity(), "  supplied/雪.json  ");
        assert_eq!(view(document.root()), case["expected"], "{}", case["id"]);
        assert_eq!(document.clone(), document);
    }
    for input in corpus["invalid"].as_array().unwrap() {
        assert!(
            matches!(
                JsonSourceDocument::parse("fixture", input.as_str().unwrap().as_bytes()),
                Err(JsonSourceError::Syntax(_))
            ),
            "{input}"
        );
    }
}

#[test]
fn exact_selection_preserves_occurrences_and_never_coerces() {
    let document = JsonSourceDocument::parse(
        "doc",
        br#"{"v":"12","v":12,"v":null,"V":"upper","v":false,"v":[1],"v":{"x":2}}"#,
    )
    .unwrap();
    let values = document.root().members_named("v").collect::<Vec<_>>();
    assert_eq!(values.len(), 6);
    assert_eq!(values[0].as_str(), Some("12"));
    assert_eq!(values[0].number_text(), None);
    assert_eq!(values[1].number_text(), Some("12"));
    assert_eq!(values[1].as_str(), None);
    assert_eq!(values[2].kind(), JsonSourceKind::Null);
    assert_eq!(values[2].as_str(), None);
    assert_eq!(values[3].as_bool(), Some(false));
    assert!(values[0].items().is_empty());
    assert!(values[4].members().is_empty());
    assert_eq!(values[4].items()[0].number_text(), Some("1"));
    assert_eq!(
        values[5].members_named("x").next().unwrap().number_text(),
        Some("2")
    );
    assert_eq!(document.root().members_named("missing").count(), 0);
}

#[test]
fn explicit_decimal_number_selection_matches_literal_contract() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../conformance/json-source/decimal-selection.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let text = case["input"].as_str().unwrap();
        let document = JsonSourceDocument::parse("number", text.as_bytes()).unwrap();
        let original = document.root().number_text().map(str::to_owned);
        let result = document.root().as_decimal_number();
        if case.get("error").is_some() {
            assert_eq!(result, Err(ir::DecimalError::Overflow), "{text}");
        } else {
            assert_eq!(
                result.unwrap().map(|value| value.to_string()),
                case["value"].as_str().map(str::to_owned),
                "{text}"
            );
        }
        assert_eq!(document.root().number_text(), original.as_deref());
    }
}

#[test]
fn numeric_conversion_is_demanded_per_duplicate_without_mutating_source() {
    let document =
        JsonSourceDocument::parse("doc", br#"{"n":1.2300,"n":1e999,"n":"1e999","n":3}"#).unwrap();
    let mut values = document.root().members_named("n");
    assert_eq!(
        values
            .next()
            .unwrap()
            .as_decimal_number()
            .unwrap()
            .unwrap()
            .to_string(),
        "1.23"
    );
    let overflow = values.next().unwrap();
    assert_eq!(overflow.number_text(), Some("1e999"));
    assert_eq!(
        overflow.as_decimal_number(),
        Err(ir::DecimalError::Overflow)
    );
    assert_eq!(values.next().unwrap().as_decimal_number(), Ok(None));
    assert_eq!(
        values
            .next()
            .unwrap()
            .as_decimal_number()
            .unwrap()
            .unwrap()
            .to_string(),
        "3"
    );
    assert!(values.next().is_none());
    let text = "1".repeat(ir::MAX_DECIMAL_LEXICAL_BYTES + 1);
    let document = JsonSourceDocument::parse("large", text.as_bytes()).unwrap();
    assert_eq!(document.root().number_text(), Some(text.as_str()));
    assert_eq!(
        document.root().as_decimal_number(),
        Err(ir::DecimalError::InputTooLong)
    );
}

#[test]
fn admission_limits_encoding_and_complete_document_failures() {
    let limits = JsonSourceLimits {
        document_bytes: 7,
        container_depth: 1,
        values: 2,
    };
    assert!(JsonSourceDocument::parse_with_limits("doc", br#"{"x":0}"#, limits).is_ok());
    assert!(matches!(
        JsonSourceDocument::parse_with_limits("doc", br#"{"x":0} "#, limits),
        Err(JsonSourceError::DocumentBytes(7))
    ));
    assert!(matches!(
        JsonSourceDocument::parse_with_limits("doc", b"[[]]", limits),
        Err(JsonSourceError::ContainerDepth(1))
    ));
    assert!(matches!(
        JsonSourceDocument::parse_with_limits("doc", b"[0,1]", limits),
        Err(JsonSourceError::Values(2))
    ));
    assert!(matches!(
        JsonSourceDocument::parse_with_limits(
            "doc",
            b"null",
            JsonSourceLimits {
                values: 0,
                ..limits
            }
        ),
        Err(JsonSourceError::Values(0))
    ));
    assert!(
        JsonSourceDocument::parse_with_limits(
            "doc",
            br#""[{}]""#,
            JsonSourceLimits {
                container_depth: 0,
                ..limits
            }
        )
        .is_ok()
    );
    for bytes in [
        &b"\xff"[..],
        &b"\xef\xbb\xbf\xc0\xaf"[..],
        &b"\xed\xa0\x80"[..],
    ] {
        assert!(matches!(
            JsonSourceDocument::parse("doc", bytes),
            Err(JsonSourceError::Utf8(_))
        ));
    }
    for name in ["".to_string(), "a\0b".to_string(), "雪".repeat(1366)] {
        assert!(matches!(
            JsonSourceDocument::parse(&name, b"null"),
            Err(JsonSourceError::InvalidIdentity)
        ));
    }
    assert!(JsonSourceDocument::parse(&"x".repeat(4096), b"null").is_ok());
    for depth in [64, 65, 129] {
        let input = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let result = JsonSourceDocument::parse("doc", input.as_bytes());
        if depth == 64 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(JsonSourceError::ContainerDepth(64))));
        }
    }
    assert!(matches!(
        JsonSourceDocument::parse_with_limits(
            "doc",
            b"null",
            JsonSourceLimits {
                container_depth: 65,
                ..limits
            }
        ),
        Err(JsonSourceError::InvalidLimits)
    ));
}
