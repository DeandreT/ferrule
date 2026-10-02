use format_xml::xsd;
use ir::{GroupAlternative, Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ferrule_xml_default_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str, value: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, value).unwrap();
        p
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn plain() -> Instance {
    Instance::Group((vec![("Value".into(), Instance::Scalar(Value::String("x".into())))]).into())
}
fn xml_type(xml: &str) -> Option<String> {
    roxmltree::Document::parse(xml)
        .unwrap()
        .root_element()
        .attribute(("http://www.w3.org/2001/XMLSchema-instance", "type"))
        .map(str::to_string)
}
fn imported(dir: &Dir, name: &str, text: &str) -> SchemaNode {
    xsd::import(&dir.file(name, text)).unwrap()
}
const CONCRETE: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="B"><xs:complexContent><xs:extension base="A"/></xs:complexContent></xs:complexType><xs:element name="Root" type="A"/></xs:schema>"#;
const ABSTRACT: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Hidden" abstract="true"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="A"><xs:complexContent><xs:extension base="Hidden"/></xs:complexContent></xs:complexType><xs:complexType name="B"><xs:complexContent><xs:extension base="Hidden"/></xs:complexContent></xs:complexType><xs:element name="Root" type="Hidden"/></xs:schema>"#;
#[test]
fn concrete_and_abstract_equal_projections_retain_different_defaults() {
    let d = Dir::new();
    let concrete = imported(&d, "concrete.xsd", CONCRETE);
    let abstract_base = imported(&d, "abstract.xsd", ABSTRACT);
    assert_eq!(concrete.kind, abstract_base.kind);
    assert_ne!(concrete, abstract_base);
    assert_eq!(concrete.xml_default_type.as_deref(), Some("A"));
    assert!(abstract_base.xml_default_type.is_none());
    let xml = "<Root><Value>x</Value></Root>";
    let mut observed_plain = plain();
    if let Instance::Group(fields) = &mut observed_plain {
        fields
            .set_xml_type_origin(ir::XmlTypeOrigin::Absent)
            .unwrap();
    }
    assert_eq!(
        format_xml::from_str(xml, &concrete).unwrap(),
        observed_plain
    );
    assert_eq!(
        observed_plain.xml_type_origin(),
        Ok(ir::XmlTypeOrigin::Absent)
    );
    assert_eq!(plain().xml_type_origin(), Ok(ir::XmlTypeOrigin::Unknown));
    assert!(matches!(
        format_xml::from_str(xml, &abstract_base),
        Err(format_xml::XmlFormatError::AmbiguousAlternative { .. })
    ));
    assert!(xml_type(&format_xml::to_string(&concrete, &plain()).unwrap()).is_none());
    assert!(matches!(
        format_xml::to_string(&abstract_base, &plain()),
        Err(format_xml::XmlFormatError::AmbiguousAlternative { .. })
    ));
    for (name, schema) in [
        ("concrete-export.xsd", concrete),
        ("abstract-export.xsd", abstract_base),
    ] {
        let text = xsd::export(&schema).unwrap();
        let restored = xsd::import(&d.file(name, &text)).unwrap();
        assert_eq!(restored, schema);
    }
}
#[test]
fn explicit_base_and_derived_annotations_survive_copies() {
    let d = Dir::new();
    let schema = imported(&d, "root.xsd", CONCRETE);
    for name in ["A", "B"] {
        let xml = format!(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="{name}"><Value>x</Value></Root>"#
        );
        let value = format_xml::from_str(&xml, &schema).unwrap();
        assert_eq!(
            value.field(XML_TYPE_FIELD).and_then(Instance::as_scalar),
            Some(&Value::String(name.into()))
        );
        assert_ne!(value, plain());
        let output = format_xml::to_string(&schema, &value.clone()).unwrap();
        assert_eq!(xml_type(&output).as_deref(), Some(name));
        assert_eq!(format_xml::from_str(&output, &schema).unwrap(), value);
    }
}
#[test]
fn qualified_default_and_constructed_derived_projection_are_exact() {
    let d = Dir::new();
    let text = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:q="urn:ferrule:default" targetNamespace="urn:ferrule:default"><xs:complexType name="Base"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:extension base="q:Base"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="q:Base"/></xs:schema>"#;
    let schema = imported(&d, "root.xsd", text);
    assert_eq!(
        schema.xml_default_type.as_deref(),
        Some("{urn:ferrule:default}Base")
    );
    assert!(xml_type(&format_xml::to_string(&schema, &plain()).unwrap()).is_none());
    let derived = Instance::Group(
        (vec![
            ("Value".into(), Instance::Scalar(Value::String("x".into()))),
            ("Extra".into(), Instance::Scalar(Value::String("y".into()))),
        ])
        .into(),
    );
    let out = format_xml::to_string(&schema, &derived).unwrap();
    let doc = roxmltree::Document::parse(&out).unwrap();
    let root = doc.root_element();
    let name = root
        .attribute(("http://www.w3.org/2001/XMLSchema-instance", "type"))
        .unwrap();
    let (prefix, local) = name.split_once(':').unwrap();
    assert_eq!(local, "Derived");
    assert_eq!(
        root.lookup_namespace_uri(Some(prefix)),
        Some("urn:ferrule:default")
    );
    let mut imported_derived = derived.clone();
    if let Instance::Group(fields) = &mut imported_derived {
        fields.push((
            XML_TYPE_FIELD.into(),
            Instance::Scalar(Value::String("{urn:ferrule:default}Derived".into())),
        ));
        fields
            .set_xml_type_origin(ir::XmlTypeOrigin::Explicit("{urn:ferrule:default}Derived"))
            .unwrap();
    }
    assert_eq!(derived.xml_type_origin(), Ok(ir::XmlTypeOrigin::Unknown));
    assert_eq!(
        format_xml::from_str(&out, &schema).unwrap(),
        imported_derived
    );
    let reimported = xsd::import(&d.file("export.xsd", &xsd::export(&schema).unwrap())).unwrap();
    assert_eq!(schema, reimported);
}
#[test]
fn legacy_unknown_default_keeps_explicit_annotation_behavior() {
    let mut schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .with_alternatives(vec![GroupAlternative {
        name: "A".into(),
        members: vec!["Value".into()],
        required: Vec::new(),
        constraints: Vec::new(),
    }])
    .unwrap();
    schema.xml_type_alternatives = true;
    let input = format_xml::from_str("<Root><Value>x</Value></Root>", &schema).unwrap();
    assert!(input.field(XML_TYPE_FIELD).is_some());
    assert_eq!(
        xml_type(&format_xml::to_string(&schema, &plain()).unwrap()).as_deref(),
        Some("A")
    );
}
#[test]
fn single_concrete_default_retains_its_exported_projection() {
    for namespace in [None, Some("urn:ferrule:single-default")] {
        let identity =
            namespace.map_or_else(|| "A".into(), |namespace| format!("{{{namespace}}}A"));
        let mut schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .with_alternatives(vec![GroupAlternative {
            name: identity.clone(),
            members: vec!["Value".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }])
        .unwrap();
        schema.xml_type_alternatives = true;
        schema.xml_default_type = Some(identity);
        if let Some(ns) = namespace {
            schema = schema.xml_qualified(ns).unwrap();
        }
        let d = Dir::new();
        let exported = xsd::export(&schema).unwrap();
        let reimported = xsd::import(&d.file("root.xsd", &exported)).unwrap();
        assert_eq!(schema, reimported);
        assert!(xml_type(&format_xml::to_string(&reimported, &plain()).unwrap()).is_none());
    }
}
#[test]
fn concrete_and_abstract_views_can_share_type_definitions_in_either_order() {
    for reverse in [false, true] {
        let d = Dir::new();
        let children = if reverse {
            r#"<xs:element name="Abstract" type="Hidden"/><xs:element name="Concrete" type="A"/>"#
        } else {
            r#"<xs:element name="Concrete" type="A"/><xs:element name="Abstract" type="Hidden"/>"#
        };
        let types=ABSTRACT.replace(r#"name="B"><xs:complexContent><xs:extension base="Hidden""#,r#"name="B"><xs:complexContent><xs:extension base="A""#).replace(r#"<xs:element name="Root" type="Hidden"/>"#,&format!(r#"<xs:element name="Root"><xs:complexType><xs:sequence>{children}</xs:sequence></xs:complexType></xs:element>"#));
        let schema = imported(&d, "input.xsd", &types);
        assert_eq!(
            schema
                .child("Concrete")
                .unwrap()
                .xml_default_type
                .as_deref(),
            Some("A")
        );
        assert!(schema.child("Abstract").unwrap().xml_default_type.is_none());
        let exported = xsd::export(&schema).unwrap();
        let restored = xsd::import(&d.file("export.xsd", &exported)).unwrap();
        assert_eq!(schema, restored);
    }
}
#[test]
fn invalid_programmatic_default_rejects_at_both_xml_boundaries() {
    let mut schema = SchemaNode::group("Root", vec![]);
    schema.xml_default_type = Some("Missing".into());
    assert!(matches!(
        format_xml::from_str("<Root/>", &schema),
        Err(format_xml::XmlFormatError::InvalidXmlDefaultType { .. })
    ));
    assert!(matches!(
        format_xml::to_string(&schema, &Instance::Group(vec![].into())),
        Err(format_xml::XmlFormatError::InvalidXmlDefaultType { .. })
    ));
    assert!(matches!(
        xsd::export(&schema),
        Err(format_xml::XmlFormatError::InvalidXmlDefaultType { .. })
    ));
}

#[test]
fn declared_default_survives_reordered_alternatives_and_restrictions() {
    let d = Dir::new();
    let input = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base"><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Note" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="Compact"><xs:complexContent><xs:restriction base="Base"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:restriction></xs:complexContent></xs:complexType><xs:complexType name="Extended"><xs:complexContent><xs:extension base="Base"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Base"/></xs:schema>"#;
    let mut schema = imported(&d, "input.xsd", input);
    assert_eq!(schema.xml_default_type.as_deref(), Some("Base"));
    if let ir::SchemaKind::Group { alternatives, .. } = &mut schema.kind {
        alternatives.reverse();
    }
    assert!(schema.xml_default_type_is_valid());
    let plain = format_xml::from_str("<Root><Value>x</Value></Root>", &schema).unwrap();
    assert!(plain.field(XML_TYPE_FIELD).is_none());
    assert!(xml_type(&format_xml::to_string(&schema, &plain).unwrap()).is_none());
    for identity in ["Base", "Compact", "Extended"] {
        let xml = format!(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="{identity}"><Value>x</Value></Root>"#
        );
        let explicit = format_xml::from_str(&xml, &schema).unwrap();
        assert_eq!(
            xml_type(&format_xml::to_string(&schema, &explicit).unwrap()).as_deref(),
            Some(identity)
        );
    }
    let restored = imported(&d, "export.xsd", &xsd::export(&schema).unwrap());
    assert_eq!(restored, schema);
    schema.xml_default_type = None;
    let restored = imported(&d, "unknown.xsd", &xsd::export(&schema).unwrap());
    assert_eq!(restored, schema);
    assert!(restored.xml_default_type.is_none());
}
#[test]
fn recursive_default_declaration_preserves_explicit_and_unannotated_groups() {
    let d = Dir::new();
    let input = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="Base"><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Again" type="Base" minOccurs="0" maxOccurs="unbounded"/></xs:sequence></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:extension base="Base"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="Base"/></xs:schema>"#;
    let schema = imported(&d, "input.xsd", input);
    assert_eq!(schema.xml_default_type.as_deref(), Some("Base"));
    let exported = xsd::export(&schema).unwrap();
    let restored = imported(&d, "export.xsd", &exported);
    for input in [
        r#"<Root><Value>x</Value><Again><Value>y</Value></Again></Root>"#,
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived"><Value>x</Value><Again xsi:type="Base"><Value>y</Value></Again><Extra>z</Extra></Root>"#,
    ] {
        let value = format_xml::from_str(input, &schema).unwrap();
        assert_eq!(format_xml::from_str(input, &restored).unwrap(), value);
        let output = format_xml::to_string(&restored, &value).unwrap();
        assert_eq!(format_xml::from_str(&output, &restored).unwrap(), value);
    }
}
