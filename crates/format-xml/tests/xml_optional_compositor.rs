use format_xml::{XmlFormatError, xsd};
use ir::{Instance, SchemaNode, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ferrule_optional_compositor_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }
    fn import(&self, declarations: &str, particle: &str) -> Result<SchemaNode, XmlFormatError> {
        let xml = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{declarations}<xs:element name="Root"><xs:complexType>{particle}</xs:complexType></xs:element></xs:schema>"#
        );
        xsd::import_root(&self.file("source.xsd", &xml), Some("Root"))
    }
    fn roundtrip(&self, schema: &SchemaNode) {
        let exported = xsd::export_set(schema, "export.xsd").unwrap();
        for item in exported.dependencies {
            self.file(&item.filename, &item.contents);
        }
        assert_eq!(
            xsd::import_root(&self.file("export.xsd", &exported.root), Some("Root")).unwrap(),
            *schema
        );
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
const VALUE: &str = r#"<xs:element name="Value" type="xs:string"/>"#;
const GROUP: &str = r#"<xs:group name="Fields"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:group>"#;
#[test]
fn single_member_optional_compositors_propagate_through_nested_wrappers() {
    for particle in [
        format!("<xs:sequence minOccurs=\"0\">{VALUE}</xs:sequence>"),
        format!("<xs:all minOccurs=\"0\">{VALUE}</xs:all>"),
        format!("<xs:choice minOccurs=\"0\">{VALUE}</xs:choice>"),
        format!("<xs:sequence><xs:choice minOccurs=\"0\">{VALUE}</xs:choice></xs:sequence>"),
        format!("<xs:sequence minOccurs=\"0\"><xs:choice>{VALUE}</xs:choice></xs:sequence>"),
        format!("<xs:sequence><xs:sequence minOccurs=\"0\">{VALUE}</xs:sequence></xs:sequence>"),
        format!("<xs:sequence minOccurs=\"0\"><xs:sequence>{VALUE}</xs:sequence></xs:sequence>"),
        format!(
            "<xs:sequence minOccurs=\"0\"><xs:sequence minOccurs=\"0\">{VALUE}</xs:sequence></xs:sequence>"
        ),
    ] {
        let d = Dir::new();
        let schema = d.import("", &particle).unwrap();
        assert!(schema.child("Value").unwrap().xml_optional, "{particle}");
        d.roundtrip(&schema);
        let empty = format_xml::from_str("<Root/>", &schema).unwrap();
        assert_eq!(empty.field("Value"), Some(&Instance::Scalar(Value::Null)));
        assert_eq!(
            format_xml::from_str("<Root><Value/></Root>", &schema)
                .unwrap()
                .field("Value"),
            Some(&Instance::Scalar(Value::String(String::new())))
        );
    }
}
#[test]
fn resolved_optional_group_particles_preserve_present_and_absent_members() {
    for particle in [
        "<xs:group ref=\"Fields\" minOccurs=\"0\"/>",
        "<xs:sequence><xs:group ref=\"Fields\" minOccurs=\"0\"/></xs:sequence>",
        "<xs:sequence minOccurs=\"0\"><xs:group ref=\"Fields\"/></xs:sequence>",
        "<xs:sequence><xs:sequence minOccurs=\"0\"><xs:group ref=\"Fields\"/></xs:sequence></xs:sequence>",
    ] {
        let d = Dir::new();
        let schema = d.import(GROUP, particle).unwrap();
        assert!(
            matches!(&schema.kind, ir::SchemaKind::Group { children, .. } if children.len() == 1)
        );
        assert!(schema.child("Value").unwrap().xml_optional);
        d.roundtrip(&schema);
        assert_eq!(
            format_xml::from_str("<Root><Value>x</Value></Root>", &schema)
                .unwrap()
                .field("Value"),
            Some(&Instance::Scalar(Value::String("x".into())))
        );
    }
}
#[test]
fn required_direct_and_nested_groups_keep_their_fields_required() {
    for particle in [
        "<xs:group ref=\"Fields\"/>",
        "<xs:sequence><xs:group ref=\"Fields\"/></xs:sequence>",
    ] {
        let d = Dir::new();
        let schema = d.import(GROUP, particle).unwrap();
        assert!(!schema.child("Value").unwrap().xml_optional);
        d.roundtrip(&schema);
    }
}
#[test]
fn optional_wrapper_does_not_change_siblings_or_element_local_children() {
    let d = Dir::new();
    let schema=d.import("",r#"<xs:sequence><xs:sequence minOccurs="0"><xs:element name="Maybe"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence><xs:element name="Must" type="xs:string"/></xs:sequence>"#).unwrap();
    assert!(schema.child("Maybe").unwrap().xml_optional);
    assert!(
        !schema
            .child("Maybe")
            .unwrap()
            .child("Value")
            .unwrap()
            .xml_optional
    );
    assert!(!schema.child("Must").unwrap().xml_optional);
    d.roundtrip(&schema);
}
#[test]
fn optional_multiple_resolved_members_and_optional_all_reject_correlation() {
    let two = format!("{VALUE}<xs:element name=\"Other\" type=\"xs:string\"/>");
    let d = Dir::new();
    assert!(
        matches!(d.import("",&format!("<xs:all minOccurs=\"0\">{two}</xs:all>")),Err(XmlFormatError::UnsupportedOptionalCompositor { compositor, element_count:2 }) if compositor=="all")
    );
    let declaration =
        format!("<xs:group name=\"Fields\"><xs:sequence>{two}</xs:sequence></xs:group>");
    assert!(matches!(
        d.import(
            &declaration,
            "<xs:sequence minOccurs=\"0\"><xs:group ref=\"Fields\"/></xs:sequence>"
        ),
        Err(XmlFormatError::UnsupportedOptionalSequence { element_count: 2 })
    ));
    assert!(matches!(
        d.import(&declaration, "<xs:group ref=\"Fields\" minOccurs=\"0\"/>"),
        Err(XmlFormatError::UnsupportedSchemaGroup { .. })
    ));
}
#[test]
fn optional_wrapper_over_repetition_does_not_discard_an_occurrence_constraint() {
    let d = Dir::new();
    assert!(matches!(d.import("",r#"<xs:sequence minOccurs="0"><xs:element name="Value" type="xs:string" maxOccurs="unbounded"/></xs:sequence>"#),Err(XmlFormatError::UnsupportedOptionalSequence { element_count:1 })));
    let declarations = r#"<xs:group name="Fields"><xs:sequence><xs:element name="Value" type="xs:string" maxOccurs="unbounded"/></xs:sequence></xs:group>"#;
    assert!(matches!(
        d.import(declarations, "<xs:group ref=\"Fields\" minOccurs=\"0\"/>"),
        Err(XmlFormatError::UnsupportedSchemaGroup { .. })
    ));
}
#[test]
fn optional_wrapper_keeps_recursive_identity_and_concrete_default_type() {
    let d = Dir::new();
    let xml = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Entry"><xs:sequence minOccurs="0"><xs:element name="Again" type="Entry"/></xs:sequence></xs:complexType><xs:complexType name="Special"><xs:complexContent><xs:extension base="Entry"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Entry"/></xs:schema>"#;
    let schema = xsd::import_root(&d.file("source.xsd", xml), Some("Root")).unwrap();
    assert!(schema.child("Again").unwrap().xml_optional);
    assert_eq!(schema.xml_default_type.as_deref(), Some("Entry"));
    assert!(schema.child("Again").unwrap().recursive_ref.is_some());
    d.roundtrip(&schema);
}
#[test]
fn optionality_after_resolution_preserves_foreign_global_identity() {
    let d = Dir::new();
    d.file("foreign.xsd",r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:optional:wrapper"><xs:element name="Value" type="xs:string"/><xs:group name="Fields"><xs:sequence><xs:element ref="f:Value" xmlns:f="urn:ferrule:optional:wrapper"/></xs:sequence></xs:group></xs:schema>"#);
    let xml = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:optional:wrapper"><xs:import namespace="urn:ferrule:optional:wrapper" schemaLocation="foreign.xsd"/><xs:element name="Root"><xs:complexType><xs:sequence minOccurs="0"><xs:group ref="f:Fields"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let schema = xsd::import_root(&d.file("source.xsd", xml), Some("Root")).unwrap();
    assert!(schema.child("Value").unwrap().xml_optional);
    d.roundtrip(&schema);
    assert!(
        !xsd::import_root(&d.0.join("foreign.xsd"), Some("Value"))
            .unwrap()
            .xml_optional
    );
}

#[test]
fn optional_choice_keeps_collective_occurrence_and_exclusive_members() {
    let d = Dir::new();
    for min in ["0", "00", "+0", " 0 "] {
        let particle = format!(
            r#"<xs:choice minOccurs="{min}" maxOccurs="+01"><xs:element name="Value" type="xs:string"/><xs:element name="Other" type="xs:string"/></xs:choice>"#
        );
        let schema = d.import("", &particle).unwrap();
        assert!(!schema.child("Value").unwrap().xml_optional);
        assert!(!schema.child("Other").unwrap().xml_optional);
        assert_eq!(schema.xml_repeating_choices.len(), 1);
        assert!(!schema.xml_repeating_choices[0].required);
        assert!(!schema.xml_repeating_choices[0].repeating);
        d.roundtrip(&schema);
    }
}
#[test]
fn unsupported_optional_choice_shape_rejects_without_independent_optional_fields() {
    let d = Dir::new();
    let particle = r#"<xs:choice minOccurs="0"><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Other" type="xs:string"/></xs:sequence><xs:element name="Third" type="xs:string"/></xs:choice>"#;
    assert!(
        matches!(d.import("",particle),Err(XmlFormatError::UnsupportedOptionalCompositor { compositor,element_count:3 }) if compositor=="choice")
    );
}

#[test]
fn optional_strict_wildcard_choice_uses_exact_zero_lexical_forms() {
    let d = Dir::new();
    d.file("foreign.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:optional:wildcard"><xs:element name="Item" type="xs:string"/></xs:schema>"#);
    for minimum in ["0", "00", "+0", " 0 "] {
        let source = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:import namespace="urn:ferrule:optional:wildcard" schemaLocation="foreign.xsd"/><xs:element name="Root"><xs:complexType><xs:choice minOccurs="{minimum}"><xs:element name="Local" type="xs:string"/><xs:any namespace="urn:ferrule:optional:wildcard" processContents="strict"/></xs:choice></xs:complexType></xs:element></xs:schema>"#
        );
        let schema = xsd::import_root(&d.file("source.xsd", &source), Some("Root")).unwrap();
        assert_eq!(schema.xml_repeating_choices.len(), 1);
        assert!(!schema.xml_repeating_choices[0].required);
        assert!(!schema.child("Local").unwrap().xml_optional);
        assert!(!schema.child("Item").unwrap().xml_optional);
        d.roundtrip(&schema);
    }
}
