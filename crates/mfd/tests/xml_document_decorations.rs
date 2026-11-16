use ir::{Instance, Value};

use test_support::TempDir;

#[test]
fn processing_instruction_before_payload_does_not_hide_document_ports()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("mfd_xml_document_decorations")?;
    std::fs::write(
        dir.path().join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.path().join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let mapping = dir.path().join("mapping.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Source">
      <entry name="Item" outkey="10"><entry name="Value" outkey="11"/></entry>
    </entry></entry></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document">
      <entry name="Target" type="processing-instruction-before" inpkey="99"/>
      <entry name="Target" type="comment-before" inpkey="98"/>
      <entry name="Target"><entry name="Row" inpkey="20"><entry name="Value" inpkey="21"/></entry></entry>
    </entry></entry></root>
    <document schema="target.xsd" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&mapping)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = format_xml::from_str(
        "<Source><Item><Value>alpha</Value></Item><Item><Value>beta</Value></Item></Source>",
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    let rows = target
        .field("Row")
        .and_then(Instance::as_repeated)
        .ok_or("missing target rows")?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("beta".into()))
    );
    Ok(())
}
