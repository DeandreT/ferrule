use ir::{Instance, ScalarType, SchemaNode};
#[test]
fn public_string_bytes_and_xml_codecs_keep_lenient_required_attribute_presence() {
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.set_xml_attribute_required(true);
    let mut kind = SchemaNode::scalar("Kind", ScalarType::String).attribute();
    kind.default = Some("fallback".into());
    let schema = SchemaNode::group("Root", vec![code, kind]);
    let descriptor = codegen_schema::encode(&schema, 1_048_576).unwrap();
    for document in ["{}", r#"{"Code":""}"#, r#"{"Code":"x","Kind":"y"}"#] {
        let native = format_json::from_str(document, &schema).unwrap();
        assert_eq!(
            codegen_runtime::parse_json(&descriptor, document).unwrap(),
            native
        );
        assert_eq!(
            codegen_runtime::parse_json_bytes(&descriptor, document.as_bytes()).unwrap(),
            native
        );
        let rendered = codegen_runtime::serialize_json(&descriptor, &native).unwrap();
        assert_eq!(
            codegen_runtime::parse_json(&descriptor, &rendered).unwrap(),
            native
        );
        assert_eq!(
            codegen_runtime::serialize_json_bytes(&descriptor, &native).unwrap(),
            rendered.as_bytes()
        );
        assert_eq!(
            codegen_runtime::serialize_xml(1, &descriptor, &native, false, false, None).unwrap(),
            ir::Value::String(
                format_xml::to_string_with_options(
                    &schema,
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
fn malformed_required_attribute_role_fails_all_public_codecs() {
    let mut code = SchemaNode::scalar("Code", ScalarType::String);
    code.xml_attribute_required = true;
    let descriptor = serde_json::to_string(&SchemaNode::group("Root", vec![code])).unwrap();
    assert!(codegen_runtime::parse_json(&descriptor, "{}").is_err());
    assert!(codegen_runtime::parse_json_bytes(&descriptor, b"{}").is_err());
    assert!(codegen_runtime::serialize_json(&descriptor, &Instance::Group(vec![])).is_err());
    assert!(codegen_runtime::serialize_json_bytes(&descriptor, &Instance::Group(vec![])).is_err());
    assert!(
        codegen_runtime::serialize_xml(
            1,
            &descriptor,
            &Instance::Group(vec![]),
            false,
            false,
            None
        )
        .is_err()
    );
}
