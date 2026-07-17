use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_edi_structural_{}_{}",
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
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="templateId"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:string"><xs:attribute name="extension" type="xs:string"/></xs:extension>
    </xs:simpleContent></xs:complexType></xs:element>
    <xs:element name="telecomA"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:string"><xs:attribute name="value" type="xs:string"/></xs:extension>
    </xs:simpleContent></xs:complexType></xs:element>
    <xs:element name="telecomB"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:string"><xs:attribute name="value" type="xs:string"/></xs:extension>
    </xs:simpleContent></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><resources/><component name="map"><structure><children>
  <component name="hl7" library="text" kind="16"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Message">
      <entry name="PID" outkey="10">
        <entry name="PID-3" outkey="11"/>
        <entry name="PID-13" outkey="12"><entry name="XTN-1" outkey="13"/></entry>
      </entry>
    </entry></entry></entry></root>
    <text type="edi" kind="EDIHL7" inputinstance="input.hl7"/>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Target">
      <entry name="templateId" inpkey="20"><entry name="extension" type="attribute" inpkey="21"/></entry>
      <entry name="telecomA" inpkey="30"><entry name="value" type="attribute" inpkey="31"/></entry>
      <entry name="telecomB" inpkey="40"><entry name="value" type="attribute" inpkey="41"/></entry>
    </entry></entry></entry></root>
    <document schema="target.xsd" outputinstance="output.xml" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="30"/><edge vertexkey="40"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="31"/><edge vertexkey="41"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn edi_groups_feed_simple_content_target_context_without_atomization() -> Result<(), Box<dyn Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("entry-tree schema inferred"));
    assert!(engine::validate(&imported.project).is_empty());

    assert!(!imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::SourceField { path, .. }
            if path == &["PID".to_string()] || path == &["PID".to_string(), "PID-13".to_string()])
    }));
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::SourceField { path, .. }
            if path.last().is_some_and(|field| field == "PID-3"))
    }));
    assert!(imported.project.graph.nodes.values().any(|node| {
        matches!(node, Node::SourceField { path, .. }
            if path.last().is_some_and(|field| field == "XTN-1"))
    }));
    Ok(())
}
