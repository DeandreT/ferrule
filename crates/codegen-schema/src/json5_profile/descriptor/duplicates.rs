use std::error::Error;

use ir::{ScalarType, SchemaNode};
use serde_json::Value;

use super::super::{Json5ProfileError, Json5ProfileResource, MAX_DESCRIPTOR_BYTES, decode_schema};

const EMPTY: &str = r#"{"name":"S","kind":{"kind":"group","children":[]}}"#;

fn variants(payload: &str) -> [String; 2] {
    [
        payload.to_owned(),
        format!("{}{}", crate::V2_PREFIX, payload),
    ]
}

fn duplicate(payload: &str, expected: &str) {
    for source in variants(payload) {
        let original = decode_schema(&source);
        eprintln!("original duplicate source={source:?} result={original:?}");
        assert!(
            matches!(original, Err(Json5ProfileError::DuplicateDescriptorField { field }) if field == expected)
        );
    }
}

#[test]
fn plain_and_versioned_hidden_node_kind_and_nondefault_members_refuse() {
    for (payload, expected) in [
        (
            r#"{"name":"S","kind":{"kind":"group","children":[],"future_profile":true},"kind":{"kind":"group","children":[]}}"#,
            "kind",
        ),
        (
            r#"{"name":"S","repeating":true,"repeating":false,"kind":{"kind":"group","children":[]}}"#,
            "repeating",
        ),
        (
            r#"{"name":"S","kind":{"kind":"group","children":[],"future_profile":true},"ki\u006ed":{"kind":"group","children":[]}}"#,
            "kind",
        ),
        (
            r#"{"name":"S","kind":{"kind":"group","children":[{"name":"n","nullable":false,"nullable":true,"kind":{"kind":"scalar","ty":"string"}}]}}"#,
            "nullable",
        ),
        (
            r#"{"name":"S","kind":{"kind":"group","children":[{"name":"n","kind":{"kind":"scalar","ty":"int","ty":"string"}}]}}"#,
            "ty",
        ),
        (
            r#"{"name":"S","kind":{"kind":"group","children":[],"children":[]}}"#,
            "children",
        ),
        (
            r#"{"name":"S","name":"S","kind":{"kind":"group","children":[]}}"#,
            "name",
        ),
    ] {
        duplicate(payload, expected);
    }
    // The three reached source witnesses are unchanged ordinary V2 controls.
    let hidden = [
        r#"{"name":"S","kind":{"kind":"group","children":[],"future_profile":true},"kind":{"kind":"group","children":[]}}"#,
        r#"{"name":"S","repeating":true,"repeating":false,"kind":{"kind":"group","children":[]}}"#,
        r#"{"name":"S","kind":{"kind":"group","children":[],"future_profile":true},"ki\u006ed":{"kind":"group","children":[]}}"#,
    ];
    for payload in hidden {
        let source = format!("{}{}", crate::V2_PREFIX, payload);
        let ordinary = crate::decode(&source, MAX_DESCRIPTOR_BYTES);
        eprintln!("original unchanged ordinary witness={source:?} result={ordinary:?}");
        assert_eq!(ordinary.unwrap(), SchemaNode::group("S", Vec::new()));
    }
}

#[test]
fn decoded_unicode_aliases_and_nested_objects_have_local_key_identity() {
    duplicate(
        r#"{"name":"S","kind":{"kind":"group","children":[]},"雪":1,"\u96ea":2}"#,
        "雪",
    );
    duplicate(
        r#"{"name":"S","kind":{"kind":"group","children":[],"required":[],"requ\u0069red":[]}}"#,
        "required",
    );
    let payload = r#"{"name":"S","kind":{"kind":"group","children":[{"name":"one","kind":{"kind":"scalar","ty":"string"}},{"name":"two","kind":{"kind":"scalar","ty":"string"}}]}}"#;
    for source in variants(payload) {
        let original = decode_schema(&source);
        eprintln!("original unique sibling source={source:?} result={original:?}");
        assert_eq!(
            original.unwrap(),
            SchemaNode::group(
                "S",
                vec![
                    SchemaNode::scalar("one", ScalarType::String),
                    SchemaNode::scalar("two", ScalarType::String)
                ]
            )
        );
    }
    // Arrays are scanner controls only, not admitted schema roots.
    let payload = r#"[{"x":1},{"x":2}]"#;
    let strict = serde_json::from_str::<Value>(payload);
    eprintln!("original pure scanner strict={strict:?}");
    assert!(strict.is_ok());
    let observed = super::keys::validate(payload);
    eprintln!("original per-object scanner={observed:?}");
    assert!(observed.is_ok());
}

#[test]
fn complete_strings_skip_colons_braces_arrays_quotes_and_backslashes() {
    let payload =
        r#"{"name":"S: { } [ ] , \"kind\": \"quoted\" \\","kind":{"kind":"group","children":[]}}"#;
    for source in variants(payload) {
        let original = decode_schema(&source);
        eprintln!("original punctuation string source={source:?} result={original:?}");
        assert_eq!(
            original.unwrap(),
            SchemaNode::group(r#"S: { } [ ] , "kind": "quoted" \"#, Vec::new())
        );
    }
    for payload in [
        r#"{"x":"\"x\": { [ ] }, \\","y":[{"x":"comma, colon:"}]}"#,
        r#"{"x":"\u0022\u005c{}:,[]","y":true}"#,
    ] {
        let strict = serde_json::from_str::<Value>(payload);
        eprintln!("original scanner string strict={strict:?}");
        assert!(strict.is_ok());
        let observed = super::keys::validate(payload);
        eprintln!("original scanner strings={observed:?}");
        assert!(observed.is_ok());
    }
}

#[test]
fn complete_strict_parse_errors_and_descriptor_byte_limit_still_dominate() {
    for payload in [
        r#"{"name":"S","name":"S",}"#.to_owned(),
        r#"{"name":"unterminated}"#.to_owned(),
        format!("{}0{}", "[".repeat(128), "]".repeat(128)),
    ] {
        let strict = serde_json::from_str::<Value>(&payload);
        eprintln!("original malformed strict={strict:?}");
        let expected = strict.unwrap_err();
        for source in variants(&payload) {
            let selected = decode_schema(&source);
            eprintln!("original malformed optional={selected:?}");
            assert!(selected.as_ref().unwrap_err().source().is_some());
            match selected {
                Err(Json5ProfileError::DescriptorSyntax(actual)) => {
                    assert_eq!(actual.classify(), expected.classify());
                    assert_eq!(
                        (actual.line(), actual.column()),
                        (expected.line(), expected.column())
                    );
                    assert_eq!(actual.to_string(), expected.to_string());
                }
                other => panic!(
                    "complete initial strict parsing must dominate key inspection: {other:?}"
                ),
            }
        }
    }
    let oversized = " ".repeat(MAX_DESCRIPTOR_BYTES + 1);
    let selected = decode_schema(&oversized);
    eprintln!("original oversized={selected:?}");
    assert!(matches!(
        selected,
        Err(Json5ProfileError::Limit {
            resource: Json5ProfileResource::DescriptorBytes,
            requested: 1_048_577,
            max: 1_048_576
        })
    ));
    let padded = format!(
        "{}{}",
        EMPTY,
        " ".repeat(MAX_DESCRIPTOR_BYTES - EMPTY.len())
    );
    let selected = decode_schema(&padded);
    eprintln!("original exact bytes result={selected:?}");
    assert_eq!(selected.unwrap(), SchemaNode::group("S", Vec::new()));
}

#[test]
fn decoded_key_limit_precedes_duplicate_error_and_counts_utf8_not_escape_width() {
    let exact = "a".repeat(4096);
    let escaped = "\\u0061".repeat(4096);
    let source = format!(
        "{{\"name\":\"S\",\"kind\":{{\"kind\":\"group\",\"children\":[]}},\"{exact}\":0,\"{escaped}\":1}}"
    );
    duplicate(&source, &exact);
    let over = "雪".repeat(1365) + "xy";
    assert_eq!(over.len(), 4097);
    let quoted = serde_json::to_string(&over).unwrap();
    let source = format!(
        "{{\"name\":\"S\",\"kind\":{{\"kind\":\"group\",\"children\":[]}}, {quoted}:0, {quoted}:1}}"
    );
    for source in variants(&source) {
        let observed = decode_schema(&source);
        eprintln!("original oversized duplicate key={observed:?}");
        assert!(matches!(
            observed,
            Err(Json5ProfileError::Limit {
                resource: Json5ProfileResource::NameLength,
                requested: 4097,
                max: 4096
            })
        ));
    }
}
