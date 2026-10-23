use codegen_runtime::{JsonSourceDocument, JsonSourceLimits, parse_native_json_source};
use mapping::{JsonSourceProperty, JsonSourceShape};

fn shape() -> JsonSourceShape {
    JsonSourceShape::Object {
        properties: vec![JsonSourceProperty {
            name: "v".into(),
            shape: JsonSourceShape::String,
        }],
    }
}
fn value(bytes: &[u8]) -> String {
    parse_native_json_source("doc", bytes, &shape(), JsonSourceLimits::default())
        .unwrap()
        .root()
        .members_named("v")
        .next()
        .unwrap()
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn bom_decoding_and_nonflushing_tail_are_explicit() {
    let text = "{\"v\":\"é😀\"}";
    assert_eq!(value(text.as_bytes()), "é😀");
    let mut utf8 = b"\xef\xbb\xbf".to_vec();
    utf8.extend(text.as_bytes());
    assert_eq!(value(&utf8), "é😀");
    for big in [false, true] {
        let mut bytes = if big {
            vec![0xfe, 0xff]
        } else {
            vec![0xff, 0xfe]
        };
        for word in text.encode_utf16() {
            bytes.extend(if big {
                word.to_be_bytes()
            } else {
                word.to_le_bytes()
            });
        }
        assert_eq!(value(&bytes), "é😀");
        let mut bytes = if big {
            vec![0, 0, 0xfe, 0xff]
        } else {
            vec![0xff, 0xfe, 0, 0]
        };
        for word in text.chars().map(u32::from) {
            bytes.extend(if big {
                word.to_be_bytes()
            } else {
                word.to_le_bytes()
            });
        }
        assert_eq!(value(&bytes), "é😀");
    }
    let mut tail = text.as_bytes().to_vec();
    tail.extend([0xe2, 0x82]);
    assert_eq!(value(&tail), "é😀");
    assert!(JsonSourceDocument::parse("doc", &tail).is_err());
}

#[test]
fn native_string_rules_preserve_controls_and_replace_unpaired_escapes() {
    for (raw, expected) in [
        ("{\"v\":\"A\nB\"}", "A\nB"),
        (r#"{"v":"\ud800"}"#, "�"),
        (r#"{"v":"\udfff"}"#, "�"),
        (r#"{"v":"\ud83d\ude00"}"#, "😀"),
        ("{\"v\":\"A\0B\"}", "A\0B"),
    ] {
        assert_eq!(value(raw.as_bytes()), expected);
    }
    let document = parse_native_json_source(
        "doc",
        br#"{"v":"first","v":"\ud800","n":1e999}"#,
        &shape(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(document.root().members_named("v").count(), 2);
    assert_eq!(
        document
            .root()
            .members_named("n")
            .next()
            .unwrap()
            .number_text(),
        Some("1e999")
    );
    for raw in [
        r#"{"v":"x",}"#,
        r#"{"n":01}"#,
        r#"{"n":+1}"#,
        r#"{"v":"\q"}"#,
        "{}\0",
        "{}{}",
    ] {
        assert!(
            parse_native_json_source("doc", raw.as_bytes(), &shape(), Default::default()).is_err()
        );
    }
}

#[test]
fn native_input_does_not_admit_non_ascii_projection_names_or_unbounded_limits() {
    let shape = JsonSourceShape::Object {
        properties: vec![JsonSourceProperty {
            name: "é".into(),
            shape: JsonSourceShape::String,
        }],
    };
    assert!(parse_native_json_source("doc", b"{}", &shape, Default::default()).is_err());
    assert!(
        parse_native_json_source(
            "doc",
            b"{}",
            &JsonSourceShape::Null,
            JsonSourceLimits {
                document_bytes: usize::MAX,
                ..Default::default()
            }
        )
        .is_err()
    );
}
