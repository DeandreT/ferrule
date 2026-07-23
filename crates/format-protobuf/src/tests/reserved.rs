use crate::Layout;

use super::{error_text, parse};

#[test]
fn proto2_message_numbers_ranges_and_names_are_accepted() {
    let layout = parse(
        r#"
        syntax = "proto2";
        message Record {
          reserved 2, 4 to 7, 9 to max;
          reserved "legacy_name", 'old_identifier';
          required string current_name = 1;
          optional int32 revision = 3;
          optional bool active = 8;
        }
        "#,
    );
    let record_id = match layout.resolve_message("Record") {
        Ok(id) => id,
        Err(error) => panic!("Record should resolve: {error}"),
    };
    let record = match layout.message(record_id) {
        Some(record) => record,
        None => panic!("resolved Record id should belong to the layout"),
    };
    assert_eq!(
        record
            .fields()
            .iter()
            .map(|field| (field.name(), field.number()))
            .collect::<Vec<_>>(),
        [("current_name", 1), ("revision", 3), ("active", 8)]
    );
}

#[test]
fn proto3_message_and_enum_declarations_are_accepted() {
    let layout = parse(
        r#"
        syntax = "proto3";
        enum State {
          reserved -10 to -2, 2, 4 to max;
          reserved "LEGACY", 'REMOVED';
          UNKNOWN = 0;
          READY = 1;
          FAILED = 3;
        }
        message Envelope {
          reserved 5 to max;
          reserved "legacy";
          State state = 1;
          string current = 2;
        }
        "#,
    );
    let enumeration = match layout.enums().first() {
        Some(enumeration) => enumeration,
        None => panic!("State should be present in the layout"),
    };
    assert_eq!(
        enumeration
            .values()
            .iter()
            .map(|value| (value.name(), value.number()))
            .collect::<Vec<_>>(),
        [("UNKNOWN", 0), ("READY", 1), ("FAILED", 3)]
    );
}

#[test]
fn rejects_invalid_or_conflicting_message_reservations() {
    let cases = [
        (
            r#"message M { reserved 2; optional string value = 2; }"#,
            "uses reserved number 2",
        ),
        (
            r#"message M { reserved 5 to max; optional string value = 42; }"#,
            "uses reserved number 42",
        ),
        (
            r#"message M { reserved "value"; optional string value = 1; }"#,
            "uses a reserved name",
        ),
        (
            r#"message M { reserved 0; optional string value = 1; }"#,
            "outside 1 to 536870911",
        ),
        (
            r#"message M { reserved 536870912; optional string value = 1; }"#,
            "outside 1 to 536870911",
        ),
        (
            r#"message M { reserved 7 to 3; optional string value = 1; }"#,
            "descending reserved range",
        ),
        (
            r#"message M { reserved 2 to 5, 5 to 8; optional string value = 1; }"#,
            "overlapping reserved ranges",
        ),
        (
            r#"message M { reserved 2 to 5; reserved 2 to 5; optional string value = 1; }"#,
            "duplicate reserved ranges",
        ),
        (
            r#"message M { reserved "old"; reserved 'old'; optional string value = 1; }"#,
            "duplicate reserved name",
        ),
        (
            r#"message M { reserved "not-a-field"; optional string value = 1; }"#,
            "must be a protobuf identifier",
        ),
        (
            r#"message M { reserved 2, "old"; optional string value = 1; }"#,
            "expected a number",
        ),
    ];
    for (source, expected) in cases {
        let error = error_text(Layout::parse(source));
        assert!(
            error.contains(expected),
            "`{error}` should contain `{expected}`"
        );
    }
}

#[test]
fn rejects_invalid_or_conflicting_enum_reservations() {
    let cases = [
        (
            r#"syntax = "proto3"; enum E { ZERO = 0; reserved 1; ONE = 1; } message M {}"#,
            "uses reserved number 1",
        ),
        (
            r#"syntax = "proto3"; enum E { ZERO = 0; reserved "ONE"; ONE = 1; } message M {}"#,
            "uses a reserved name",
        ),
        (
            r#"syntax = "proto3"; enum E { ZERO = 0; reserved -4 to -1, 3 to max; FOUR = 4; } message M {}"#,
            "uses reserved number 4",
        ),
        (
            r#"syntax = "proto2"; enum E { reserved -1 to -5; ZERO = 0; } message M {}"#,
            "descending reserved range",
        ),
        (
            r#"syntax = "proto2"; enum E { reserved -5 to 2, 2 to 4; ZERO = 0; } message M {}"#,
            "overlapping reserved ranges",
        ),
        (
            r#"syntax = "proto2"; enum E { reserved 7; reserved 7; ZERO = 0; } message M {}"#,
            "duplicate reserved ranges",
        ),
        (
            r#"syntax = "proto2"; enum E { reserved "OLD", 'OLD'; ZERO = 0; } message M {}"#,
            "duplicate reserved name",
        ),
        (
            r#"syntax = "proto2"; enum E { reserved 2147483648; ZERO = 0; } message M {}"#,
            "invalid reserved enum number",
        ),
        (
            r#"syntax = "proto2"; enum E { reserved -2147483649; ZERO = 0; } message M {}"#,
            "invalid reserved enum number",
        ),
    ];
    for (source, expected) in cases {
        let error = error_text(Layout::parse(source));
        assert!(
            error.contains(expected),
            "`{error}` should contain `{expected}`"
        );
    }
}
