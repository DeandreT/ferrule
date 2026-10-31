use codegen_schema::{CodecError, V2_PREFIX, V3_PREFIX, V4_PREFIX, V5_PREFIX, decode, encode};
use ir::{ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode};

const MAX: usize = 1024 * 1024;

/// An open `[string, integer]` tuple with a string tail of length 0..4.
fn frame() -> SchemaNode {
    SchemaNode::group(
        "line",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
            SchemaNode::scalar("rest", ScalarType::String).repeating(),
        ],
    )
    .with_json_tuple(JsonTupleLayout {
        length: ItemCountRange::new(0, Some(4)),
        tail: JsonTupleTail::Open,
    })
    .expect("the test frame is canonical")
}

#[test]
fn tuple_frames_use_v5_descriptors_that_round_trip() {
    let mut path = frame().repeating();
    path.json_object_item_nullable = Some(true);
    let mut wrapper = SchemaNode::group(
        "batches",
        vec![SchemaNode::scalar(ir::JSON_ARRAY_ITEM_FIELD, ScalarType::Int).repeating()],
    )
    .repeating()
    .with_json_array_wrapper()
    .expect("the test wrapper is well formed");
    wrapper.container_nullable = true;
    for schema in [
        frame(),
        SchemaNode::group("document", vec![frame()]),
        // Frames take precedence over every older descriptor feature.
        SchemaNode::group("document", vec![path, wrapper]),
    ] {
        let encoded = encode(&schema, MAX).unwrap();
        assert!(encoded.starts_with(V5_PREFIX), "{encoded}");
        for older in [V2_PREFIX, V3_PREFIX, V4_PREFIX] {
            assert!(!encoded.starts_with(older));
        }
        assert!(encoded.contains(r#""json_tuple":{"length":{"maximum":4},"tail":"open"}"#));
        assert_eq!(decode(&encoded, MAX).unwrap(), schema);
    }
}

#[test]
fn schemas_without_frames_keep_their_descriptor_bytes() {
    let plain = SchemaNode::group(
        "line",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("rest", ScalarType::String).repeating(),
        ],
    );
    let encoded = encode(&plain, MAX).unwrap();
    assert_eq!(encoded, serde_json::to_string(&plain).unwrap());
    assert!(!encoded.contains("json_tuple"));
}

#[test]
fn v5_descriptors_refuse_malformed_frames_and_unknown_versions() {
    let encoded = encode(&frame(), MAX).unwrap();
    let malformed = encoded.replace(r#""tail":"open""#, r#""tail":"closed""#);
    assert!(matches!(
        decode(&malformed, MAX),
        Err(CodecError::Deserialization(_))
    ));
    let later = encoded.replacen(V5_PREFIX, "FERRULE-EMBEDDED-SCHEMA/6\n", 1);
    assert_eq!(decode(&later, MAX), Err(CodecError::UnsupportedVersion));
}

#[test]
fn the_json5_profile_refuses_frames() {
    let schema = SchemaNode::group("document", vec![frame()]);
    assert!(matches!(
        codegen_schema::json5_profile::validate_schema(&schema),
        Err(
            codegen_schema::json5_profile::Json5ProfileError::UnsupportedMetadata {
                field: "json_tuple"
            }
        )
    ));
}
