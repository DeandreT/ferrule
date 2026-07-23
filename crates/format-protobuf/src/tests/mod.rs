use ir::{Instance, Value};

use crate::{
    Cardinality, DefaultValue, FieldType, Layout, MAX_SCHEMA_BYTES, ProtobufError, ScalarType,
    from_slice, to_ir_schema, to_vec,
};

const CONTACT_SCHEMA: &str = r#"
    // Omitting syntax selects proto2, as required by legacy schemas.
    package example.directory;

    message Contact {
      required string name = 1;
      required int32 id = 2;
      optional Kind kind = 3 [default = HOME];

      enum Kind {
        MOBILE = 0;
        HOME = 1;
        WORK = 2;
      }

      message Phone {
        required string number = 1;
        optional bool primary = 2;
      }

      repeated Phone phones = 4;
    }

    message Directory {
      repeated Contact contacts = 1;
      optional string label = 2;
    }
"#;

fn parse(source: &str) -> Layout {
    match Layout::parse(source) {
        Ok(layout) => layout,
        Err(error) => panic!("schema should parse: {error}"),
    }
}

fn encode(layout: &Layout, root: &str, instance: &Instance) -> Vec<u8> {
    match to_vec(layout, root, instance) {
        Ok(bytes) => bytes,
        Err(error) => panic!("instance should encode: {error}"),
    }
}

fn decode(layout: &Layout, root: &str, bytes: &[u8]) -> Instance {
    match from_slice(layout, root, bytes) {
        Ok(instance) => instance,
        Err(error) => panic!("message should decode: {error}"),
    }
}

fn error_text<T>(result: Result<T, ProtobufError>) -> String {
    match result {
        Ok(_) => panic!("operation should fail"),
        Err(error) => error.to_string(),
    }
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

#[test]
fn rejects_oversized_and_excessively_nested_schemas() {
    let oversized = " ".repeat(MAX_SCHEMA_BYTES + 1);
    assert!(error_text(Layout::parse(&oversized)).contains("byte limit"));

    let mut nested = String::new();
    for index in 0..129 {
        nested.push_str(&format!("message M{index} {{ "));
    }
    nested.push_str(&"}".repeat(129));
    assert!(error_text(Layout::parse(&nested)).contains("nesting exceeds"));
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

#[test]
fn parses_and_resolves_nested_messages_enums_and_defaults() {
    let layout = parse(CONTACT_SCHEMA);
    assert_eq!(layout.package(), Some("example.directory"));

    let contact_id = match layout.resolve_message("example.directory.Contact") {
        Ok(id) => id,
        Err(error) => panic!("contact should resolve: {error}"),
    };
    let contact = match layout.message(contact_id) {
        Some(message) => message,
        None => panic!("resolved message id should exist"),
    };
    assert_eq!(contact.name(), "Contact");
    assert_eq!(contact.fields().len(), 4);
    let kind = match contact.field("kind") {
        Some(field) => field,
        None => panic!("kind field should exist"),
    };
    assert_eq!(kind.cardinality(), Cardinality::Optional);
    assert_eq!(kind.default(), Some(&DefaultValue::Enum(1)));
    let FieldType::Enum(kind_id) = kind.ty() else {
        panic!("kind should resolve to an enum");
    };
    let enumeration = match layout.enumeration(kind_id) {
        Some(enumeration) => enumeration,
        None => panic!("resolved enum id should exist"),
    };
    assert_eq!(enumeration.full_name(), "example.directory.Contact.Kind");
    assert_eq!(
        enumeration.value_by_name("WORK").map(|v| v.number()),
        Some(2)
    );

    let phones = match contact.field("phones") {
        Some(field) => field,
        None => panic!("phones field should exist"),
    };
    let FieldType::Message(phone_id) = phones.ty() else {
        panic!("phones should resolve to a message");
    };
    assert_eq!(
        layout.message(phone_id).map(|message| message.full_name()),
        Some("example.directory.Contact.Phone")
    );
}

#[test]
fn encodes_nested_repeated_messages_and_enum_names() {
    let layout = parse(CONTACT_SCHEMA);
    let instance = group(vec![
        (
            "contacts",
            Instance::Repeated(vec![group(vec![
                ("name", scalar(Value::String("Ada".to_string()))),
                ("id", scalar(Value::Int(150))),
                ("kind", scalar(Value::String("WORK".to_string()))),
                (
                    "phones",
                    Instance::Repeated(vec![group(vec![
                        ("number", scalar(Value::String("555".to_string()))),
                        ("primary", scalar(Value::Bool(true))),
                    ])]),
                ),
            ])]),
        ),
        ("label", scalar(Value::String("team".to_string()))),
    ]);

    assert_eq!(
        encode(&layout, "Directory", &instance),
        vec![
            0x0a, 0x13, 0x0a, 0x03, b'A', b'd', b'a', 0x10, 0x96, 0x01, 0x18, 0x02, 0x22, 0x07,
            0x0a, 0x03, b'5', b'5', b'5', 0x10, 0x01, 0x12, 0x04, b't', b'e', b'a', b'm',
        ]
    );
}

#[test]
fn encodes_every_scalar_wire_representation_and_packed_values() {
    let layout = parse(
        r#"
        syntax = "proto2";
        message Wires {
          required double d = 1;
          required float f = 2;
          required int32 i32 = 3;
          required int64 i64 = 4;
          required uint32 u32 = 5;
          required uint64 u64 = 6;
          required sint32 si32 = 7;
          required sint64 si64 = 8;
          required fixed32 fx32 = 9;
          required fixed64 fx64 = 10;
          required sfixed32 sfx32 = 11;
          required sfixed64 sfx64 = 12;
          required bool flag = 13;
          required string text = 14;
          required bytes data = 15;
          repeated int32 packed_numbers = 16 [packed = true];
        }
        "#,
    );
    let instance = group(vec![
        ("d", scalar(Value::Float(1.5))),
        ("f", scalar(Value::Float(2.5))),
        ("i32", scalar(Value::Int(-1))),
        ("i64", scalar(Value::Int(300))),
        ("u32", scalar(Value::Int(4))),
        ("u64", scalar(Value::Int(5))),
        ("si32", scalar(Value::Int(-2))),
        ("si64", scalar(Value::Int(-3))),
        ("fx32", scalar(Value::Int(0x0102_0304))),
        ("fx64", scalar(Value::Int(0x0102_0304_0506_0708))),
        ("sfx32", scalar(Value::Int(-2))),
        ("sfx64", scalar(Value::Int(-3))),
        ("flag", scalar(Value::Bool(true))),
        ("text", scalar(Value::String("x".to_string()))),
        ("data", scalar(Value::String("yz".to_string()))),
        (
            "packed_numbers",
            Instance::Repeated(vec![scalar(Value::Int(1)), scalar(Value::Int(2))]),
        ),
    ]);

    assert_eq!(
        encode(&layout, "Wires", &instance),
        vec![
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf8, 0x3f, 0x15, 0x00, 0x00, 0x20, 0x40,
            0x18, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x20, 0xac, 0x02,
            0x28, 0x04, 0x30, 0x05, 0x38, 0x03, 0x40, 0x05, 0x4d, 0x04, 0x03, 0x02, 0x01, 0x51,
            0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x5d, 0xfe, 0xff, 0xff, 0xff, 0x61,
            0xfd, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x68, 0x01, 0x72, 0x01, b'x', 0x7a,
            0x02, b'y', b'z', 0x82, 0x01, 0x02, 0x01, 0x02,
        ]
    );
}

#[test]
fn optional_null_is_absent_and_mapped_sequences_are_repeated() {
    let layout = parse(CONTACT_SCHEMA);
    let instance = group(vec![
        (
            "contacts",
            Instance::MappedSequence(vec![group(vec![
                ("name", scalar(Value::String("N".to_string()))),
                ("id", scalar(Value::Int(1))),
                ("kind", scalar(Value::Null)),
            ])]),
        ),
        ("label", scalar(Value::Null)),
    ]);
    assert_eq!(
        encode(&layout, ".example.directory.Directory", &instance),
        vec![0x0a, 0x05, 0x0a, 0x01, b'N', 0x10, 0x01]
    );
}

#[test]
fn string_fields_lexically_coerce_finite_scalars() {
    let layout = parse(
        "message Text { required string number = 1; required string flag = 2; required string decimal = 3; }",
    );
    let instance = group(vec![
        ("number", scalar(Value::Int(42))),
        ("flag", scalar(Value::Bool(true))),
        ("decimal", scalar(Value::Float(1.5))),
    ]);

    assert_eq!(
        encode(&layout, "Text", &instance),
        vec![
            0x0a, 0x02, b'4', b'2', 0x12, 0x04, b't', b'r', b'u', b'e', 0x1a, 0x03, b'1', b'.',
            b'5',
        ]
    );
}

#[test]
fn rejects_invalid_schemas_before_encoding() {
    let cases = [
        (
            r#"syntax = "proto3"; message M { required string value = 1; }"#,
            "cannot be `required`",
        ),
        (
            r#"syntax = "proto3"; message M { string value = 1 [default = "x"]; }"#,
            "cannot declare explicit defaults",
        ),
        (
            r#"syntax = "proto3"; message M { optional int32 value = 1 [default = 1]; }"#,
            "cannot declare explicit defaults",
        ),
        (
            r#"syntax = "proto3"; enum E { ONE = 1; ZERO = 0; } message M { E value = 1; }"#,
            "must declare zero as its first value",
        ),
        (
            "message M { required string a = 1; optional int32 b = 1; }",
            "duplicate field number",
        ),
        ("message M { required Missing value = 1; }", "unknown type"),
        (
            "message M { repeated string value = 1 [packed=true]; }",
            "packed encoding",
        ),
        (
            "message M { repeated int32 value = 1 [default=1]; }",
            "non-optional field",
        ),
        (
            "message M { required int32 value = 1 [default=1]; }",
            "non-optional field",
        ),
        (
            "message M { required int32 value = 19000; }",
            "invalid or reserved number",
        ),
        (
            r#"syntax = "proto3"; message M { optional repeated string value = 1; }"#,
            "more than one cardinality label",
        ),
        (
            "message M { required optional string value = 1; }",
            "more than one cardinality label",
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
fn proto3_optional_preserves_scalar_and_message_presence() {
    let layout = parse(
        r#"
        syntax = "proto3";
        message Child { optional string name = 1; }
        enum State { UNKNOWN = 0; READY = 1; }
        message Root {
          optional int32 tracked = 1;
          int32 implicit_count = 2;
          optional string label = 3;
          optional State state = 4;
          optional Child child = 5;
        }
        "#,
    );
    let root_id = layout.resolve_message("Root").unwrap();
    let root = layout.message(root_id).unwrap();
    assert_eq!(
        root.field("tracked").unwrap().cardinality(),
        Cardinality::Optional
    );
    assert_eq!(root.field("tracked").unwrap().default(), None);
    assert_eq!(
        root.field("implicit_count").unwrap().cardinality(),
        Cardinality::Implicit
    );
    assert_eq!(
        root.field("implicit_count").unwrap().default(),
        Some(&DefaultValue::Signed(0))
    );
    assert_eq!(
        root.field("child").unwrap().cardinality(),
        Cardinality::Optional
    );

    let schema = to_ir_schema(&layout, "Root").unwrap();
    let ir::SchemaKind::Group { children, .. } = schema.kind else {
        panic!("protobuf message should project to a group");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["tracked", "implicit_count", "label", "state", "child"]
    );
    assert!(children.iter().all(|child| !child.repeating));

    assert_eq!(
        decode(&layout, "Root", &[]),
        group(vec![
            ("tracked", scalar(Value::Null)),
            ("implicit_count", scalar(Value::Int(0))),
            ("label", scalar(Value::Null)),
            ("state", scalar(Value::Null)),
        ])
    );

    let present_defaults = group(vec![
        ("tracked", scalar(Value::Int(0))),
        ("implicit_count", scalar(Value::Int(0))),
        ("label", scalar(Value::String(String::new()))),
        ("state", scalar(Value::Int(0))),
        ("child", Instance::Group(vec![])),
    ]);
    assert_eq!(
        encode(&layout, "Root", &present_defaults),
        vec![0x08, 0x00, 0x1a, 0x00, 0x20, 0x00, 0x2a, 0x00]
    );
    let decoded = decode(
        &layout,
        "Root",
        &[0x08, 0x00, 0x1a, 0x00, 0x20, 0x00, 0x2a, 0x00],
    );
    assert_eq!(
        decoded.field("tracked").and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
    assert_eq!(
        decoded
            .field("implicit_count")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
    assert_eq!(
        decoded.field("label").and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
    assert_eq!(
        decoded.field("state").and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
    assert!(decoded.field("child").is_some());
}

#[test]
fn parses_and_decodes_proto3_implicit_defaults() {
    let layout = parse(
        r#"
        syntax = "proto3";
        package example;
        message Root {
          string text = 1;
          int32 count = 2;
          bool enabled = 3;
          float ratio = 4;
          bytes data = 5;
          State state = 6;
          Child child = 7;
          repeated int32 values = 8;
        }
        message Child { string name = 1; }
        enum State { UNKNOWN = 0; READY = 1; }
        "#,
    );
    let root_id = layout.resolve_message("example.Root").unwrap();
    let root = layout.message(root_id).unwrap();
    assert!(
        root.fields()
            .iter()
            .take(7)
            .all(|field| field.cardinality() == Cardinality::Implicit)
    );
    assert_eq!(
        root.field("state").unwrap().default(),
        Some(&DefaultValue::Enum(0))
    );

    assert_eq!(
        decode(&layout, "example.Root", &[]),
        group(vec![
            ("text", scalar(Value::String(String::new()))),
            ("count", scalar(Value::Int(0))),
            ("enabled", scalar(Value::Bool(false))),
            ("ratio", scalar(Value::Float(0.0))),
            ("data", scalar(Value::String(String::new()))),
            ("state", scalar(Value::Int(0))),
            ("values", Instance::Repeated(Vec::new())),
        ])
    );

    let bytes = [
        0x0a, 0x02, b'o', b'k', // text
        0x10, 0x07, // count
        0x30, 0x01, // state READY
        0x3a, 0x03, 0x0a, 0x01, b'x', // child name
    ];
    let decoded = decode(&layout, "example.Root", &bytes);
    assert_eq!(
        decoded.field("text").and_then(Instance::as_scalar),
        Some(&Value::String("ok".to_string()))
    );
    assert_eq!(
        decoded
            .field("child")
            .and_then(|child| child.field("name"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("x".to_string()))
    );
}

#[test]
fn rejects_missing_unknown_duplicate_and_shape_incompatible_fields() {
    let layout = parse(CONTACT_SCHEMA);
    let missing = group(vec![("contacts", Instance::Repeated(Vec::new()))]);
    let error = error_text(to_vec(
        &layout,
        "Contact",
        &group(vec![("name", scalar(Value::String("A".to_string())))]),
    ));
    assert!(error.contains("Contact.id"));
    assert!(error.contains("required field"));

    let error = error_text(to_vec(
        &layout,
        "Directory",
        &group(vec![
            ("contacts", Instance::Repeated(Vec::new())),
            ("other", scalar(Value::Int(1))),
        ]),
    ));
    assert!(error.contains("Directory.other"));
    assert!(error.contains("no field named"));

    let error = error_text(to_vec(
        &layout,
        "Directory",
        &group(vec![
            ("contacts", Instance::Repeated(Vec::new())),
            ("contacts", Instance::Repeated(Vec::new())),
        ]),
    ));
    assert!(error.contains("occurs more than once"));

    let error = error_text(to_vec(
        &layout,
        "Directory",
        &group(vec![(
            "contacts",
            scalar(Value::String("not rows".to_string())),
        )]),
    ));
    assert!(error.contains("expected a repeated sequence"));

    assert_eq!(encode(&layout, "Directory", &missing), Vec::<u8>::new());
}

#[test]
fn rejects_scalar_range_type_and_unknown_enum_values_with_paths() {
    let layout = parse(CONTACT_SCHEMA);
    let contact = |id: Value, kind: Value| {
        group(vec![
            ("name", scalar(Value::String("A".to_string()))),
            ("id", scalar(id)),
            ("kind", scalar(kind)),
        ])
    };

    let error = error_text(to_vec(
        &layout,
        "Contact",
        &contact(
            Value::Int(i64::from(i32::MAX) + 1),
            Value::String("HOME".to_string()),
        ),
    ));
    assert!(error.contains("Contact.id"));
    assert!(error.contains("int32 range"));

    let error = error_text(to_vec(
        &layout,
        "Contact",
        &contact(Value::Bool(true), Value::String("HOME".to_string())),
    ));
    assert!(error.contains("expected integer, got bool"));

    let error = error_text(to_vec(
        &layout,
        "Contact",
        &contact(Value::Int(1), Value::String("UNKNOWN".to_string())),
    ));
    assert!(error.contains("Contact.kind"));
    assert!(error.contains("no value named `UNKNOWN`"));

    assert_eq!(
        encode(
            &layout,
            "Contact",
            &contact(Value::Int(1), Value::Float(2.0))
        ),
        vec![0x0a, 0x01, b'A', 0x10, 0x01, 0x18, 0x02]
    );
}

#[test]
fn exposes_all_scalar_types_in_resolved_fields() {
    let layout =
        parse("message M { required bytes a = 1; optional sfixed64 b = 2; repeated bool c = 3; }");
    let id = match layout.resolve_message("M") {
        Ok(id) => id,
        Err(error) => panic!("M should resolve: {error}"),
    };
    let fields = match layout.message(id) {
        Some(message) => message.fields(),
        None => panic!("M id should be valid"),
    };
    assert_eq!(fields[0].ty(), FieldType::Scalar(ScalarType::Bytes));
    assert_eq!(fields[1].ty(), FieldType::Scalar(ScalarType::Sfixed64));
    assert_eq!(fields[2].ty(), FieldType::Scalar(ScalarType::Bool));
}

#[test]
fn decodes_nested_messages_packed_scalars_defaults_and_unknown_fields() {
    let layout = parse(
        r#"
        message Child {
          optional int32 left = 1;
          optional int32 right = 2;
        }
        message Root {
          required int32 id = 1;
          optional string label = 2 [default = "fallback"];
          repeated sint32 values = 3 [packed = true];
          optional Child child = 4;
        }
        "#,
    );
    let bytes = [
        0x08, 0x07, // id
        0x1a, 0x03, 0x03, 0x00, 0x04, // packed -2, 0, 2
        0x22, 0x02, 0x08, 0x01, // first child occurrence
        0x22, 0x02, 0x10, 0x02, // second child occurrence merges
        0x78, 0x63, // unknown varint field
    ];
    assert_eq!(
        decode(&layout, "Root", &bytes),
        group(vec![
            ("id", scalar(Value::Int(7))),
            ("label", scalar(Value::String("fallback".to_string()))),
            (
                "values",
                Instance::Repeated(vec![
                    scalar(Value::Int(-2)),
                    scalar(Value::Int(0)),
                    scalar(Value::Int(2)),
                ]),
            ),
            (
                "child",
                group(vec![
                    ("left", scalar(Value::Int(1))),
                    ("right", scalar(Value::Int(2))),
                ]),
            ),
        ])
    );
}

#[test]
fn all_scalar_wire_forms_roundtrip_through_the_decoder() {
    let layout = parse(
        r#"
        message Wires {
          required double d = 1;
          required float f = 2;
          required int32 i32 = 3;
          required int64 i64 = 4;
          required uint32 u32 = 5;
          required uint64 u64 = 6;
          required sint32 si32 = 7;
          required sint64 si64 = 8;
          required fixed32 fx32 = 9;
          required fixed64 fx64 = 10;
          required sfixed32 sfx32 = 11;
          required sfixed64 sfx64 = 12;
          required bool flag = 13;
          required string text = 14;
          required bytes data = 15;
          repeated int32 packed_numbers = 16 [packed = true];
        }
        "#,
    );
    let instance = group(vec![
        ("d", scalar(Value::Float(1.5))),
        ("f", scalar(Value::Float(2.5))),
        ("i32", scalar(Value::Int(-1))),
        ("i64", scalar(Value::Int(300))),
        ("u32", scalar(Value::Int(4))),
        ("u64", scalar(Value::Int(5))),
        ("si32", scalar(Value::Int(-2))),
        ("si64", scalar(Value::Int(-3))),
        ("fx32", scalar(Value::Int(0x0102_0304))),
        ("fx64", scalar(Value::Int(0x0102_0304_0506_0708))),
        ("sfx32", scalar(Value::Int(-2))),
        ("sfx64", scalar(Value::Int(-3))),
        ("flag", scalar(Value::Bool(true))),
        ("text", scalar(Value::String("x".to_string()))),
        ("data", scalar(Value::String("yz".to_string()))),
        (
            "packed_numbers",
            Instance::Repeated(vec![scalar(Value::Int(1)), scalar(Value::Int(2))]),
        ),
    ]);
    let bytes = encode(&layout, "Wires", &instance);
    assert_eq!(decode(&layout, "Wires", &bytes), instance);
}

#[test]
fn decoder_rejects_truncated_wrong_wire_and_non_utf8_values() {
    let layout = parse("message M { required int32 id = 1; }");
    let truncated = error_text(from_slice(&layout, "M", &[0x08, 0x80]));
    assert!(truncated.contains("truncated int32"), "{truncated}");

    let wrong_wire = error_text(from_slice(&layout, "M", &[0x0a, 0x01, 0x00]));
    assert!(wrong_wire.contains("expected wire type 0"), "{wrong_wire}");

    let strings = parse("message Text { required string value = 1; }");
    let non_utf8 = error_text(from_slice(&strings, "Text", &[0x0a, 0x01, 0xff]));
    assert!(non_utf8.contains("not valid UTF-8"), "{non_utf8}");
}

#[test]
fn projects_resolved_messages_into_the_mapping_schema_ir() {
    let layout = parse(CONTACT_SCHEMA);
    let schema = match to_ir_schema(&layout, "Directory") {
        Ok(schema) => schema,
        Err(error) => panic!("Directory should project: {error}"),
    };
    assert_eq!(schema.name, "Directory");
    let contacts = match schema.child("contacts") {
        Some(child) => child,
        None => panic!("contacts schema should exist"),
    };
    assert!(contacts.repeating);
    assert_eq!(
        contacts.child("id").map(|child| &child.kind),
        Some(&ir::SchemaKind::Scalar {
            ty: ir::ScalarType::Int
        })
    );
    assert_eq!(
        contacts.child("kind").map(|child| &child.kind),
        Some(&ir::SchemaKind::Scalar {
            ty: ir::ScalarType::Int
        })
    );
    assert!(
        contacts
            .child("phones")
            .is_some_and(|child| child.repeating)
    );

    let recursive = parse("message Node { optional Node child = 1; }");
    let error = error_text(to_ir_schema(&recursive, "Node"));
    assert!(error.contains("recursive message"));
}
