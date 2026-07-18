use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XML_TYPE_FIELD};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_single_xsi_type_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

#[test]
fn one_conditioned_derived_view_extends_its_base_schema_and_executes() {
    let directory = TempDir::new();
    write(
        &directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:address" targetNamespace="urn:ferrule:address"
                elementFormDefault="qualified">
          <xs:complexType name="Address"><xs:sequence>
            <xs:element name="name" type="xs:string"/>
          </xs:sequence></xs:complexType>
          <xs:complexType name="EuropeanAddress"><xs:complexContent>
            <xs:extension base="t:Address"><xs:sequence>
              <xs:element name="postcode" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:complexType name="AmericanAddress"><xs:complexContent>
            <xs:extension base="t:Address"><xs:sequence>
              <xs:element name="state" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:element name="Order"><xs:complexType><xs:sequence>
            <xs:element name="shipTo" type="t:Address"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    write(
        &directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Result"><xs:complexType><xs:sequence>
            <xs:element name="Postcode" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    write(
        &directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Order">
              <entry name="shipTo" displayselectionmode="all"/>
              <entry name="shipTo" outkey="10">
                <condition><expression><function name="equal" library="core">
                  <expression><attribute ns="http://www.w3.org/2001/XMLSchema-instance" name="type"/></expression>
                  <expression><constant value="{urn:ferrule:address}EuropeanAddress" datatype="QName"/></expression>
                </function></expression></condition>
                <entry name="name" outkey="11"/>
                <entry name="postcode" outkey="12"/>
              </entry>
            </entry></entry></entry>
          </root><document schema="source.xsd" inputinstance="source.xml"
            instanceroot="{urn:ferrule:address}Order"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Result">
              <entry name="Postcode" inpkey="20"/>
            </entry></entry></entry>
          </root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Result"/></data></component>
        </children><graph><vertices>
          <vertex vertexkey="12"><edges><edge vertexkey="20"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let address = imported.project.source.child("shipTo").unwrap();
    assert!(address.child("postcode").is_some());
    assert!(address.child("state").is_some());
    assert_eq!(address.alternatives().len(), 3);

    let unconnected_type = format_xml::from_str(
        r#"<Order xmlns="urn:ferrule:address"
             xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
             xmlns:t="urn:ferrule:address">
          <shipTo xsi:type="t:AmericanAddress"><name>Ada</name><state>WA</state></shipTo>
        </Order>"#,
        &imported.project.source,
    )
    .unwrap();
    assert_eq!(
        unconnected_type
            .field("shipTo")
            .and_then(|address| address.field(XML_TYPE_FIELD))
            .and_then(Instance::as_scalar),
        Some(&Value::String(
            "{urn:ferrule:address}AmericanAddress".into()
        ))
    );

    let source = format_xml::from_str(
        r#"<Order xmlns="urn:ferrule:address"
             xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
             xmlns:t="urn:ferrule:address">
          <shipTo xsi:type="t:EuropeanAddress"><name>Ada</name><postcode>AB12</postcode></shipTo>
        </Order>"#,
        &imported.project.source,
    )
    .unwrap();
    assert_eq!(
        source
            .field("shipTo")
            .and_then(|address| address.field(XML_TYPE_FIELD))
            .and_then(Instance::as_scalar),
        Some(&Value::String(
            "{urn:ferrule:address}EuropeanAddress".into()
        ))
    );
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        target.field("Postcode").and_then(Instance::as_scalar),
        Some(&Value::String("AB12".into()))
    );
}
