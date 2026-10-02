use ir::{Instance, ScalarType, SchemaNode};
#[test]
fn json_presence_and_schema_required_sets_ignore_valid_xml_attribute_use() {
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.set_xml_attribute_required(true);
    let schema = SchemaNode::group("Root", vec![code]);
    assert!(schema.required_fields().is_empty());
    for text in ["{}", r#"{"Code":""}"#, r#"{"Code":"x"}"#] {
        let typed = format_json::from_str(text, &schema).unwrap();
        assert_eq!(
            format_json::from_str(&format_json::to_string(&schema, &typed).unwrap(), &schema)
                .unwrap(),
            typed
        );
    }
    assert_eq!(
        format_json::to_string(&schema, &Instance::Group(vec![])).unwrap(),
        "{}\n"
    );
}
#[test]
fn programmatic_invalid_attribute_use_is_rejected_in_nested_json_boundaries() {
    let mut code = SchemaNode::scalar("Code", ScalarType::String);
    code.xml_attribute_required = true;
    let schema = SchemaNode::group("Root", vec![code]);
    assert!(matches!(
        format_json::from_str("{}", &schema),
        Err(format_json::JsonFormatError::InvalidXmlAttributeRequired { .. })
    ));
    assert!(matches!(
        format_json::to_string(&schema, &Instance::Group(vec![])),
        Err(format_json::JsonFormatError::InvalidXmlAttributeRequired { .. })
    ));
}
