use codegen_schema::{V4_PREFIX, decode, encode};
use ir::SchemaNode;

#[test]
fn object_item_metadata_uses_v4_and_retains_legacy_descriptor_bytes() {
    let old = r#"{"name":"Items","repeating":true,"container_nullable":true,"kind":{"kind":"group","children":[]}}"#;
    let legacy: SchemaNode = serde_json::from_str(old).unwrap();
    assert_eq!(encode(&legacy, 1024 * 1024).unwrap(), old);
    for nullable in [false, true] {
        let mut explicit = legacy.clone();
        explicit.json_object_item_nullable = Some(nullable);
        let encoded = encode(&explicit, 1024 * 1024).unwrap();
        assert!(encoded.starts_with(V4_PREFIX));
        assert!(encoded.contains(&format!("\"json_object_item_nullable\":{nullable}")));
        assert_eq!(decode(&encoded, 1024 * 1024).unwrap(), explicit);
        let nested = SchemaNode::group("Envelope", vec![explicit]);
        assert!(encode(&nested, 1024 * 1024).unwrap().starts_with(V4_PREFIX));
        assert_eq!(
            decode(&encode(&nested, 1024 * 1024).unwrap(), 1024 * 1024).unwrap(),
            nested
        );
    }
    let malformed = old
        .replace("\"repeating\":true", "\"repeating\":false")
        .replace(
            "\"container_nullable\":true",
            "\"json_object_item_nullable\":true",
        );
    assert!(decode(&format!("{V4_PREFIX}{malformed}"), 1024 * 1024).is_err());
    for value in ["null", "0", "\"false\""] {
        let malformed = old.replace(
            "\"container_nullable\":true",
            &format!("\"json_object_item_nullable\":{value}"),
        );
        assert!(decode(&format!("{V4_PREFIX}{malformed}"), 1024 * 1024).is_err());
    }
}
