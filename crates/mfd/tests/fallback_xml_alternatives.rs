use std::error::Error;
use std::path::{Path, PathBuf};

use ir::{Instance, Value, XML_TYPE_FIELD};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-fallback-alternatives-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

fn write_types(path: &Path, state_type: &str) -> Result<(), std::io::Error> {
    write(
        path,
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:fallback-types"
    targetNamespace="urn:ferrule:fallback-types"
    elementFormDefault="unqualified">
  <xs:complexType name="Address"><xs:sequence>
    <xs:element name="Name" type="xs:string"/>
  </xs:sequence></xs:complexType>
  <xs:complexType name="Domestic"><xs:complexContent>
    <xs:extension base="t:Address"><xs:sequence>
      <xs:element name="State" type="{state_type}"/>
    </xs:sequence></xs:extension>
  </xs:complexContent></xs:complexType>
  <xs:complexType name="International"><xs:complexContent>
    <xs:extension base="t:Address"><xs:sequence>
      <xs:element name="Country" type="xs:string"/>
    </xs:sequence></xs:extension>
  </xs:complexContent></xs:complexType>
</xs:schema>"#
        ),
    )
}

fn write_source_schema(path: &Path) -> Result<(), std::io::Error> {
    write(
        path,
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:fallback-types">
  <xs:import namespace="urn:ferrule:fallback-types" schemaLocation="types.xsd"/>
  <xs:element name="Inbound"><xs:complexType><xs:sequence>
    <xs:element name="Address" type="t:Address"/>
    <xs:any namespace="##any" processContents="skip"
            minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###,
    )
}

fn write_target_schema(path: &Path, types: &str) -> Result<(), std::io::Error> {
    write(
        path,
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:fallback-types" elementFormDefault="unqualified">
  <xs:import namespace="urn:ferrule:fallback-types" schemaLocation="{types}"/>
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Address" type="t:Address"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )
}

fn mapping(target_schema: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root>
    <entry name="FileInstance"><entry name="document"><entry name="Inbound">
      <entry name="Address" outkey="10" displayselectionmode="all"/>
      <entry name="Address">
        <condition><expression><function name="equal" library="core">
          <expression><attribute name="type"
              ns="http://www.w3.org/2001/XMLSchema-instance"/></expression>
          <expression><constant datatype="QName"
              value="{{urn:ferrule:fallback-types}}Domestic"/></expression>
        </function></expression></condition>
        <entry name="Name"/>
        <entry name="State"/>
      </entry>
    </entry></entry></entry>
  </root><document schema="source.xsd" inputinstance="input.xml"
      instanceroot="{{}}Inbound"/></data></component>
  <component name="target" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data><root>
      <entry name="FileInstance"><entry name="document"><entry name="Output">
        <entry name="Address" inpkey="20"/>
      </entry></entry></entry>
    </root><document schema="{target_schema}" outputinstance="output.xml"
        instanceroot="{{}}Output"/></data>
  </component>
</children><graph directed="1">
  <edges><edge edgekey="1"><data><dataconnection type="2"/></data></edge></edges>
  <vertices><vertex vertexkey="10"><edges>
    <edge vertexkey="20" edgekey="1"/>
  </edges></vertex></vertices>
</graph></structure></component></mapping>"#
    )
}

fn write_design(directory: &TempDir, target_schema: &str) -> Result<PathBuf, Box<dyn Error>> {
    write_types(&directory.path("types.xsd"), "xs:string")?;
    write_source_schema(&directory.path("source.xsd"))?;
    write_target_schema(&directory.path("target.xsd"), target_schema)?;
    let design = directory.path("mapping.mfd");
    write(&design, &mapping("target.xsd"))?;
    Ok(design)
}

#[test]
fn copy_all_recovers_exact_missing_fallback_xml_type_alternatives() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("exact")?;
    let design = write_design(&directory, "types.xsd")?;

    let imported = mfd::import(&design)?;
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0].contains("xs:any wildcard cannot be represented"),
        "{:?}",
        imported.warnings
    );
    assert!(engine::validate(&imported.project).is_empty());

    let address_schema = imported
        .project
        .source
        .child("Address")
        .ok_or("source address is missing")?;
    assert!(address_schema.xml_namespace.is_none());
    assert_eq!(address_schema.alternatives().len(), 3);
    assert!(
        address_schema
            .alternatives()
            .iter()
            .any(|alternative| alternative.name == "{urn:ferrule:fallback-types}International")
    );

    let source = format_xml::from_str(
        r#"<Inbound xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    xmlns:t="urn:ferrule:fallback-types">
  <Address xsi:type="t:International">
    <Name>Ada</Name><Country>CA</Country>
  </Address>
</Inbound>"#,
        &imported.project.source,
    )?;
    assert_eq!(
        source
            .field("Address")
            .and_then(|address| address.field(XML_TYPE_FIELD))
            .and_then(Instance::as_scalar),
        Some(&Value::String(
            "{urn:ferrule:fallback-types}International".into()
        ))
    );

    let target = engine::run(&imported.project, &source)?;
    let address = target.field("Address").ok_or("target address is missing")?;
    assert_eq!(
        address.field("Country").and_then(Instance::as_scalar),
        Some(&Value::String("CA".into()))
    );
    Ok(())
}

#[test]
fn incompatible_fallback_xml_type_alternatives_warn_and_keep_the_conditioned_subset()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("incompatible")?;
    write_types(&directory.path("target-types.xsd"), "xs:integer")?;
    let design = write_design(&directory, "target-types.xsd")?;

    let imported = mfd::import(&design)?;
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains(
                "fallback XML type alternatives at `source/Address` could not be recovered exactly",
            )
        }),
        "{:?}",
        imported.warnings
    );
    let address = imported
        .project
        .source
        .child("Address")
        .ok_or("source address is missing")?;
    assert_eq!(address.alternatives().len(), 2);
    assert!(
        address
            .alternatives()
            .iter()
            .all(|alternative| alternative.name != "{urn:ferrule:fallback-types}International")
    );
    Ok(())
}
