use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_left_right_{}_{}",
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

fn write_design(directory: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        directory.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Text" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Result"><xs:complexType><xs:sequence>
    <xs:element name="Left" type="xs:string"/>
    <xs:element name="Right" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source"><entry name="Text" outkey="10"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="11"/></targets><data><constant value="2" datatype="integer"/></data></component>
  <component name="left" library="lang" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
  <component name="right" library="lang" kind="5"><sources><datapoint pos="0" key="23"/><datapoint pos="1" key="24"/></sources><targets><datapoint pos="0" key="25"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Result"><entry name="Left" inpkey="30"/><entry name="Right" inpkey="31"/></entry></root>
    <document schema="target.xsd" outputinstance="result.xml" instanceroot="{}Result"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="23"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="21"/><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="25"><edges><edge vertexkey="31"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn left_and_right_import_execute_and_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = format_xml::from_str(
        "<Source><Text>A\u{1f642}BC</Text></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Left").and_then(Instance::as_scalar),
        Some(&Value::String("A\u{1f642}".into()))
    );
    assert_eq!(
        output.field("Right").and_then(Instance::as_scalar),
        Some(&Value::String("BC".into()))
    );

    let exported_path = directory.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&exported_path)?;
    assert!(exported.contains("name=\"left\" library=\"lang\""));
    assert!(exported.contains("name=\"right\" library=\"lang\""));

    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(output, engine::run(&roundtrip.project, &source)?);
    Ok(())
}
