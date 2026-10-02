use ir::{Instance, ScalarType, SchemaNode};
#[test]
fn public_string_and_bytes_codecs_preserve_optional_xml_host_fixed_points() {
    let mut scalar = SchemaNode::scalar("Value", ScalarType::String);
    assert!(scalar.set_xml_optional(true));
    let mut group = SchemaNode::group("Maybe", vec![]);
    assert!(group.set_xml_optional(true));
    let s = SchemaNode::group("Root", vec![scalar, group]);
    let descriptor = codegen_schema::encode(&s, 1_048_576).unwrap();
    for doc in [
        "{}",
        r#"{"Value":"","Maybe":{}}"#,
        r#"{"Value":"x"}"#,
        r#"{"Maybe":{}}"#,
    ] {
        let native = format_json::from_str(doc, &s).unwrap();
        let generated = codegen_runtime::parse_json(&descriptor, doc).unwrap();
        assert_eq!(generated, native);
        assert_eq!(
            codegen_runtime::parse_json_bytes(&descriptor, doc.as_bytes()).unwrap(),
            native
        );
        let text = codegen_runtime::serialize_json(&descriptor, &native).unwrap();
        assert_eq!(
            codegen_runtime::parse_json(&descriptor, &text).unwrap(),
            native
        );
        assert_eq!(
            codegen_runtime::serialize_json_bytes(&descriptor, &native).unwrap(),
            text.as_bytes()
        );
        assert_eq!(
            codegen_runtime::serialize_xml(1, &descriptor, &native, false, false, None).unwrap(),
            ir::Value::String(
                format_xml::to_string_with_options(
                    &s,
                    &native,
                    &format_xml::XmlWriteOptions {
                        declaration: false,
                        indent: false,
                        default_namespace: None
                    }
                )
                .unwrap()
            )
        );
    }
}
#[test]
fn public_codecs_reject_invalid_optional_metadata_before_instance_decode() {
    let mut invalid = SchemaNode::scalar("Value", ScalarType::String).repeating();
    invalid.xml_optional = true;
    let d = serde_json::to_string(&invalid).unwrap();
    assert!(codegen_runtime::parse_json(&d, "[]").is_err());
    assert!(codegen_runtime::parse_json_bytes(&d, b"[]").is_err());
    assert!(codegen_runtime::serialize_json(&d, &Instance::Repeated(vec![])).is_err());
    assert!(codegen_runtime::serialize_json_bytes(&d, &Instance::Repeated(vec![])).is_err());
    assert!(
        codegen_runtime::serialize_xml(1, &d, &Instance::Repeated(vec![]), false, false, None)
            .is_err()
    );
}
