use format_xml::{XmlWriteOptions, xsd};
use ir::{Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_selected_empty_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn import(&self, text: &str) -> SchemaNode {
        let path = self.0.join("source.xsd");
        std::fs::write(&path, text).unwrap();
        xsd::import_root(&path, Some("Root")).unwrap()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
const EMPTY: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"/><xs:complexType name="B"><xs:complexContent><xs:extension base="A"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="A"/></xs:schema>"#;
fn typed(name: &str) -> Instance {
    Instance::Group(
        (vec![(
            XML_TYPE_FIELD.into(),
            Instance::Scalar(Value::String(name.into())),
        )])
        .into(),
    )
}
fn write(schema: &SchemaNode, value: &Instance, indent: bool) -> String {
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
fn assert_empty(xml: &str) {
    let parsed = roxmltree::Document::parse(xml).unwrap();
    assert!(parsed.root_element().children().next().is_none(), "{xml}");
}

#[test]
fn declared_empty_type_and_explicit_marker_have_no_indentation_content() {
    let schema = Dir::new().import(EMPTY);
    for value in [Instance::Group((Vec::new()).into()), typed("A")] {
        for indent in [false, true] {
            let xml = write(&schema, &value, indent);
            assert_empty(&xml);
            let doc = roxmltree::Document::parse(&xml).unwrap();
            assert_eq!(
                doc.root_element()
                    .attribute(("http://www.w3.org/2001/XMLSchema-instance", "type")),
                value.field(XML_TYPE_FIELD).map(|_| "A")
            );
        }
    }
}

#[test]
fn attribute_only_selected_type_keeps_attributes_without_text() {
    let schema = Dir::new().import(&EMPTY.replace(
        "<xs:complexType name=\"A\"/>",
        "<xs:complexType name=\"A\"><xs:attribute name=\"Code\" type=\"xs:string\" use=\"required\"/></xs:complexType>"));
    for explicit in [false, true] {
        let mut value = if explicit {
            typed("A")
        } else {
            Instance::Group((Vec::new()).into())
        };
        let Instance::Group(fields) = &mut value else {
            unreachable!()
        };
        fields.push((
            "Code".into(),
            Instance::Scalar(Value::String("kept".into())),
        ));
        for indent in [false, true] {
            let xml = write(&schema, &value, indent);
            assert_empty(&xml);
            assert_eq!(
                roxmltree::Document::parse(&xml)
                    .unwrap()
                    .root_element()
                    .attribute("Code"),
                Some("kept")
            );
        }
    }
}

#[test]
fn qualified_selected_empty_type_retains_explicit_expanded_identity() {
    let source = EMPTY.replace("<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">",
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:q=\"urn:ferrule:selected\" targetNamespace=\"urn:ferrule:selected\">")
        .replace("base=\"A\"", "base=\"q:A\"").replace("type=\"A\"", "type=\"q:A\"");
    let schema = Dir::new().import(&source);
    for value in [
        Instance::Group((Vec::new()).into()),
        typed("{urn:ferrule:selected}A"),
    ] {
        for indent in [false, true] {
            let xml = write(&schema, &value, indent);
            assert_empty(&xml);
            let doc = roxmltree::Document::parse(&xml).unwrap();
            let root = doc.root_element();
            assert_eq!(root.tag_name().namespace(), Some("urn:ferrule:selected"));
            if value.field(XML_TYPE_FIELD).is_some() {
                let marker = root
                    .attribute(("http://www.w3.org/2001/XMLSchema-instance", "type"))
                    .unwrap();
                let (prefix, local) = marker.split_once(':').unwrap();
                assert_eq!(local, "A");
                assert_eq!(
                    root.lookup_namespace_uri(Some(prefix)),
                    Some("urn:ferrule:selected")
                );
            }
        }
    }
}

#[test]
fn selected_empty_type_inside_ordinary_group_has_no_inner_whitespace() {
    let selected = Dir::new().import(EMPTY);
    let schema = SchemaNode::group("Envelope", vec![selected]);
    let value = Instance::Group((vec![("Root".into(), typed("A"))]).into());
    for indent in [false, true] {
        let xml = write(&schema, &value, indent);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert!(
            doc.root_element()
                .first_element_child()
                .unwrap()
                .children()
                .next()
                .is_none(),
            "{xml}"
        );
    }
}

#[test]
fn ordinary_and_selected_element_only_empty_groups_keep_existing_formatting() {
    let schema = Dir::new().import(EMPTY);
    assert_eq!(
        write(&schema, &typed("B"), false),
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="B"></Root>"#
    );
    assert_eq!(
        write(&schema, &typed("B"), true),
        "<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"B\">\n</Root>"
    );
    let mut optional = SchemaNode::scalar("Value", ScalarType::String);
    optional.xml_optional = true;
    let ordinary = SchemaNode::group("Root", vec![optional]);
    let absent = Instance::Group((Vec::new()).into());
    assert_eq!(write(&ordinary, &absent, false), "<Root></Root>");
    assert_eq!(write(&ordinary, &absent, true), "<Root>\n</Root>");
}

#[test]
fn selected_derived_present_empty_and_populated_elements_remain_present() {
    let schema = Dir::new().import(EMPTY);
    for text in ["", "kept"] {
        let mut value = typed("B");
        let Instance::Group(fields) = &mut value else {
            unreachable!()
        };
        fields.push(("Value".into(), Instance::Scalar(Value::String(text.into()))));
        for indent in [false, true] {
            let xml = write(&schema, &value, indent);
            let doc = roxmltree::Document::parse(&xml).unwrap();
            let child = doc.root_element().first_element_child().unwrap();
            assert_eq!(child.tag_name().name(), "Value");
            assert_eq!(child.text().unwrap_or(""), text);
        }
    }
}

#[test]
fn explicit_selection_is_order_independent_and_does_not_admit_foreign_fields() {
    let mut schema = Dir::new().import(EMPTY);
    let ir::SchemaKind::Group { alternatives, .. } = &mut schema.kind else {
        unreachable!()
    };
    alternatives.reverse();
    schema.xml_default_type = None;
    for indent in [false, true] {
        assert_empty(&write(&schema, &typed("A"), indent));
    }
    for value in [
        typed("Unknown"),
        Instance::Group(
            (vec![
                (
                    XML_TYPE_FIELD.into(),
                    Instance::Scalar(Value::String("A".into())),
                ),
                (
                    "Value".into(),
                    Instance::Scalar(Value::String("foreign".into())),
                ),
            ])
            .into(),
        ),
    ] {
        for indent in [false, true] {
            assert!(
                format_xml::to_string_with_options(
                    &schema,
                    &value,
                    &XmlWriteOptions {
                        declaration: false,
                        indent,
                        default_namespace: None,
                        schema_hints: None,
                    }
                )
                .is_err()
            );
        }
    }
}
