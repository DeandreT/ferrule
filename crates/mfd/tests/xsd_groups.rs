use ir::{Instance, Value};

use test_support::TempDir;

#[test]
fn named_xsd_groups_supply_executable_ports_and_roundtrip() {
    let directory = TempDir::new("mfd_xsd_groups").unwrap();
    directory
        .write(
            "shared.xsd",
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:mfd-groups"
                targetNamespace="urn:ferrule:mfd-groups"
                elementFormDefault="qualified">
          <xs:group name="AuditFields"><xs:sequence>
            <xs:element name="Actor" type="xs:string"/>
            <xs:element name="Timestamp" type="xs:string"/>
          </xs:sequence></xs:group>
          <xs:attributeGroup name="Identity">
            <xs:attribute name="id" type="xs:string"/>
          </xs:attributeGroup>
        </xs:schema>"#,
        )
        .unwrap();
    directory
        .write(
            "source.xsd",
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:mfd-groups"
                targetNamespace="urn:ferrule:mfd-groups"
                elementFormDefault="qualified">
          <xs:include schemaLocation="shared.xsd"/>
          <xs:element name="Envelope"><xs:complexType>
            <xs:sequence><xs:group ref="t:AuditFields"/></xs:sequence>
            <xs:attributeGroup ref="t:Identity"/>
          </xs:complexType></xs:element>
        </xs:schema>"#,
        )
        .unwrap();
    directory
        .write(
            "target.xsd",
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Result"><xs:complexType><xs:sequence>
            <xs:element name="Actor" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
        )
        .unwrap();
    directory.write("mapping.mfd", r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Envelope">
              <entry name="Actor" outkey="10"/><entry name="Timestamp"/><entry name="@id"/>
            </entry></entry></entry>
          </root><document schema="source.xsd" inputinstance="source.xml"
            instanceroot="{urn:ferrule:mfd-groups}Envelope"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Result">
              <entry name="Actor" inpkey="20"/>
            </entry></entry></entry>
          </root><document schema="target.xsd" outputinstance="target.xml"
            instanceroot="{}Result"/></data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#).unwrap();

    let imported = mfd::import(&directory.path().join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.source.child("Actor").is_some());
    assert!(imported.project.source.child("Timestamp").is_some());
    assert!(imported.project.source.child("id").unwrap().attribute);
    let source = format_xml::from_str(
        r#"<Envelope xmlns="urn:ferrule:mfd-groups" id="A-17">
          <Actor>Ada</Actor><Timestamp>2026-07-22T08:30:00</Timestamp>
        </Envelope>"#,
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        target.field("Actor").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );

    let exported_path = directory.path().join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtripped = mfd::import(&exported_path).unwrap();
    assert!(
        roundtripped.warnings.is_empty(),
        "{:?}",
        roundtripped.warnings
    );
    let target = engine::run(&roundtripped.project, &source).unwrap();
    assert_eq!(
        target.field("Actor").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
}
