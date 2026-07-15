use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::{AggregateOp, Node};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_variable_aggregate_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Catalog"><xs:complexType><xs:sequence>
    <xs:element name="Entry" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Given" type="xs:string"/>
      <xs:element name="Family" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("entry.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Entry"><xs:complexType><xs:sequence>
    <xs:element name="Given" type="xs:string"/>
    <xs:element name="Family" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("summary.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Summary"><xs:complexType><xs:sequence>
    <xs:element name="Names" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("catalog.xml"),
        "<Catalog><Entry><Given>Ada</Given><Family>Lovelace</Family></Entry><Entry><Given>Grace</Given><Family>Hopper</Family></Entry></Catalog>",
    )?;

    let design = dir.join("variable-aggregate.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="catalog" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Catalog"><entry name="Entry" outkey="1"><entry name="Given" outkey="2"/><entry name="Family" outkey="3"/></entry></entry></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  <component name="entry-variable" library="xml" kind="14"><data><parameter usageKind="variable"/><root><entry name="document"><entry name="Entry" inpkey="10"><entry name="Given" outkey="11"/><entry name="Family" outkey="12"/></entry></entry></root><document schema="entry.xsd" instanceroot="{}Entry"/></data></component>
  <component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/><datapoint pos="2" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="27"/></targets><data><constant value=" " datatype="string"/></data></component>
  <component name="string-join" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="24"/><datapoint pos="2" key="25"/></sources><targets><datapoint pos="0" key="26"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="28"/></targets><data><constant value=" | " datatype="string"/></data></component>
  <component name="summary" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Summary"><entry name="Names" inpkey="30"/></entry></entry></entry></root><document schema="summary.xsd" instanceroot="{}Summary"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="27"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="28"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="26"><edges><edge vertexkey="30"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn computed_aggregate_resolves_transparent_variable_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let (collection, expression) = imported
        .project
        .graph
        .nodes
        .values()
        .find_map(|node| match node {
            Node::Aggregate {
                function: AggregateOp::Join,
                collection,
                expression: Some(expression),
                ..
            } => Some((collection, expression)),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("computed string-join was not imported"))?;
    assert_eq!(collection, &["Entry"]);
    assert!(matches!(
        imported.project.graph.nodes.get(expression),
        Some(Node::Call { function, .. }) if function == "concat"
    ));

    let source = format_xml::read(&dir.0.join("catalog.xml"), &imported.project.source)?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Names").and_then(Instance::as_scalar),
        Some(&Value::String("Ada Lovelace | Grace Hopper".to_string()))
    );
    Ok(())
}
