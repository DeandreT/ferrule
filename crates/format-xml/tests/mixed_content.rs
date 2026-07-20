use format_xml::{XmlFormatError, from_str, to_string};
use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_MIXED_CONTENT_FIELD,
    XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};

fn text_item(text: &str) -> Instance {
    Instance::Group(vec![
        (
            XML_NODE_NAME_FIELD.into(),
            Instance::Scalar(Value::String(String::new())),
        ),
        (
            XML_TEXT_FIELD.into(),
            Instance::Scalar(Value::String(text.into())),
        ),
        (
            XML_MIXED_CONTENT_VALUE_FIELD.into(),
            Instance::Scalar(Value::String(text.into())),
        ),
    ])
}

fn element_item(name: &str, value: &str) -> Instance {
    Instance::Group(vec![
        (
            XML_NODE_NAME_FIELD.into(),
            Instance::Scalar(Value::String(name.into())),
        ),
        (
            XML_TEXT_FIELD.into(),
            Instance::Scalar(Value::String(value.into())),
        ),
        (
            XML_MIXED_CONTENT_VALUE_FIELD.into(),
            Instance::Scalar(Value::String(value.into())),
        ),
    ])
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Description",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Bold", ScalarType::String).repeating(),
            SchemaNode::scalar("Italic", ScalarType::String).repeating(),
        ],
    )
}

#[test]
fn writer_emits_repeated_mixed_children_in_recorded_order() {
    let instance = Instance::Group(vec![
        (
            XML_TEXT_FIELD.into(),
            Instance::Scalar(Value::String("Example 2014 uses  and  data.".into())),
        ),
        (
            "Bold".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::String("XMLSpy".into()))]),
        ),
        (
            "Italic".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("XML".into())),
                Instance::Scalar(Value::String("EDI".into())),
            ]),
        ),
        (
            XML_MIXED_CONTENT_FIELD.into(),
            Instance::Repeated(vec![
                text_item("Example "),
                element_item("Bold", "XMLSpy"),
                text_item(" 2014 uses "),
                element_item("Italic", "XML"),
                text_item(" and "),
                element_item("Italic", "EDI"),
                text_item(" data."),
            ]),
        ),
    ]);

    let xml = to_string(&schema(), &instance).unwrap();
    assert!(
        xml.contains(
            "Example <Bold>XMLSpy</Bold> 2014 uses <Italic>XML</Italic> and <Italic>EDI</Italic> data."
        ),
        "{xml}"
    );
    assert_eq!(from_str(&xml, &schema()).unwrap(), instance);
}

#[test]
fn writer_rejects_unknown_mixed_child_names() {
    let instance = Instance::Group(vec![
        (
            XML_TEXT_FIELD.into(),
            Instance::Scalar(Value::String(String::new())),
        ),
        (
            XML_MIXED_CONTENT_FIELD.into(),
            Instance::Repeated(vec![element_item("Unknown", "value")]),
        ),
    ]);

    assert!(matches!(
        to_string(&schema(), &instance),
        Err(XmlFormatError::InvalidMixedContent { .. })
    ));
}
