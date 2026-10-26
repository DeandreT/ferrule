use format_xml::{XmlFormatError, from_str, to_string, xsd};
use ir::{Instance, ScalarType, SchemaNode, Value, XmlNamespace};

fn failures(schema: &SchemaNode) -> [String; 3] {
    [
        from_str("<Root/>", schema).unwrap_err().to_string(),
        to_string(schema, &Instance::Group(Vec::new().into()))
            .unwrap_err()
            .to_string(),
        xsd::export(schema).unwrap_err().to_string(),
    ]
}

#[test]
fn compiler_reader_and_writer_share_schema_metadata_error_order() {
    let mut wrapper = SchemaNode::group("Root", vec![]);
    wrapper.json_array_wrapper = true;
    wrapper.xml_optional = true;
    wrapper.attribute = true;
    let expected = XmlFormatError::UnsupportedSchemaRole {
        node: "Root".into(),
        role: "element",
        kind: "JSON array wrapper",
    }
    .to_string();
    assert_eq!(
        failures(&wrapper),
        [expected.clone(), expected.clone(), expected]
    );

    let mut child = SchemaNode::scalar("Field", ScalarType::String);
    child.attribute = true;
    child.xml_optional = true;
    child.xml_default_type = Some("invalid".into());
    let schema = SchemaNode::group("Root", vec![child]);
    let expected = XmlFormatError::InvalidXmlOptional {
        name: "Field".into(),
    }
    .to_string();
    assert_eq!(
        failures(&schema),
        [expected.clone(), expected.clone(), expected]
    );
}

#[test]
fn compiler_reader_and_writer_share_namespace_ambiguity() {
    let first = SchemaNode::scalar("Item", ScalarType::String);
    let mut second = first.clone();
    second.xml_namespace = XmlNamespace::qualified("urn:second");
    let schema = SchemaNode::group("Root", vec![first, second]);
    let expected = XmlFormatError::AmbiguousNamespaceSiblings {
        group: "Root".into(),
        name: "Item".into(),
        first_namespace: "legacy/unspecified".into(),
        second_namespace: "urn:second".into(),
    }
    .to_string();
    assert_eq!(
        failures(&schema),
        [expected.clone(), expected.clone(), expected]
    );
}

#[test]
fn shared_admission_keeps_valid_schema_and_instance_roundtrips() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let value = Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String("kept".into())),
        )]
        .into(),
    );
    assert!(!xsd::export(&schema).unwrap().is_empty());
    let xml = to_string(&schema, &value).unwrap();
    assert_eq!(from_str(&xml, &schema).unwrap(), value);
}
