use format_xml::{XmlFormatError, xsd};
use ir::{Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ferrule_optional_{}_{}",
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
    fn exported(&self, schema: &SchemaNode, filename: &str) -> SchemaNode {
        let set = xsd::export_set(schema, filename).unwrap();
        for a in &set.dependencies {
            self.file(&a.filename, &a.contents);
        }
        xsd::import(&self.file(filename, &set.root)).unwrap()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn child<'a>(s: &'a SchemaNode, name: &str) -> &'a SchemaNode {
    s.child(name).unwrap()
}
const OPTIONAL: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Root"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0" nillable="true"/><xs:element name="Maybe" minOccurs="0"><xs:complexType/></xs:element><xs:element name="Must" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
#[test]
fn direct_scalar_and_group_roundtrip_exact_optional_metadata() {
    let d = Dir::new();
    let s = xsd::import(&d.file("source.xsd", OPTIONAL)).unwrap();
    assert!(child(&s, "Value").xml_optional);
    assert!(child(&s, "Maybe").xml_optional);
    assert!(!child(&s, "Must").xml_optional);
    assert_eq!(d.exported(&s, "export.xsd"), s);
    let doc = xsd::export(&s).unwrap();
    assert!(doc.contains("name=\"Value\" type=\"xs:string\" minOccurs=\"0\""));
}
#[test]
fn lenient_presence_keeps_null_absent_empty_and_nil_distinct() {
    let d = Dir::new();
    let s = xsd::import(&d.file("source.xsd", OPTIONAL)).unwrap();
    let absent = format_xml::from_str("<Root><Must>x</Must></Root>", &s).unwrap();
    assert_eq!(absent.field("Value"), Some(&Instance::Scalar(Value::Null)));
    assert!(absent.field("Maybe").is_none());
    let empty = format_xml::from_str("<Root><Value/><Maybe/><Must>x</Must></Root>", &s).unwrap();
    assert_eq!(
        empty.field("Value"),
        Some(&Instance::Scalar(Value::String(String::new())))
    );
    assert_eq!(
        empty.field("Maybe"),
        Some(&Instance::Group((vec![]).into()))
    );
    let nil = format_xml::from_str(r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Value xsi:nil="true"/><Must>x</Must></Root>"#, &s).unwrap();
    assert_eq!(
        nil.field("Value"),
        Some(&Instance::Scalar(Value::xml_nil()))
    );
    for value in [absent, empty, nil] {
        let xml = format_xml::to_string(&s, &value).unwrap();
        assert_eq!(format_xml::from_str(&xml, &s).unwrap(), value);
    }
    // Required element metadata still does not turn the lenient instance API
    // into full XML Schema validation.
    assert!(format_xml::from_str("<Root/>", &s).is_ok());
}
#[test]
fn local_reference_owns_occurrence_instead_of_global_declaration() {
    let d = Dir::new();
    let text = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Value" type="xs:string"/><xs:element name="Root"><xs:complexType><xs:sequence><xs:element ref="Value" minOccurs="0"/><xs:element name="Must" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    let path = d.file("ref.xsd", text);
    let s = xsd::import_root(&path, Some("Root")).unwrap();
    assert!(child(&s, "Value").xml_optional);
    assert!(!xsd::import_root(&path, Some("Value")).unwrap().xml_optional);
    assert_eq!(d.exported(&s, "export.xsd"), s);
}
#[test]
fn foreign_optional_and_required_references_share_one_global_declaration() {
    let d = Dir::new();
    d.file("foreign.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:optional:foreign"><xs:element name="Value" type="xs:string"/></xs:schema>"#);
    let p = d.file("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:ferrule:optional:foreign"><xs:import namespace="urn:ferrule:optional:foreign" schemaLocation="foreign.xsd"/><xs:element name="Root"><xs:complexType><xs:sequence><xs:element name="Left"><xs:complexType><xs:sequence><xs:element ref="f:Value" minOccurs="0"/></xs:sequence></xs:complexType></xs:element><xs:element name="Right"><xs:complexType><xs:sequence><xs:element ref="f:Value"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    let s = xsd::import(&p).unwrap();
    let set = xsd::export_set(&s, "export.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert_eq!(
        set.dependencies[0]
            .contents
            .matches("name=\"Value\"")
            .count(),
        1
    );
    let dep = roxmltree::Document::parse(&set.dependencies[0].contents).unwrap();
    let value = dep
        .root_element()
        .children()
        .find(|n| n.has_tag_name(("http://www.w3.org/2001/XMLSchema", "element")))
        .unwrap();
    assert!(value.attribute("minOccurs").is_none());
    assert!(child(child(&s, "Left"), "Value").xml_optional);
    assert!(!child(child(&s, "Right"), "Value").xml_optional);
    assert_eq!(d.exported(&s, "export.xsd"), s);
}
#[test]
fn recursive_local_occurrence_does_not_change_named_anchor_definition() {
    let d = Dir::new();
    let p = d.file("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Tree"><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Again" type="Tree" minOccurs="0"/></xs:sequence></xs:complexType><xs:element name="Root" type="Tree"/></xs:schema>"#);
    let s = xsd::import(&p).unwrap();
    assert!(child(&s, "Again").xml_optional);
    assert!(child(&s, "Again").recursive_ref.is_some());
    assert_eq!(d.exported(&s, "export.xsd"), s);
    let xml = "<Root><Value>a</Value><Again><Value>b</Value></Again></Root>";
    let input = format_xml::from_str(xml, &s).unwrap();
    assert_eq!(
        format_xml::from_str(&format_xml::to_string(&s, &input).unwrap(), &s).unwrap(),
        input
    );
}
#[test]
fn singular_choice_and_repeated_members_keep_their_existing_owners() {
    let d = Dir::new();
    let p = d.file("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Root"><xs:complexType><xs:sequence><xs:choice><xs:element name="Left" type="xs:string"/><xs:element name="Right" type="xs:string"/></xs:choice><xs:element name="Rows" type="xs:string" minOccurs="0" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    let s = xsd::import(&p).unwrap();
    assert!(!child(&s, "Left").xml_optional);
    assert!(!child(&s, "Rows").xml_optional);
    assert!(child(&s, "Rows").repeating);
    assert_eq!(d.exported(&s, "export.xsd"), s);
}
const COMPATIBLE: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="B"><xs:complexContent><xs:extension base="A"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="A"/></xs:schema>"#;
#[test]
fn compatible_alternatives_keep_default_identity_occurrences_and_explicit_copies() {
    let d = Dir::new();
    let s = xsd::import(&d.file("source.xsd", COMPATIBLE)).unwrap();
    assert!(s.xml_type_alternatives);
    assert_eq!(s.xml_default_type.as_deref(), Some("A"));
    assert!(child(&s, "Value").xml_optional);
    assert!(child(&s, "Extra").xml_optional);
    assert_eq!(d.exported(&s, "export.xsd"), s);
    for ty in ["A", "B"] {
        let xml = format!(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="{ty}"><Value>x</Value></Root>"#
        );
        let input = format_xml::from_str(&xml, &s).unwrap();
        assert_eq!(
            input.field(XML_TYPE_FIELD),
            Some(&Instance::Scalar(Value::String(ty.into())))
        );
        let output = format_xml::to_string(&s, &input).unwrap();
        assert!(output.contains(&format!("xsi:type=\"{ty}\"")));
        assert_eq!(format_xml::from_str(&output, &s).unwrap(), input);
    }
    assert!(
        format_xml::from_str("<Root/>", &s)
            .unwrap()
            .field(XML_TYPE_FIELD)
            .is_none()
    );
}
#[test]
fn conflicting_required_optional_restriction_cannot_drop_alternatives() {
    let d = Dir::new();
    let p=d.file("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="B"><xs:complexContent><xs:restriction base="A"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:restriction></xs:complexContent></xs:complexType><xs:element name="Root" type="A"/></xs:schema>"#);
    assert!(
        matches!(xsd::import(&p), Err(XmlFormatError::UnsupportedXmlAlternativeOccurrence { field, .. }) if field=="Value")
    );
}
#[test]
fn conflicting_extension_siblings_and_nested_members_reject_explicitly() {
    let d = Dir::new();
    for (a, b, path) in [
        (
            "<xs:element name=\"Value\" type=\"xs:string\" minOccurs=\"0\"/>",
            "<xs:element name=\"Value\" type=\"xs:string\"/>",
            "Value",
        ),
        (
            "<xs:element name=\"Box\"><xs:complexType><xs:sequence><xs:element name=\"Value\" type=\"xs:string\" minOccurs=\"0\"/></xs:sequence></xs:complexType></xs:element>",
            "<xs:element name=\"Box\"><xs:complexType><xs:sequence><xs:element name=\"Value\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element>",
            "Box/Value",
        ),
    ] {
        let text = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Hidden" abstract="true"/><xs:complexType name="A"><xs:complexContent><xs:extension base="Hidden"><xs:sequence>{a}</xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:complexType name="B"><xs:complexContent><xs:extension base="Hidden"><xs:sequence>{b}</xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Hidden"/></xs:schema>"#
        );
        assert!(
            matches!(xsd::import(&d.file("source.xsd",&text)),Err(XmlFormatError::UnsupportedXmlAlternativeOccurrence{field,..}) if field==path)
        );
    }
}
#[test]
fn restriction_widening_required_to_optional_is_not_admitted() {
    let d = Dir::new();
    let text=COMPATIBLE.replace("minOccurs=\"0\"", "").replace("<xs:extension base=\"A\"><xs:sequence><xs:element name=\"Extra\" type=\"xs:string\" /></xs:sequence></xs:extension>","<xs:restriction base=\"A\"><xs:sequence><xs:element name=\"Value\" type=\"xs:string\" minOccurs=\"0\"/></xs:sequence></xs:restriction>");
    assert!(matches!(
        xsd::import(&d.file("source.xsd", &text)),
        Err(XmlFormatError::UnsupportedComplexContentRestriction { .. })
    ));
}
#[test]
fn programmatic_invalid_roles_reject_at_xml_and_xsd_boundaries() {
    let mut s = SchemaNode::scalar("Value", ScalarType::String).repeating();
    s.xml_optional = true;
    assert!(matches!(
        format_xml::from_str("<Value/>", &s),
        Err(XmlFormatError::InvalidXmlOptional { .. })
    ));
    assert!(matches!(
        format_xml::to_string(&s, &Instance::Repeated(vec![])),
        Err(XmlFormatError::InvalidXmlOptional { .. })
    ));
    assert!(matches!(
        xsd::export(&s),
        Err(XmlFormatError::InvalidXmlOptional { .. })
    ));
    assert!(matches!(
        xsd::export_set(&s, "root.xsd"),
        Err(XmlFormatError::InvalidXmlOptional { .. })
    ));
}
#[test]
fn unrepresentable_alternative_compositors_reject_instead_of_becoming_sequences() {
    let d = Dir::new();
    let text = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base"><xs:choice><xs:element name="Left" type="xs:string"/><xs:element name="Right" type="xs:string"/></xs:choice></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:extension base="Base"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Base"/></xs:schema>"#;
    let s = xsd::import(&d.file("source.xsd", text)).unwrap();
    assert!(s.xml_type_alternatives);
    assert_eq!(s.xml_repeating_choices.len(), 1);
    assert!(matches!(
        xsd::export(&s),
        Err(XmlFormatError::UnsupportedXmlAlternativeCompositor { .. })
    ));
    assert!(matches!(
        xsd::export_set(&s, "export.xsd"),
        Err(XmlFormatError::UnsupportedXmlAlternativeCompositor { .. })
    ));
}
#[test]
fn per_alternative_required_profile_conflict_has_typed_export_error() {
    let mut value = SchemaNode::scalar("Value", ScalarType::String);
    value.set_xml_optional(true);
    let s = SchemaNode::group("Root", vec![value])
        .with_alternatives(vec![
            ir::GroupAlternative {
                name: "A".into(),
                members: vec!["Value".into()],
                required: vec![],
                constraints: vec![],
            },
            ir::GroupAlternative {
                name: "B".into(),
                members: vec!["Value".into()],
                required: vec!["Value".into()],
                constraints: vec![],
            },
        ])
        .unwrap();
    assert!(matches!(
        xsd::export(&s),
        Err(XmlFormatError::UnsupportedXmlAlternativeOccurrence { .. })
    ));
    assert!(matches!(
        xsd::export_set(&s, "root.xsd"),
        Err(XmlFormatError::UnsupportedXmlAlternativeOccurrence { .. })
    ));
}
#[test]
fn zero_occurrence_lexical_forms_preserve_singular_optional_fields() {
    let d = Dir::new();
    for zero in ["0", "00", "+0", " 0 "] {
        let text = OPTIONAL.replace(
            "minOccurs=\"0\"",
            &format!("minOccurs=\"{zero}\" maxOccurs=\"+01\""),
        );
        let s = xsd::import(&d.file("source.xsd", &text)).unwrap();
        assert!(child(&s, "Value").xml_optional);
        assert!(child(&s, "Maybe").xml_optional);
        assert_eq!(d.exported(&s, "export.xsd"), s);
    }
}
#[test]
fn optional_multi_member_sequences_do_not_lose_correlated_absence() {
    let d = Dir::new();
    let text = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Root"><xs:complexType><xs:sequence minOccurs="0"><xs:element name="Left" type="xs:string"/><xs:element name="Right" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    assert!(matches!(
        xsd::import(&d.file("source.xsd", text)),
        Err(XmlFormatError::UnsupportedOptionalSequence { element_count: 2 })
    ));
}
#[test]
fn derived_only_compositor_cannot_disappear_from_the_type_projection() {
    let d = Dir::new();
    for part in [
        "<xs:choice><xs:element name=\"Left\" type=\"xs:string\"/><xs:element name=\"Right\" type=\"xs:string\"/></xs:choice>",
        "<xs:sequence maxOccurs=\"unbounded\"><xs:element name=\"Left\" type=\"xs:string\"/><xs:element name=\"Right\" type=\"xs:string\"/></xs:sequence>",
    ] {
        let text = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"/><xs:complexType name="B"><xs:complexContent><xs:extension base="A">{part}</xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="A"/></xs:schema>"#
        );
        assert!(matches!(
            xsd::import(&d.file("source.xsd", &text)),
            Err(XmlFormatError::UnsupportedXmlAlternativeCompositor { .. })
        ));
    }
}
