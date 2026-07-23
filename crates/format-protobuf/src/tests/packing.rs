use ir::{Instance, Value};

use crate::Layout;

use super::{encode, error_text, group, parse, scalar};

fn repeated(values: Vec<Value>) -> Instance {
    Instance::Repeated(values.into_iter().map(scalar).collect())
}

#[test]
fn proto3_repeated_packable_fields_encode_packed_unless_disabled() {
    let layout = parse(
        r#"
        syntax = "proto3";
        enum State { UNKNOWN = 0; READY = 1; DONE = 2; }
        message Batch {
          repeated int32 values = 1;
          repeated sint32 deltas = 2 [packed = false];
          repeated State states = 3;
          repeated string labels = 4;
        }
        "#,
    );
    let batch_id = match layout.resolve_message("Batch") {
        Ok(id) => id,
        Err(error) => panic!("Batch should resolve: {error}"),
    };
    let batch = match layout.message(batch_id) {
        Some(batch) => batch,
        None => panic!("resolved Batch id should belong to the layout"),
    };
    assert!(batch.field("values").is_some_and(|field| field.packed()));
    assert!(batch.field("deltas").is_some_and(|field| !field.packed()));
    assert!(batch.field("states").is_some_and(|field| field.packed()));
    assert!(batch.field("labels").is_some_and(|field| !field.packed()));

    let instance = group(vec![
        (
            "values",
            repeated(vec![Value::Int(1), Value::Int(2), Value::Int(300)]),
        ),
        ("deltas", repeated(vec![Value::Int(-1), Value::Int(1)])),
        (
            "states",
            repeated(vec![
                Value::String("READY".to_string()),
                Value::String("DONE".to_string()),
            ]),
        ),
        (
            "labels",
            repeated(vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
            ]),
        ),
    ]);
    assert_eq!(
        encode(&layout, "Batch", &instance),
        [
            0x0a, 0x04, 0x01, 0x02, 0xac, 0x02, // packed int32 values
            0x10, 0x01, 0x10, 0x02, // explicitly unpacked sint32 deltas
            0x1a, 0x02, 0x01, 0x02, // packed enum states
            0x22, 0x01, b'a', 0x22, 0x01, b'b', // strings cannot be packed
        ]
    );
}

#[test]
fn proto2_repeated_packable_fields_stay_unpacked_unless_enabled() {
    let layout = parse(
        r#"
        syntax = "proto2";
        enum State { UNKNOWN = 0; READY = 1; DONE = 2; }
        message Batch {
          repeated int32 values = 1;
          repeated int32 compact_values = 2 [packed = true];
          repeated State states = 3;
          repeated State compact_states = 4 [packed = true];
        }
        "#,
    );
    let batch_id = match layout.resolve_message("Batch") {
        Ok(id) => id,
        Err(error) => panic!("Batch should resolve: {error}"),
    };
    let batch = match layout.message(batch_id) {
        Some(batch) => batch,
        None => panic!("resolved Batch id should belong to the layout"),
    };
    assert!(batch.field("values").is_some_and(|field| !field.packed()));
    assert!(
        batch
            .field("compact_values")
            .is_some_and(|field| field.packed())
    );
    assert!(batch.field("states").is_some_and(|field| !field.packed()));
    assert!(
        batch
            .field("compact_states")
            .is_some_and(|field| field.packed())
    );

    let instance = group(vec![
        ("values", repeated(vec![Value::Int(1), Value::Int(2)])),
        (
            "compact_values",
            repeated(vec![Value::Int(1), Value::Int(2)]),
        ),
        (
            "states",
            repeated(vec![
                Value::String("READY".to_string()),
                Value::String("DONE".to_string()),
            ]),
        ),
        (
            "compact_states",
            repeated(vec![
                Value::String("READY".to_string()),
                Value::String("DONE".to_string()),
            ]),
        ),
    ]);
    assert_eq!(
        encode(&layout, "Batch", &instance),
        [
            0x08, 0x01, 0x08, 0x02, // default-unpacked int32 values
            0x12, 0x02, 0x01, 0x02, // explicitly packed int32 values
            0x18, 0x01, 0x18, 0x02, // default-unpacked enum states
            0x22, 0x02, 0x01, 0x02, // explicitly packed enum states
        ]
    );
}

#[test]
fn proto3_default_packing_excludes_maps_oneofs_and_non_packable_scalars() {
    let layout = parse(
        r#"
        syntax = "proto3";
        message M {
          repeated string labels = 1;
          repeated bytes payloads = 2;
          oneof choice { int32 count = 3; }
          map<string, int32> values = 4;
        }
        "#,
    );
    let message_id = match layout.resolve_message("M") {
        Ok(id) => id,
        Err(error) => panic!("M should resolve: {error}"),
    };
    let message = match layout.message(message_id) {
        Some(message) => message,
        None => panic!("resolved M id should belong to the layout"),
    };
    for name in ["labels", "payloads", "count", "values"] {
        assert!(message.field(name).is_some_and(|field| !field.packed()));
    }

    let cases = [
        (
            r#"syntax = "proto3"; message M { repeated string labels = 1 [packed = true]; }"#,
            "not a repeated numeric, bool, or enum field",
        ),
        (
            r#"syntax = "proto3"; message M { repeated string labels = 1 [packed = false]; }"#,
            "not a repeated numeric, bool, or enum field",
        ),
        (
            r#"syntax = "proto3"; message M { repeated bytes payloads = 1 [packed = false]; }"#,
            "not a repeated numeric, bool, or enum field",
        ),
        (
            r#"syntax = "proto3"; message M { oneof choice { int32 count = 1 [packed = true]; } }"#,
            "cannot use packed encoding",
        ),
        (
            r#"syntax = "proto3"; message M { oneof choice { int32 count = 1 [packed = false]; } }"#,
            "cannot use packed encoding",
        ),
        (
            r#"syntax = "proto3"; message M { int32 count = 1 [packed = false]; }"#,
            "not a repeated numeric, bool, or enum field",
        ),
        (
            r#"syntax = "proto3"; message M { map<string, int32> values = 1 [packed = false]; }"#,
            "map field cannot use option `packed`",
        ),
        (
            r#"syntax = "proto3"; message M { repeated int32 values = 1 [packed = true, packed = false]; }"#,
            "declares `packed` more than once",
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
