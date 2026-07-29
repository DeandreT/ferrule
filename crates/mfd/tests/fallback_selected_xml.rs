use std::error::Error;
use std::path::{Path, PathBuf};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-selected-xml-{tag}-{}-{}",
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

fn write_payload_schema(path: &Path, namespace: &str, field: &str) -> Result<(), std::io::Error> {
    write(
        path,
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="{namespace}" elementFormDefault="qualified">
  <xs:element name="Order"><xs:complexType><xs:sequence>
    <xs:element name="{field}" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#
        ),
    )
}

fn write_target_schema(path: &Path) -> Result<(), std::io::Error> {
    write(
        path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Results"><xs:complexType><xs:sequence>
    <xs:element name="Result" minOccurs="0" maxOccurs="unbounded">
      <xs:complexType><xs:sequence>
        <xs:element name="Code" type="xs:string"/>
      </xs:sequence></xs:complexType>
    </xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )
}

fn mapping(selections: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root>
    <entry name="FileInstance"><entry name="document"><entry name="Envelope">
      <entry name="Body">
        <entry name="*"><selections>{selections}</selections></entry>
        <entry name="Order" outkey="11"><entry name="Code" outkey="10"/></entry>
      </entry>
    </entry></entry></entry>
  </root><document schema="source.xsd" inputinstance="input.xml"
      instanceroot="{{urn:ferrule:envelope}}Envelope"/></data></component>
  <component name="target" library="xml" kind="14">
    <properties XSLTDefaultOutput="1"/><data><root>
      <entry name="FileInstance"><entry name="document"><entry name="Results">
        <entry name="Result" inpkey="21"><entry name="Code" inpkey="20"/></entry>
      </entry></entry></entry>
    </root><document schema="target.xsd" outputinstance="output.xml"
        instanceroot="{{}}Results"/></data>
  </component>
</children><graph directed="1">
<edges><edge edgekey="1"><data><dataconnection type="2"/></data></edge></edges>
<vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21" edgekey="1"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

#[test]
fn selected_qname_replaces_an_existing_fallback_placeholder_and_executes()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("exact")?;
    write_payload_schema(
        &directory.path("payload.xsd"),
        "urn:ferrule:selected-order",
        "Code",
    )?;
    write(
        &directory.path("source.xsd"),
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:envelope"
    xmlns:e="urn:ferrule:envelope">
  <xs:import namespace="urn:ferrule:selected-order" schemaLocation="payload.xsd"/>
  <xs:element name="Envelope"><xs:complexType><xs:sequence>
    <xs:element name="Body"><xs:complexType><xs:sequence>
      <xs:any namespace="##other" processContents="strict"
              minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###,
    )?;
    write_target_schema(&directory.path("target.xsd"))?;
    let design = directory.path("mapping.mfd");
    write(
        &design,
        &mapping(r#"<qname QNameAsString="{urn:ferrule:selected-order}Order"/>"#),
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let order_schema = imported
        .project
        .source
        .child("Body")
        .and_then(|body| body.child("Order"))
        .ok_or("selected order schema is missing")?;
    assert!(order_schema.repeating);
    assert!(order_schema.child("Code").is_some());

    let source = format_xml::from_str(
        r#"<e:Envelope xmlns:e="urn:ferrule:envelope"
    xmlns:p="urn:ferrule:selected-order">
  <Body>
    <p:Order><p:Code>A</p:Code></p:Order>
    <p:Order><p:Code>B</p:Code></p:Order>
  </Body>
</e:Envelope>"#,
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    let results = target
        .field("Result")
        .and_then(Instance::as_repeated)
        .ok_or("target results are not repeated")?;
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.field("Code"))
            .filter_map(Instance::as_scalar)
            .collect::<Vec<_>>(),
        vec![&Value::String("A".into()), &Value::String("B".into())]
    );
    Ok(())
}

#[test]
fn colliding_selected_qnames_warn_and_retain_the_fallback_placeholder() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new("ambiguous")?;
    write_payload_schema(
        &directory.path("payload-a.xsd"),
        "urn:ferrule:selected-a",
        "Code",
    )?;
    write_payload_schema(
        &directory.path("payload-b.xsd"),
        "urn:ferrule:selected-b",
        "Reference",
    )?;
    write(
        &directory.path("source.xsd"),
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:envelope">
  <xs:import namespace="urn:ferrule:selected-a" schemaLocation="payload-a.xsd"/>
  <xs:import namespace="urn:ferrule:selected-b" schemaLocation="payload-b.xsd"/>
  <xs:element name="Envelope"><xs:complexType><xs:sequence>
    <xs:element name="Body"><xs:complexType><xs:sequence>
      <xs:any namespace="##other" processContents="strict"
              minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###,
    )?;
    write_target_schema(&directory.path("target.xsd"))?;
    let design = directory.path("mapping.mfd");
    write(
        &design,
        &mapping(
            r#"<qname QNameAsString="{urn:ferrule:selected-a}Order"/>
               <qname QNameAsString="{urn:ferrule:selected-b}Order"/>"#,
        ),
    )?;

    let imported = mfd::import(&design)?;
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("multiple qualified names share that mapping path")
                && warning.contains("Body/Order")
        }),
        "{:?}",
        imported.warnings
    );
    let order_schema = imported
        .project
        .source
        .child("Body")
        .and_then(|body| body.child("Order"))
        .ok_or("fallback order schema is missing")?;
    assert!(!order_schema.repeating);
    assert!(order_schema.child("Code").is_some());
    Ok(())
}

#[test]
fn structured_selected_field_does_not_replace_an_exposed_fallback_scalar()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new("incompatible")?;
    write(
        &directory.path("payload.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:selected-order" elementFormDefault="qualified">
  <xs:element name="Order"><xs:complexType><xs:sequence>
    <xs:element name="Code"><xs:complexType><xs:sequence>
      <xs:element name="Part" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &directory.path("source.xsd"),
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:ferrule:envelope">
  <xs:import namespace="urn:ferrule:selected-order" schemaLocation="payload.xsd"/>
  <xs:element name="Envelope"><xs:complexType><xs:sequence>
    <xs:element name="Body"><xs:complexType><xs:sequence>
      <xs:any namespace="##other" processContents="strict"
              minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"###,
    )?;
    write_target_schema(&directory.path("target.xsd"))?;
    let design = directory.path("mapping.mfd");
    write(
        &design,
        &mapping(r#"<qname QNameAsString="{urn:ferrule:selected-order}Order"/>"#),
    )?;

    let imported = mfd::import(&design)?;
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("field `Code` is exposed as a scalar")
                && warning.contains("incompatible scalar connections are non-executable")
        }),
        "{:?}",
        imported.warnings
    );
    let order_schema = imported
        .project
        .source
        .child("Body")
        .and_then(|body| body.child("Order"))
        .ok_or("resolved order schema is missing")?;
    assert!(order_schema.repeating);
    assert!(
        order_schema
            .child("Code")
            .is_some_and(|code| !code.is_scalar())
    );
    let validation = engine::validate(&imported.project);
    assert!(
        validation
            .iter()
            .any(|issue| issue.to_string().contains("matches no scalar")),
        "{validation:?}"
    );
    Ok(())
}
