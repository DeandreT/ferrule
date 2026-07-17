use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XML_TEXT_FIELD};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_explicit_text_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Code" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Value" minOccurs="0"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r##"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source"><entry name="Item" outkey="10"><entry name="Code" outkey="11"/></entry></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Target"><entry name="Value" inpkey="20"><entry name="#text" inpkey="21"/></entry></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
</vertices></graph></structure></component></mapping>"##,
    )?;
    Ok(design)
}

#[test]
fn explicit_text_entry_promotes_an_untyped_xsd_element() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        imported
            .project
            .target
            .child("Value")
            .and_then(|value| value.child(XML_TEXT_FIELD))
            .is_some_and(|text| text.text)
    );

    let source = format_xml::from_str(
        "<Source><Item><Code>A</Code></Item><Item><Code>B</Code></Item></Source>",
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    let values = target
        .field("Value")
        .and_then(Instance::as_mapped_sequence)
        .ok_or("target values are not a mapped sequence")?;
    assert_eq!(values.len(), 2);
    assert_eq!(
        values[0]
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );
    assert_eq!(
        values[1]
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
    Ok(())
}
