use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XmlAlternativeKind};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_substitution_group_{}_{}",
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
fn complex_substitution_group_imports_executes_and_roundtrips() {
    let directory = TempDir::new();
    write(
        &directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:mfd-substitution"
                targetNamespace="urn:ferrule:mfd-substitution"
                elementFormDefault="qualified">
          <xs:complexType name="CreatureType"><xs:sequence>
            <xs:element name="name" type="xs:string"/>
          </xs:sequence></xs:complexType>
          <xs:complexType name="CatType"><xs:complexContent>
            <xs:extension base="t:CreatureType"><xs:sequence>
              <xs:element name="lives" type="xs:integer"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:complexType name="DogType"><xs:complexContent>
            <xs:extension base="t:CreatureType"><xs:sequence>
              <xs:element name="barks" type="xs:boolean"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:element name="Creature" type="t:CreatureType" abstract="true"/>
          <xs:element name="Cat" type="t:CatType" substitutionGroup="t:Creature"/>
          <xs:element name="Dog" type="t:DogType" substitutionGroup="t:Creature"/>
          <xs:element name="Habitat"><xs:complexType><xs:sequence>
            <xs:element ref="t:Creature"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    write(
        &directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Result"><xs:complexType><xs:sequence>
            <xs:element name="Name" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    write(
        &directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Habitat">
              <entry name="Creature"><entry name="name" outkey="10"/>
                <entry name="lives"/><entry name="barks"/>
              </entry>
            </entry></entry></entry>
          </root><document schema="source.xsd" inputinstance="source.xml"
            instanceroot="{urn:ferrule:mfd-substitution}Habitat"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Result">
              <entry name="Name" inpkey="20"/>
            </entry></entry></entry>
          </root><document schema="target.xsd" outputinstance="target.xml"
            instanceroot="{}Result"/></data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let creature = imported.project.source.child("Creature").unwrap();
    assert_eq!(
        creature.xml_alternative_kind,
        XmlAlternativeKind::SubstitutionGroup
    );
    assert_eq!(creature.alternatives().len(), 2);
    let cat_xml = r#"<Habitat xmlns="urn:ferrule:mfd-substitution">
      <Cat><name>Ada</name><lives>9</lives></Cat>
    </Habitat>"#;
    let source = format_xml::from_str(cat_xml, &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        target.field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );

    let exported_path = directory.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtripped = mfd::import(&exported_path).unwrap();
    assert!(
        roundtripped.warnings.is_empty(),
        "{:?}",
        roundtripped.warnings
    );
    assert_eq!(
        roundtripped
            .project
            .source
            .child("Creature")
            .unwrap()
            .xml_alternative_kind,
        XmlAlternativeKind::SubstitutionGroup
    );
    let dog_xml = r#"<Habitat xmlns="urn:ferrule:mfd-substitution">
      <Dog><name>Byron</name><barks>true</barks></Dog>
    </Habitat>"#;
    let source = format_xml::from_str(dog_xml, &roundtripped.project.source).unwrap();
    let target = engine::run(&roundtripped.project, &source).unwrap();
    assert_eq!(
        target.field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Byron".into()))
    );
}
