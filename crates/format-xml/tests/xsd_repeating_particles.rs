use format_xml::xsd;
use ir::{ScalarType, SchemaKind};

#[test]
fn projects_repeating_multi_element_particles_as_repeated_named_fields() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_repeating_tuple_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Rows">
    <xs:complexType>
      <xs:sequence maxOccurs="unbounded">
        <xs:element name="Code" type="xs:string"/>
        <xs:sequence>
          <xs:element name="Amount" type="xs:decimal"/>
        </xs:sequence>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = xsd::import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(schema.xml_repeating_sequences.len(), 1);

    let exported_text = xsd::export(&schema).unwrap();
    assert!(exported_text.contains("<xs:sequence maxOccurs=\"unbounded\">"));
    assert!(exported_text.contains("<xs:element name=\"Code\" type=\"xs:string\"/>"));
    assert!(exported_text.contains("<xs:element name=\"Amount\" type=\"xs:decimal\"/>"));

    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("expected imported root group");
    };
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, "Code");
    assert_eq!(children[1].name, "Amount");
    assert!(children.iter().all(|child| child.repeating));
}

#[test]
fn ignores_unreachable_repeating_multi_element_particles() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_unreachable_tuple_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Root" type="xs:string"/>
  <xs:complexType name="UnusedRows">
    <xs:sequence maxOccurs="unbounded">
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Amount" type="xs:decimal"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = xsd::import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(schema.name, "Root");
    assert!(matches!(
        schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
}

#[test]
fn ignores_disabled_particles_when_checking_a_repeating_sequence() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_disabled_tuple_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Rows">
    <xs:complexType>
      <xs:sequence maxOccurs="unbounded">
        <xs:element name="Code" type="xs:string"/>
        <xs:element name="Never" type="xs:string" maxOccurs="0"/>
        <xs:choice maxOccurs="+000">
          <xs:element name="DisabledA" type="xs:string"/>
          <xs:sequence>
            <xs:element name="DisabledB" type="xs:string"/>
            <xs:element name="DisabledC" type="xs:string"/>
          </xs:sequence>
        </xs:choice>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = xsd::import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let SchemaKind::Group { children, .. } = schema.kind else {
        panic!("expected imported root group");
    };
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "Code");
    assert!(children[0].repeating);
}

#[test]
fn keeps_repeating_choice_import_best_effort() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_repeating_choice_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Values">
    <xs:complexType>
      <xs:choice maxOccurs="unbounded">
        <xs:element name="Code" type="xs:string"/>
        <xs:element name="Amount" type="xs:decimal"/>
      </xs:choice>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = xsd::import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let SchemaKind::Group { children, .. } = schema.kind else {
        panic!("expected imported root group");
    };
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|child| child.repeating));
}

#[test]
fn rejects_choice_nested_inside_a_repeating_sequence() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_repeating_nested_choice_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Rows">
    <xs:complexType>
      <xs:sequence maxOccurs="unbounded">
        <xs:element name="Code" type="xs:string"/>
        <xs:choice>
          <xs:element name="Text" type="xs:string"/>
          <xs:element name="Amount" type="xs:decimal"/>
        </xs:choice>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let result = xsd::import(&path);
    std::fs::remove_file(&path).unwrap();

    assert!(matches!(
        result,
        Err(format_xml::XmlFormatError::UnsupportedRepeatingSequenceCompositor {
            compositor
        }) if compositor == "choice"
    ));
}

#[test]
fn rejects_multi_member_repetition_nested_inside_a_repeating_sequence() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_nested_repeating_tuple_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Rows"><xs:complexType>
    <xs:sequence maxOccurs="unbounded">
      <xs:sequence maxOccurs="unbounded">
        <xs:element name="Code" type="xs:string"/>
        <xs:element name="Amount" type="xs:decimal"/>
      </xs:sequence>
      <xs:element name="Note" type="xs:string"/>
    </xs:sequence>
  </xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();

    let result = xsd::import(&path);
    std::fs::remove_file(&path).unwrap();

    assert!(matches!(
        result,
        Err(format_xml::XmlFormatError::UnsupportedNestedRepeatingSequence { element_count: 2 })
    ));
}
