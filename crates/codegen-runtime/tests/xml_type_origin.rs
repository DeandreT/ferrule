use codegen_runtime::{
    ScopeContext, parse_json, parse_json_bytes, recursive_filter, serialize_json,
    serialize_json_bytes, serialize_xml,
};
use ir::{
    Instance, InstanceGroup, ScalarType, SchemaNode, Value, XML_TYPE_ORIGIN_FIELD, XmlTypeOrigin,
};

fn marked(fields: Vec<(String, Instance)>, origin: XmlTypeOrigin<'_>) -> Instance {
    Instance::Group(
        InstanceGroup::from(fields)
            .with_xml_type_origin(origin)
            .unwrap(),
    )
}

#[test]
fn generated_copy_and_json_hosts_retain_source_facts_without_publishing_them() {
    let schema = SchemaNode::group("Root", vec![])
        .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
        .unwrap();
    let schema = serde_json::to_string(&schema).unwrap();
    for origin in [XmlTypeOrigin::Absent, XmlTypeOrigin::Explicit("Derived")] {
        let source = marked(
            vec![("Code".into(), Instance::Scalar(Value::String("a".into())))],
            origin,
        );
        let copied = ScopeContext::new(&source).copy_current_group().unwrap();
        assert_eq!(copied, source);
        assert_eq!(copied.xml_type_origin(), Ok(origin));
        let text = serialize_json(&schema, &copied).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            serde_json::json!({"Code":"a"})
        );
        assert_eq!(
            serialize_json_bytes(&schema, &copied).unwrap(),
            text.as_bytes()
        );
    }
}

#[test]
fn generated_json_text_and_bytes_preserve_actual_equal_key_input() {
    for schema in [
        SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar(
                XML_TYPE_ORIGIN_FIELD,
                ScalarType::String,
            )],
        ),
        SchemaNode::group("Root", vec![])
            .with_dynamic_fields(SchemaNode::scalar("Value", ScalarType::String))
            .unwrap(),
    ] {
        let descriptor = serde_json::to_string(&schema).unwrap();
        let input = serde_json::json!({XML_TYPE_ORIGIN_FIELD:"ordinary JSON value"});
        let text = input.to_string();
        let value = parse_json(&descriptor, &text).unwrap();
        assert_eq!(
            parse_json_bytes(&descriptor, text.as_bytes()).unwrap(),
            value
        );
        assert_eq!(value.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
        let output = serialize_json(&descriptor, &value).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            input
        );
        assert_eq!(
            serialize_json_bytes(&descriptor, &value).unwrap(),
            output.as_bytes()
        );
    }
}

#[test]
fn recursive_projection_resets_parent_but_retains_unmodified_item_facts() {
    let item = marked(
        vec![("Code".into(), Instance::Scalar(Value::String("a".into())))],
        XmlTypeOrigin::Explicit("Derived"),
    );
    let child = marked(
        vec![("file".into(), Instance::Repeated(vec![item.clone()]))],
        XmlTypeOrigin::Absent,
    );
    let source = marked(
        vec![
            ("file".into(), Instance::Repeated(vec![item.clone()])),
            ("directory".into(), Instance::Repeated(vec![child])),
        ],
        XmlTypeOrigin::Explicit("Base"),
    );
    let output = recursive_filter(&ScopeContext::new(&source), "directory", "file", 1, |_| {
        Ok(Value::Bool(true))
    })
    .unwrap();
    assert_eq!(output.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_eq!(
        output.field("file").unwrap().as_repeated().unwrap()[0],
        item
    );
    let children = output.field("directory").unwrap().as_repeated().unwrap();
    assert_eq!(children[0].xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_eq!(
        children[0].field("file").unwrap().as_repeated().unwrap()[0].xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Derived"))
    );
    assert_eq!(
        source.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Base"))
    );
}

#[test]
fn generated_xml_serialization_keeps_origin_separate_from_selected_identity() {
    let schema: SchemaNode = serde_json::from_str(r#"{"name":"Root","xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","attribute":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code"]},{"name":"Derived","members":["Code","Extra"]}]}}"#).unwrap();
    let descriptor = serde_json::to_string(&schema).unwrap();
    let explicit = format_xml::from_str(r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="a" Extra="b"/>"#, &schema).unwrap();
    let inferred = format_xml::from_str(r#"<Root Code="a" Extra="b"/>"#, &schema).unwrap();
    assert_ne!(explicit, inferred);
    for source in [&explicit, &inferred] {
        let native = format_xml::to_string_with_options(
            &schema,
            source,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                default_namespace: None,
            },
        )
        .unwrap();
        assert_eq!(
            serialize_xml(7, &descriptor, source, false, false, None).unwrap(),
            Value::String(native)
        );
    }
    assert_eq!(
        serialize_xml(7, &descriptor, &explicit, false, false, None).unwrap(),
        serialize_xml(7, &descriptor, &inferred, false, false, None).unwrap()
    );
}

#[test]
fn ordered_recursive_rebuild_retains_unchanged_wrapper_and_resets_replaced_wrapper() {
    use ir::{XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD};
    let item = marked(
        vec![("Code".into(), Instance::Scalar(Value::String("a".into())))],
        XmlTypeOrigin::Explicit("item"),
    );
    let unrelated = marked(
        vec![
            (
                XML_NODE_NAME_FIELD.into(),
                Instance::Scalar(Value::String("unrelated".into())),
            ),
            (
                XML_MIXED_CONTENT_VALUE_FIELD.into(),
                marked(vec![], XmlTypeOrigin::Absent),
            ),
        ],
        XmlTypeOrigin::Explicit("unchanged-wrapper"),
    );
    let replaced = marked(
        vec![
            (
                XML_NODE_NAME_FIELD.into(),
                Instance::Scalar(Value::String("file".into())),
            ),
            (XML_MIXED_CONTENT_VALUE_FIELD.into(), item.clone()),
        ],
        XmlTypeOrigin::Explicit("replaced-wrapper"),
    );
    let source = marked(
        vec![
            ("file".into(), Instance::Repeated(vec![item.clone()])),
            (
                XML_MIXED_CONTENT_FIELD.into(),
                Instance::Repeated(vec![unrelated.clone(), replaced.clone()]),
            ),
        ],
        XmlTypeOrigin::Explicit("parent"),
    );
    let output = recursive_filter(&ScopeContext::new(&source), "directory", "file", 1, |_| {
        Ok(Value::Bool(true))
    })
    .unwrap();
    assert_eq!(output.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    let ordered = output
        .field(XML_MIXED_CONTENT_FIELD)
        .unwrap()
        .as_repeated()
        .unwrap();
    assert_eq!(ordered[0], unrelated);
    assert_eq!(
        ordered[0].xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("unchanged-wrapper"))
    );
    assert_eq!(
        ordered[0]
            .field(XML_MIXED_CONTENT_VALUE_FIELD)
            .unwrap()
            .xml_type_origin(),
        Ok(XmlTypeOrigin::Absent)
    );
    assert_eq!(ordered[1].xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_eq!(ordered[1].field(XML_MIXED_CONTENT_VALUE_FIELD), Some(&item));
    assert_eq!(
        replaced.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("replaced-wrapper"))
    );
    assert_eq!(
        source.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("parent"))
    );
}

#[test]
fn mixed_content_noops_transfer_exact_output_and_attachment_invalidates_own_fact() {
    use codegen_runtime::{XmlMixedContentElement, preserve_xml_mixed_content};
    use ir::{
        XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
    };
    let child = marked(
        vec![("Code".into(), Instance::Scalar(Value::String("a".into())))],
        XmlTypeOrigin::Explicit("child"),
    );
    let output = marked(
        vec![("file".into(), Instance::Repeated(vec![child.clone()]))],
        XmlTypeOrigin::Explicit("output"),
    );
    let no_stream = marked(vec![], XmlTypeOrigin::Absent);
    assert_eq!(
        preserve_xml_mixed_content(&ScopeContext::new(&no_stream), output.clone(), &[]),
        output
    );
    let source = marked(
        vec![(
            XML_MIXED_CONTENT_FIELD.into(),
            Instance::Repeated(vec![marked(
                vec![
                    (
                        XML_NODE_NAME_FIELD.into(),
                        Instance::Scalar(Value::String("file".into())),
                    ),
                    (
                        XML_TEXT_FIELD.into(),
                        Instance::Scalar(Value::String(String::new())),
                    ),
                    (XML_MIXED_CONTENT_VALUE_FIELD.into(), child.clone()),
                ],
                XmlTypeOrigin::Absent,
            )]),
        )],
        XmlTypeOrigin::Absent,
    );
    assert_eq!(
        preserve_xml_mixed_content(&ScopeContext::new(&source), output.clone(), &[]),
        output
    );
    let attached = preserve_xml_mixed_content(
        &ScopeContext::new(&source),
        output.clone(),
        &[XmlMixedContentElement {
            source: "file",
            target: "file",
        }],
    );
    assert_eq!(attached.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    let ordered = attached
        .field(XML_MIXED_CONTENT_FIELD)
        .unwrap()
        .as_repeated()
        .unwrap();
    assert_eq!(ordered[0].xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_eq!(
        ordered[0].field(XML_MIXED_CONTENT_VALUE_FIELD),
        Some(&child)
    );
    assert_eq!(
        output.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("output"))
    );
}
