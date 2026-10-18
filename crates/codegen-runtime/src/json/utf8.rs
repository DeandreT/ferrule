use std::error::Error;

use ir::{Instance, Value};

use super::{
    JsonBoundaryError, MAX_JSON_DOCUMENT_BYTES, parse_json, parse_json_bytes,
    parse_json_bytes_with_utf8_cause,
};

const SCHEMA: &str = r#"{"name":"Root","kind":{"kind":"group","children":[{"name":"Value","kind":{"kind":"scalar","ty":"string"}}]}}"#;

#[test]
fn utf8_cause_retains_nonzero_raw_offsets_partial_sequences_and_source_identity() {
    for (document, valid_up_to, error_len) in [
        (&b"{\"Value\":\"ok\xff\"}"[..], 12, Some(1)),
        (&b"{\"Value\":\"ok\xe2\x82X\"}"[..], 12, Some(2)),
        (&b"{\"Value\":\"ok\xf0\x9f\x92"[..], 12, None),
        (&b"\xef\xbb\xbf\xff"[..], 3, Some(1)),
    ] {
        let original = std::str::from_utf8(document).expect_err("malformed UTF-8 fixture");
        let error = parse_json_bytes_with_utf8_cause(SCHEMA, document).expect_err("invalid UTF-8");
        let JsonBoundaryError::InvalidUtf8(inner) = &error else {
            panic!("expected typed UTF-8 cause, got {error:?}");
        };
        assert_eq!(*inner, original);
        assert_eq!(inner.valid_up_to(), valid_up_to);
        assert_eq!(inner.error_len(), error_len);
        let source = error.source().expect("UTF-8 error source");
        let source = source
            .downcast_ref::<std::str::Utf8Error>()
            .expect("original source type");
        assert!(
            std::ptr::eq(inner, source),
            "source borrows the stored original error"
        );
        assert!(source.source().is_none());
        assert_eq!(
            error.to_string(),
            format!("JSON input is invalid: document is not UTF-8: {original}")
        );
        // Raw UTF-8 decoding also precedes embedded schema validation.
        assert_eq!(
            parse_json_bytes_with_utf8_cause("{}", document),
            Err(JsonBoundaryError::InvalidUtf8(original))
        );
    }
}

#[test]
fn legacy_utf8_errors_keep_frozen_messages_and_no_source() {
    for (document, expected) in [
        (
            &b"{\"Value\":\"ok\xff\"}"[..],
            "document is not UTF-8: invalid utf-8 sequence of 1 bytes from index 12",
        ),
        (
            &b"{\"Value\":\"ok\xf0\x9f\x92"[..],
            "document is not UTF-8: incomplete utf-8 byte sequence from index 12",
        ),
    ] {
        let error = parse_json_bytes(SCHEMA, document).expect_err("legacy UTF-8 failure");
        assert_eq!(
            error,
            JsonBoundaryError::InvalidInput {
                message: expected.into()
            }
        );
        assert_eq!(
            error.to_string(),
            format!("JSON input is invalid: {expected}")
        );
        assert!(error.source().is_none());
    }
}

#[test]
fn utf8_helper_preserves_json_parser_and_schema_error_profiles() {
    let expected = JsonBoundaryError::InvalidInput {
        message: "json error: EOF while parsing a value at line 1 column 9".into(),
    };
    assert_eq!(
        parse_json_bytes_with_utf8_cause(SCHEMA, b"{\"Value\":"),
        Err(expected)
    );
    for (schema, document) in [
        (SCHEMA, "{\"Value\":"),
        (SCHEMA, "{\"Value\":true}"),
        (SCHEMA, "{\"Extra\":1}"),
        ("{}", "{}"),
    ] {
        let error = parse_json(schema, document).expect_err("legacy parse failure");
        assert!(error.source().is_none());
        assert_eq!(parse_json_bytes(schema, document.as_bytes()), Err(error));
        let error = parse_json_bytes_with_utf8_cause(schema, document.as_bytes())
            .expect_err("same parse failure");
        assert_eq!(error, parse_json(schema, document).unwrap_err());
        assert!(
            error.source().is_none(),
            "valid UTF-8 does not add parser causes"
        );
    }
}

#[test]
fn utf8_helper_preserves_bom_unicode_identity_and_typed_input() {
    let document = "\u{feff} {\"Value\":\"café😀e\u{301}\"}\n";
    let expected = Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("café😀e\u{301}".into())),
        )]
        .into(),
    );
    assert_eq!(
        parse_json_bytes_with_utf8_cause(SCHEMA, document.as_bytes()),
        Ok(expected.clone())
    );
    assert_eq!(parse_json_bytes(SCHEMA, document.as_bytes()), Ok(expected));
}

#[test]
fn input_size_precedes_utf8_and_schema_errors_at_the_byte_boundary() {
    let mut document = vec![b'a'; MAX_JSON_DOCUMENT_BYTES + 1];
    document[0] = 0xff;
    let expected = || JsonBoundaryError::InputTooLarge {
        bytes: MAX_JSON_DOCUMENT_BYTES + 1,
        max: MAX_JSON_DOCUMENT_BYTES,
    };
    let error = parse_json_bytes_with_utf8_cause("{}", &document).expect_err("size first");
    assert_eq!(error, expected());
    assert!(error.source().is_none());
    assert_eq!(parse_json_bytes("{}", &document), Err(expected()));
    // The exact inclusive size is admitted to decoding, not rejected as too large.
    document.truncate(MAX_JSON_DOCUMENT_BYTES);
    assert!(matches!(parse_json_bytes_with_utf8_cause("{}", &document),
        Err(JsonBoundaryError::InvalidUtf8(error))
            if error.valid_up_to() == 0 && error.error_len() == Some(1)));
    let error = parse_json_bytes("{}", &document).expect_err("legacy decoding at exact limit");
    assert_eq!(
        error,
        JsonBoundaryError::InvalidInput {
            message: "document is not UTF-8: invalid utf-8 sequence of 1 bytes from index 0".into(),
        }
    );
    assert!(error.source().is_none());
}
