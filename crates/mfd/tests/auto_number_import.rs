use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_auto_number_{}_{}",
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
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType/></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Number" type="xs:integer"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source"><entry name="Row" outkey="10"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="constant" library="core" kind="2">
    <targets><datapoint pos="0" key="11"/></targets>
    <data><constant value="5" datatype="integer"/></data>
  </component>
  <component name="constant" library="core" kind="2">
    <targets><datapoint pos="0" key="12"/></targets>
    <data><constant value="2" datatype="integer"/></data>
  </component>
  <component name="auto-number" library="core" kind="5">
    <sources><datapoint/><datapoint pos="1" key="20"/><datapoint pos="2" key="21"/><datapoint/></sources>
    <targets><datapoint pos="0" key="22"/></targets>
  </component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Target"><entry name="Row" inpkey="30"><entry name="Number" inpkey="31"/></entry></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="31"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn auto_number_uses_current_position_start_and_increment() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let input = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![
            Instance::Group(Vec::new()),
            Instance::Group(Vec::new()),
        ]),
    )]);
    let output = engine::run(&imported.project, &input)?;
    let numbers = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .into_iter()
        .flatten()
        .filter_map(|row| row.field("Number").and_then(Instance::as_scalar))
        .collect::<Vec<_>>();
    assert_eq!(numbers, [&Value::Int(5), &Value::Int(7)]);
    Ok(())
}
