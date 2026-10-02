use format_xml::{XmlFormatError, xsd};
use ir::{Instance, ScalarType, SchemaNode, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_attribute_use_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }
    fn import(&self, text: &str) -> Result<SchemaNode, XmlFormatError> {
        xsd::import_root(&self.file("source.xsd", text), Some("Root"))
    }
    fn roundtrip(&self, schema: &SchemaNode) -> SchemaNode {
        let set = xsd::export_set(schema, "export.xsd").unwrap();
        for dep in set.dependencies {
            self.file(&dep.filename, &dep.contents);
        }
        xsd::import_root(&self.file("export.xsd", &set.root), Some("Root")).unwrap()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn document(attrs: &str, declarations: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{declarations}<xs:element name="Root"><xs:complexType>{attrs}</xs:complexType></xs:element></xs:schema>"#
    )
}

#[test]
fn local_optional_required_fixed_and_legacy_export_are_exact() {
    let d = Dir::new();
    for (use_value, fixed, required) in [
        ("", "", false),
        (" use=\"optional\"", "", false),
        (" use=\"required\"", "", true),
        (" use=\"required\"", " fixed=\"x\"", true),
    ] {
        let schema = d
            .import(&document(
                &format!("<xs:attribute name=\"Code\" type=\"xs:string\"{use_value}{fixed}/>"),
                "",
            ))
            .unwrap();
        assert_eq!(
            schema.child("Code").unwrap().xml_attribute_required,
            required
        );
        assert_eq!(d.roundtrip(&schema), schema);
        assert_eq!(
            xsd::export(&schema).unwrap().contains("use=\"required\""),
            required
        );
    }
}

#[test]
fn missing_required_attributes_remain_lenient_and_defaults_unchanged() {
    let d = Dir::new();
    let schema = d.import(&document(r#"<xs:attribute name="Code" type="xs:string" use="required"/><xs:attribute name="Kind" type="xs:string" default="fallback"/>"#, "")).unwrap();
    let absent = format_xml::from_str("<Root/>", &schema).unwrap();
    assert_eq!(absent.field("Code"), Some(&Instance::Scalar(Value::Null)));
    assert_eq!(
        absent.field("Kind"),
        Some(&Instance::Scalar(Value::String("fallback".into())))
    );
    for input in [
        absent,
        format_xml::from_str(r#"<Root Code="" Kind="x"/>"#, &schema).unwrap(),
    ] {
        let xml = format_xml::to_string(&schema, &input).unwrap();
        assert_eq!(format_xml::from_str(&xml, &schema).unwrap(), input);
    }
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.xml_attribute_required = true;
    code.default = Some("prior".into());
    let authored = SchemaNode::group("Root", vec![code]);
    assert_eq!(
        format_xml::from_str("<Root/>", &authored)
            .unwrap()
            .field("Code"),
        Some(&Instance::Scalar(Value::String("prior".into())))
    );
    assert!(matches!(
        xsd::export(&authored),
        Err(XmlFormatError::UnsupportedXmlAttributeDefault { .. })
    ));
}

#[test]
fn ordinary_ref_and_attribute_group_retain_use() {
    let d = Dir::new();
    for attrs in [
        r#"<xs:attribute ref="Code" use="required"/>"#,
        r#"<xs:attributeGroup ref="Fields"/>"#,
    ] {
        let schema = d.import(&document(attrs, r#"<xs:attribute name="Code" type="xs:string"/><xs:attributeGroup name="Fields"><xs:attribute ref="Code" use="required"/></xs:attributeGroup>"#)).unwrap();
        assert!(schema.child("Code").unwrap().xml_attribute_required);
        assert_eq!(d.roundtrip(&schema), schema);
    }
}

#[test]
fn foreign_shared_global_keeps_distinct_required_and_optional_local_refs() {
    let d = Dir::new();
    d.file("fields.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:attribute:foreign"><xs:attribute name="Code" type="xs:string"/></xs:schema>"#);
    let schema = d.import(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:attribute:foreign"><xs:import namespace="urn:ferrule:attribute:foreign" schemaLocation="fields.xsd"/><xs:element name="Root"><xs:complexType><xs:sequence><xs:element name="Left"><xs:complexType><xs:attribute ref="f:Code" use="required"/></xs:complexType></xs:element><xs:element name="Right"><xs:complexType><xs:attribute ref="f:Code"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    assert!(
        schema
            .child("Left")
            .unwrap()
            .child("Code")
            .unwrap()
            .xml_attribute_required
    );
    assert!(
        !schema
            .child("Right")
            .unwrap()
            .child("Code")
            .unwrap()
            .xml_attribute_required
    );
    let set = xsd::export_set(&schema, "export.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert_eq!(
        set.dependencies[0]
            .contents
            .matches("name=\"Code\"")
            .count(),
        1
    );
    assert!(!set.dependencies[0].contents.contains("use="));
    assert_eq!(set.root.matches("use=\"required\"").count(), 1);
    assert_eq!(d.roundtrip(&schema), schema);
}

#[test]
fn qualified_local_use_and_recursive_attribute_members_roundtrip() {
    let d = Dir::new();
    let local = d.import(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:attribute:local" elementFormDefault="qualified"><xs:element name="Root"><xs:complexType><xs:attribute name="Code" type="xs:string" form="qualified" use="required"/></xs:complexType></xs:element></xs:schema>"#).unwrap();
    assert_eq!(d.roundtrip(&local), local);
    let recursive = d.import(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Tree"><xs:sequence><xs:element name="Again" type="Tree" minOccurs="0"/></xs:sequence><xs:attribute name="Code" type="xs:string" use="required"/></xs:complexType><xs:element name="Root" type="Tree"/></xs:schema>"#).unwrap();
    assert!(recursive.child("Code").unwrap().xml_attribute_required);
    assert_eq!(d.roundtrip(&recursive), recursive);
    let input =
        format_xml::from_str(r#"<Root Code="a"><Again Code="b"/></Root>"#, &recursive).unwrap();
    assert_eq!(
        format_xml::from_str(
            &format_xml::to_string(&recursive, &input).unwrap(),
            &recursive
        )
        .unwrap(),
        input
    );
}

#[test]
fn flattened_required_default_and_disputed_fixed_refs_reject_explicitly() {
    let d = Dir::new();
    for source in [
        document(
            r#"<xs:attribute name="Code" use="required" default="x"/>"#,
            "",
        ),
        document(
            r#"<xs:attribute ref="Code" use="required"/>"#,
            r#"<xs:attribute name="Code" default="x"/>"#,
        ),
    ] {
        assert!(matches!(
            d.import(&source),
            Err(XmlFormatError::UnsupportedXmlAttributeDefault { .. })
        ));
    }
    for attrs in [
        r#"<xs:attribute ref="Code" use="required"/>"#,
        r#"<xs:attribute ref="Code" fixed="x"/>"#,
        r#"<xs:attribute ref="Code" fixed="y"/>"#,
    ] {
        assert!(matches!(
            d.import(&document(attrs, r#"<xs:attribute name="Code" fixed="x"/>"#)),
            Err(XmlFormatError::UnsupportedXmlAttributeFixedReference { .. })
        ));
    }
}

#[test]
fn unknown_and_global_attribute_use_reject_instead_of_becoming_optional() {
    let d = Dir::new();
    assert!(matches!(
        d.import(&document(
            r#"<xs:attribute name="Code" use="unknown"/>"#,
            ""
        )),
        Err(XmlFormatError::UnsupportedXmlAttributeUse { .. })
    ));
    assert!(matches!(
        d.import(&document(
            r#"<xs:attribute ref="Code"/>"#,
            r#"<xs:attribute name="Code" use="required"/>"#
        )),
        Err(XmlFormatError::UnsupportedXmlAttributeUse { .. })
    ));
}

#[test]
fn incompatible_shared_alternative_attribute_use_is_typed_rejection() {
    let d = Dir::new();
    for content in ["complexContent", "simpleContent"] {
        let base = if content == "simpleContent" {
            r#"<xs:simpleContent><xs:extension base="xs:string"><xs:attribute name="Code"/></xs:extension></xs:simpleContent>"#
        } else {
            r#"<xs:attribute name="Code"/>"#
        };
        let source = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base">{base}</xs:complexType><xs:complexType name="Derived"><xs:{content}><xs:restriction base="Base"><xs:attribute name="Code" use="required"/></xs:restriction></xs:{content}></xs:complexType><xs:element name="Root" type="Base"/></xs:schema>"#
        );
        assert!(matches!(
            d.import(&source),
            Err(XmlFormatError::UnsupportedXmlAlternativeAttributeUse { .. })
        ));
    }
}

#[test]
fn compatible_extension_keeps_added_required_attribute_and_base_identity() {
    let d = Dir::new();
    let schema = d.import(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base"><xs:attribute name="Code" use="required"/></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:extension base="Base"><xs:attribute name="Extra" use="required"/></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Base"/></xs:schema>"#).unwrap();
    assert_eq!(schema.xml_default_type.as_deref(), Some("Base"));
    assert!(schema.child("Code").unwrap().xml_attribute_required);
    assert!(schema.child("Extra").unwrap().xml_attribute_required);
    assert_eq!(d.roundtrip(&schema), schema);
}

#[test]
fn restrictions_cannot_weaken_or_prohibit_a_required_attribute() {
    let d = Dir::new();
    for use_value in ["optional", "prohibited"] {
        let source = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base"><xs:attribute name="Code" use="required"/></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:restriction base="Base"><xs:attribute name="Code" use="{use_value}"/></xs:restriction></xs:complexContent></xs:complexType><xs:element name="Root" type="Derived"/></xs:schema>"#
        );
        assert!(matches!(
            d.import(&source),
            Err(XmlFormatError::UnsupportedComplexContentRestriction { .. })
        ));
    }
}

#[test]
fn malformed_role_is_rejected_by_read_write_and_export_before_payload() {
    let mut code = SchemaNode::scalar("Code", ScalarType::String);
    code.xml_attribute_required = true;
    let schema = SchemaNode::group("Root", vec![code]);
    assert!(matches!(
        format_xml::from_str("<Root/>", &schema),
        Err(XmlFormatError::InvalidXmlAttributeRequired { .. })
    ));
    assert!(matches!(
        format_xml::to_string(&schema, &Instance::Group(vec![])),
        Err(XmlFormatError::InvalidXmlAttributeRequired { .. })
    ));
    assert!(matches!(
        xsd::export(&schema),
        Err(XmlFormatError::InvalidXmlAttributeRequired { .. })
    ));
}
