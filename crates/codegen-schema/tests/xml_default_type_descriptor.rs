use codegen_schema::{V2_PREFIX, decode, encode};
use ir::{GroupAlternative, ScalarType, SchemaNode};
fn schema() -> SchemaNode {
    let mut s = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .with_alternatives(
        ["{urn:ferrule:descriptor}A", "{urn:ferrule:descriptor}B"]
            .into_iter()
            .map(|name| GroupAlternative {
                name: name.into(),
                members: vec!["Value".into()],
                required: vec![],
                constraints: vec![],
            })
            .collect(),
    )
    .unwrap();
    s.xml_type_alternatives = true;
    s.xml_default_type = Some("{urn:ferrule:descriptor}A".into());
    s
}
#[test]
fn descriptor_preserves_optional_default_identity_in_both_versions() {
    let s = schema();
    let encoded = encode(&s, 1024 * 1024).unwrap();
    assert_eq!(decode(&encoded, 1024 * 1024).unwrap(), s);
    assert!(!encoded.starts_with(V2_PREFIX));
    let v2 = format!("{V2_PREFIX}{encoded}");
    assert_eq!(decode(&v2, 1024 * 1024).unwrap(), s);
    let mut legacy = s.clone();
    legacy.xml_default_type = None;
    let encoded = encode(&legacy, 1024 * 1024).unwrap();
    assert!(!encoded.contains("xml_default_type"));
    assert_eq!(decode(&encoded, 1024 * 1024).unwrap(), legacy);
}
#[test]
fn descriptor_rejects_invalid_default_metadata_before_generated_boundaries() {
    let s = schema();
    let encoded = encode(&s, 1024 * 1024).unwrap();
    let invalid = encoded.replace(
        "\"xml_default_type\":\"{urn:ferrule:descriptor}A\"",
        "\"xml_default_type\":\"Missing\"",
    );
    assert!(decode(&invalid, 1024 * 1024).is_err());
    assert!(decode(&format!("{V2_PREFIX}{invalid}"), 1024 * 1024).is_err());
    let mut invalid = s;
    invalid.xml_default_type = Some("Missing".into());
    assert!(encode(&invalid, 1024 * 1024).is_err());
}
