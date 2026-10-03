use format_xml::{XmlFormatError, XmlWriteOptions};
use ir::{
    GroupAlternative, Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD, XmlNamespace,
    XmlSchemaHints, XmlSchemaLocation,
};
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
fn hints(location: &str) -> XmlSchemaHints {
    XmlSchemaHints {
        no_namespace_location: Some(location.into()),
        ..Default::default()
    }
}
fn options(hints: XmlSchemaHints) -> XmlWriteOptions {
    XmlWriteOptions {
        schema_hints: Some(hints),
        ..Default::default()
    }
}
fn instance(fields: &[(&str, &str)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(n, v)| {
                (
                    n.to_string(),
                    Instance::Scalar(Value::String(v.to_string())),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    )
}
fn root() -> SchemaNode {
    SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Code", ScalarType::String).attribute()],
    )
}
#[test]
fn default_output_is_exact_and_explicit_hint_is_escaped_lexical_metadata() {
    let schema = root();
    let input = instance(&[("Code", "value")]);
    let default = format_xml::to_string(&schema, &input).unwrap();
    assert_eq!(
        default,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Root Code=\"value\"/>"
    );
    let location = "../schemas/é&quote\"<.xsd";
    let rendered =
        format_xml::to_string_with_options(&schema, &input, &options(hints(location))).unwrap();
    let doc = roxmltree::Document::parse(&rendered).unwrap();
    assert_eq!(
        doc.root_element()
            .attribute((XSI, "noNamespaceSchemaLocation")),
        Some(location)
    );
    assert!(rendered.contains("é&amp;quote&quot;&lt;.xsd"));
    assert_eq!(format_xml::from_str(&rendered, &schema).unwrap(), input);
    assert_eq!(format_xml::to_string(&schema, &input).unwrap(), default);
}
#[test]
fn qualified_and_mixed_roots_retain_data_namespaces_and_pair_order() {
    for qualified_attributes in [false, true] {
        let mut schema = root();
        schema.xml_namespace = Some(XmlNamespace::qualified("urn:document").unwrap());
        if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
            children[0].xml_namespace = Some(if qualified_attributes {
                XmlNamespace::qualified("urn:document").unwrap()
            } else {
                XmlNamespace::Unqualified
            });
        }
        let metadata = XmlSchemaHints {
            locations: vec![
                XmlSchemaLocation {
                    namespace: "urn:document".into(),
                    location: "./schemas/doc&main.xsd".into(),
                },
                XmlSchemaLocation {
                    namespace: "urn:import".into(),
                    location: "import.xsd".into(),
                },
            ],
            ..Default::default()
        };
        let rendered = format_xml::to_string_with_options(
            &schema,
            &instance(&[("Code", "x")]),
            &options(metadata),
        )
        .unwrap();
        let doc = roxmltree::Document::parse(&rendered).unwrap();
        let root = doc.root_element();
        assert_eq!(root.tag_name().namespace(), Some("urn:document"));
        assert_eq!(
            root.attribute((XSI, "schemaLocation")),
            Some("urn:document ./schemas/doc&main.xsd urn:import import.xsd")
        );
        assert_eq!(
            if qualified_attributes {
                root.attribute(("urn:document", "Code"))
            } else {
                root.attribute("Code")
            },
            Some("x")
        );
        assert!(root.attribute((XSI, "noNamespaceSchemaLocation")).is_none());
    }
}
#[test]
fn hints_stay_on_root_and_share_xsi_declaration_with_type_and_nil() {
    let mut schema = root()
        .with_alternatives(vec![GroupAlternative {
            name: "Base".into(),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        }])
        .unwrap();
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some("Base".into());
    let output = format_xml::to_string_with_options(
        &schema,
        &instance(&[("Code", "x"), (XML_TYPE_FIELD, "Base")]),
        &options(hints("root.xsd")),
    )
    .unwrap();
    assert_eq!(output.matches("xmlns:xsi=").count(), 1);
    let doc = roxmltree::Document::parse(&output).unwrap();
    assert_eq!(doc.root_element().attribute((XSI, "type")), Some("Base"));
    let mut scalar = SchemaNode::scalar("Value", ScalarType::String);
    scalar.nillable = true;
    let output = format_xml::to_string_with_options(
        &scalar,
        &Instance::Scalar(Value::xml_nil()),
        &options(hints("nil.xsd")),
    )
    .unwrap();
    assert_eq!(output.matches("xmlns:xsi=").count(), 1);
    let doc = roxmltree::Document::parse(&output).unwrap();
    assert_eq!(doc.root_element().attribute((XSI, "nil")), Some("true"));
    let nested = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Child", ScalarType::String)],
    );
    let output = format_xml::to_string_with_options(
        &nested,
        &instance(&[("Child", "text")]),
        &options(hints("nested.xsd")),
    )
    .unwrap();
    let doc = roxmltree::Document::parse(&output).unwrap();
    assert!(
        doc.root_element()
            .first_element_child()
            .unwrap()
            .attribute((XSI, "noNamespaceSchemaLocation"))
            .is_none()
    );
    assert_eq!(output.matches("noNamespaceSchemaLocation=").count(), 1);
}
#[test]
fn hint_and_namespace_attribute_collisions_refuse() {
    for name in ["noNamespaceSchemaLocation", "schemaLocation"] {
        let mut attr = SchemaNode::scalar(name, ScalarType::String).attribute();
        attr.xml_namespace = Some(XmlNamespace::qualified(XSI).unwrap());
        let schema = SchemaNode::group("Root", vec![attr]);
        let metadata = if name == "schemaLocation" {
            XmlSchemaHints {
                locations: vec![XmlSchemaLocation {
                    namespace: "urn:test".into(),
                    location: "x.xsd".into(),
                }],
                ..Default::default()
            }
        } else {
            hints("x.xsd")
        };
        assert!(matches!(
            format_xml::to_string_with_options(
                &schema,
                &instance(&[(name, "mapped")]),
                &options(metadata)
            ),
            Err(XmlFormatError::SchemaHintCollision(_))
        ));
    }
    for (name, value) in [
        ("xmlns:xsi", "urn:wrong"),
        ("xsi:unknown", "value"),
        ("xmlns:xml", "urn:wrong"),
    ] {
        let schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar(name, ScalarType::String).attribute()],
        );
        assert!(matches!(
            format_xml::to_string_with_options(
                &schema,
                &instance(&[(name, value)]),
                &options(hints("x.xsd"))
            ),
            Err(XmlFormatError::SchemaHintCollision(_))
        ));
    }
}
#[test]
fn header_and_hint_bounds_refuse_before_file_creation() {
    let schema = root();
    let input = instance(&[("Code", &"x".repeat(1024 * 1024))]);
    assert!(matches!(
        format_xml::to_string_with_options(&schema, &input, &options(hints("x.xsd"))),
        Err(XmlFormatError::SchemaHintCollision(_))
    ));
    assert!(matches!(
        format_xml::to_string_with_options(&schema, &instance(&[]), &options(hints("a b.xsd"))),
        Err(XmlFormatError::InvalidSchemaHints(_))
    ));
}

#[test]
fn alternate_prefix_duplicate_expanded_attributes_refuse_and_distinct_namespaces_pass() {
    let schema = SchemaNode::group(
        "Root",
        ["xmlns:p", "xmlns:q", "p:Value", "q:Value"]
            .into_iter()
            .map(|name| SchemaNode::scalar(name, ScalarType::String).attribute())
            .collect(),
    );
    for (second_namespace, collides) in [("urn:shared", true), ("urn:distinct", false)] {
        let input = instance(&[
            ("xmlns:p", "urn:shared"),
            ("xmlns:q", second_namespace),
            ("p:Value", "one"),
            ("q:Value", "two"),
        ]);
        let result =
            format_xml::to_string_with_options(&schema, &input, &options(hints("root.xsd")));
        if collides {
            assert!(
                matches!(result, Err(XmlFormatError::SchemaHintCollision(ref reason)) if reason.contains("duplicate expanded"))
            );
        } else {
            let output = result.unwrap();
            let doc = roxmltree::Document::parse(&output).unwrap();
            assert_eq!(
                doc.root_element().attribute(("urn:shared", "Value")),
                Some("one")
            );
            assert_eq!(
                doc.root_element().attribute(("urn:distinct", "Value")),
                Some("two")
            );
        }
    }
}
#[test]
fn alternate_prefix_existing_xsi_hints_refuse_and_unrelated_attribute_passes() {
    for local in ["noNamespaceSchemaLocation", "schemaLocation", "other"] {
        let name = format!("h:{local}");
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("xmlns:h", ScalarType::String).attribute(),
                SchemaNode::scalar(&name, ScalarType::String).attribute(),
            ],
        );
        let metadata = if local == "schemaLocation" {
            XmlSchemaHints {
                locations: vec![XmlSchemaLocation {
                    namespace: "urn:test".into(),
                    location: "root.xsd".into(),
                }],
                ..Default::default()
            }
        } else {
            hints("root.xsd")
        };
        let result = format_xml::to_string_with_options(
            &schema,
            &instance(&[("xmlns:h", XSI), (&name, "existing")]),
            &options(metadata),
        );
        if local == "other" {
            let output = result.unwrap();
            let doc = roxmltree::Document::parse(&output).unwrap();
            assert_eq!(
                doc.root_element().attribute((XSI, "other")),
                Some("existing")
            );
            assert_eq!(
                doc.root_element()
                    .attribute((XSI, "noNamespaceSchemaLocation")),
                Some("root.xsd")
            );
        } else {
            assert!(
                matches!(result,Err(XmlFormatError::SchemaHintCollision(ref reason)) if reason.contains("already supplies"))
            );
        }
    }
}
