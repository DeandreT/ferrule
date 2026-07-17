use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XML_TEXT_FIELD};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_simple_content_atomization_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn write_fixture(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Token" maxOccurs="unbounded"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:string"><xs:attribute name="kind" type="xs:string"/></xs:extension>
    </xs:simpleContent></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Entry" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Value" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source"><entry name="Token" outkey="10"><entry name="kind" type="attribute" outkey="11"/></entry></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="upper-case" library="xpath2" kind="5">
    <sources><datapoint pos="0" key="30"/></sources>
    <targets><datapoint pos="0" key="31"/></targets>
  </component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Target"><entry name="Entry" inpkey="20"><entry name="Value" inpkey="21"/></entry></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="31"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn repeating_simple_content_atomizes_when_consumed_as_a_scalar() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::SourceField { frame: Some(frame), path }
            if frame == &["Token".to_string()]
                && path == &[XML_TEXT_FIELD.to_string()])
    }));

    let source = format_xml::from_str(
        "<Source><Token kind=\"word\">alpha</Token><Token kind=\"word\">beta</Token></Source>",
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    let entries = target
        .field("Entry")
        .and_then(Instance::as_repeated)
        .ok_or("target entries are not repeated")?;
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("ALPHA".into()))
    );
    assert_eq!(
        entries[1].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("BETA".into()))
    );
    Ok(())
}
