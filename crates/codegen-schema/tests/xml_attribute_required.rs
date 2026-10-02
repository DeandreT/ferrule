use codegen_schema::{V2_PREFIX, decode, encode};
use ir::{ScalarType, SchemaNode};
#[test]
fn attribute_use_survives_descriptor_versions_and_false_preserves_legacy_wire() {
    let mut schema = SchemaNode::scalar("Code", ScalarType::String).attribute();
    let legacy = encode(&schema, 1_048_576).unwrap();
    assert!(!legacy.contains("xml_attribute_required"));
    schema.set_xml_attribute_required(true);
    let descriptor = encode(&schema, 1_048_576).unwrap();
    assert_eq!(decode(&descriptor, 1_048_576).unwrap(), schema);
    assert_eq!(
        decode(&format!("{V2_PREFIX}{descriptor}"), 1_048_576).unwrap(),
        schema
    );
    schema.set_xml_attribute_required(false);
    assert_eq!(encode(&schema, 1_048_576).unwrap(), legacy);
}
#[test]
fn descriptor_rejects_invalid_required_use_before_payload_decode() {
    let mut schema = SchemaNode::scalar("Code", ScalarType::String)
        .attribute()
        .repeating();
    schema.xml_attribute_required = true;
    assert!(encode(&schema, 1_048_576).is_err());
    let wire = serde_json::to_string(&schema).unwrap();
    assert!(decode(&wire, 1_048_576).is_err());
    assert!(decode(&format!("{V2_PREFIX}{wire}"), 1_048_576).is_err());
    assert!(encode(&SchemaNode::group("Root", vec![schema]), 1_048_576).is_err());
}
