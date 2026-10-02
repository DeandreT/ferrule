use codegen_schema::{V2_PREFIX, decode, encode};
use ir::{ScalarType, SchemaNode};
#[test]
fn optional_occurrence_survives_both_descriptor_versions_and_legacy_stays_stable() {
    let mut s = SchemaNode::scalar("Value", ScalarType::String);
    let legacy = encode(&s, 1_048_576).unwrap();
    assert!(!legacy.contains("xml_optional"));
    assert!(s.set_xml_optional(true));
    let encoded = encode(&s, 1_048_576).unwrap();
    assert_eq!(decode(&encoded, 1_048_576).unwrap(), s);
    assert_eq!(
        decode(&format!("{V2_PREFIX}{encoded}"), 1_048_576).unwrap(),
        s
    );
    assert!(s.set_xml_optional(false));
    assert_eq!(encode(&s, 1_048_576).unwrap(), legacy);
}
#[test]
fn descriptor_rejects_invalid_and_nested_optional_roles() {
    let mut s = SchemaNode::scalar("Value", ScalarType::String).repeating();
    s.xml_optional = true;
    assert!(encode(&s, 1_048_576).is_err());
    assert!(encode(&SchemaNode::group("Root", vec![s.clone()]), 1_048_576).is_err());
    let invalid = serde_json::to_string(&s).unwrap();
    assert!(decode(&invalid, 1_048_576).is_err());
    assert!(decode(&format!("{V2_PREFIX}{invalid}"), 1_048_576).is_err());
}
