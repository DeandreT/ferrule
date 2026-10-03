use format_xml::{XmlFormatError, XmlWriteOptions, xsd};
use ir::{Instance, ScalarType, SchemaNode, Value, XML_TEXT_FIELD};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct SchemaFile(PathBuf);

impl SchemaFile {
    fn parse(text: &str) -> SchemaNode {
        let file = Self(std::env::temp_dir().join(format!(
            "ferrule_empty_content_{}_{}.xsd",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::write(&file.0, text).unwrap();
        xsd::import(&file.0).unwrap()
    }
}

impl Drop for SchemaFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn render(schema: &SchemaNode, value: &Instance, indent: bool) -> String {
    format_xml::to_string_with_options(
        schema,
        value,
        &XmlWriteOptions {
            declaration: false,
            indent,
            default_namespace: None,
            schema_hints: None,
        },
    )
    .unwrap()
}

#[test]
fn empty_and_attribute_only_content_has_no_indentation_text() {
    for (declarations, attributes, input) in [
        ("", "", "<Root/>"),
        (
            "",
            r#"<xs:attribute name="Code" type="xs:string"/>"#,
            r#"<Root Code="a"/>"#,
        ),
        (
            r#"targetNamespace="urn:ferrule:empty" attributeFormDefault="qualified""#,
            r#"<xs:attribute name="Code" type="xs:string"/>"#,
            r#"<Root xmlns="urn:ferrule:empty" xmlns:f="urn:ferrule:empty" f:Code="a"/>"#,
        ),
        (
            "",
            r#"<xs:anyAttribute processContents="skip"/>"#,
            r#"<Root xmlns:f="urn:ferrule:attributes" Code="a" f:Flag="true"/>"#,
        ),
    ] {
        let schema = SchemaFile::parse(&format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" {declarations}><xs:element name="Root"><xs:complexType>{attributes}</xs:complexType></xs:element></xs:schema>"#
        ));
        let value = format_xml::from_str(input, &schema).unwrap();
        for indent in [false, true] {
            let xml = render(&schema, &value, indent);
            let document = roxmltree::Document::parse(&xml).unwrap();
            assert_eq!(document.root_element().children().count(), 0);
            assert!(xml.ends_with("/>"));
            assert_eq!(format_xml::from_str(&xml, &schema).unwrap(), value);
        }
    }
}

#[test]
fn an_empty_child_stays_present_and_an_absent_child_stays_absent() {
    let schema = SchemaFile::parse(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Root"><xs:complexType><xs:sequence><xs:element name="Empty" minOccurs="0"><xs:complexType><xs:attribute name="Code" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    for input in [
        "<Root/>",
        "<Root><Empty/></Root>",
        r#"<Root><Empty Code="a"/></Root>"#,
    ] {
        let value = format_xml::from_str(input, &schema).unwrap();
        let xml = render(&schema, &value, true);
        let document = roxmltree::Document::parse(&xml).unwrap();
        if let Some(empty) = document
            .root_element()
            .children()
            .find(|child| child.is_element())
        {
            assert_eq!(empty.children().count(), 0);
        }
        assert_eq!(format_xml::from_str(&xml, &schema).unwrap(), value);
    }
}

#[test]
fn empty_typed_or_fixed_text_still_reports_conversion_errors() {
    for (ty, fixed) in [
        (ScalarType::Int, None),
        (ScalarType::Float, None),
        (ScalarType::Bool, None),
        (ScalarType::String, Some("required")),
    ] {
        let mut text = SchemaNode::scalar(XML_TEXT_FIELD, ty);
        text.text = true;
        text.fixed = fixed.map(str::to_owned);
        let schema = SchemaNode::group("Root", vec![text]);
        let value = Instance::Group(
            (vec![(
                XML_TEXT_FIELD.into(),
                Instance::Scalar(Value::String(String::new())),
            )])
            .into(),
        );
        for indent in [false, true] {
            assert!(
                format_xml::to_string_with_options(
                    &schema,
                    &value,
                    &XmlWriteOptions {
                        indent,
                        ..Default::default()
                    },
                )
                .is_err()
            );
        }
    }
    for fixed in [None, Some("")] {
        let mut text = SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String);
        text.text = true;
        text.fixed = fixed.map(str::to_owned);
        let schema = SchemaNode::group("Root", vec![text]);
        let value = Instance::Group(
            (vec![(
                XML_TEXT_FIELD.into(),
                Instance::Scalar(Value::String(String::new())),
            )])
            .into(),
        );
        assert_eq!(render(&schema, &value, true), "<Root/>");
    }
}

#[test]
fn empty_content_keeps_attribute_and_group_shape_errors() {
    let mut attribute = SchemaNode::scalar("Code", ScalarType::Int);
    attribute.attribute = true;
    let schema = SchemaNode::group("Root", vec![attribute]);
    for value in [
        Instance::Group((vec![("Code".into(), Instance::Group((vec![]).into()))]).into()),
        Instance::Group(
            (vec![("Code".into(), Instance::Scalar(Value::String("bad".into())))]).into(),
        ),
        Instance::Group((vec![("Other".into(), Instance::Scalar(Value::Int(1)))]).into()),
    ] {
        assert!(format_xml::to_string(&schema, &value).is_err());
    }
    let schema = SchemaNode::group("Root", vec![]);
    assert!(matches!(
        format_xml::to_string(&schema, &Instance::Repeated(vec![])),
        Err(XmlFormatError::Shape { .. })
    ));
}
