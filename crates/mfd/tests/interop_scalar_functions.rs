use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_interop_scalar_functions_{}_{}",
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
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Date" type="xs:string"/>
    <xs:element name="Number" type="xs:decimal"/>
    <xs:element name="Text" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Formatted" type="xs:string"/>
    <xs:element name="Boolean" type="xs:boolean"/>
    <xs:element name="Positive" type="xs:decimal"/>
    <xs:element name="Floor" type="xs:decimal"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Date" outkey="10"/><entry name="Number" outkey="11"/><entry name="Text" outkey="12"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Input"/>
  </data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="13"/></targets><data><constant value="[D01] [MNn,3-3] [Y] [Z]" datatype="string"/></data></component>
  <component name="format-dateTime" library="xpath2" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
  <component name="boolean" library="xpath2" kind="5"><sources><datapoint pos="0" key="23"/></sources><targets><datapoint pos="0" key="24"/></targets></component>
  <component name="positive" library="core" kind="5"><sources><datapoint pos="0" key="25"/></sources><targets><datapoint pos="0" key="26"/></targets></component>
  <component name="floor" library="xpath2" kind="5"><sources><datapoint pos="0" key="27"/></sources><targets><datapoint pos="0" key="28"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Formatted" inpkey="30"/><entry name="Boolean" inpkey="31"/><entry name="Positive" inpkey="32"/><entry name="Floor" inpkey="33"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="23"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="25"/><edge vertexkey="27"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="24"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="26"><edges><edge vertexkey="32"/></edges></vertex>
  <vertex vertexkey="28"><edges><edge vertexkey="33"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn execute(project: &mapping::Project) -> Result<Instance, Box<dyn std::error::Error>> {
    let source = format_xml::from_str(
        "<Input><Date>2010-12-01T15:02:39+01:00</Date><Number>-2.1</Number><Text>false</Text></Input>",
        &project.source,
    )?;
    Ok(engine::run(project, &source)?)
}

fn assert_output(output: &Instance) {
    for (field, expected) in [
        ("Formatted", Value::String("01 Dec 2010 +01:00".to_string())),
        ("Boolean", Value::Bool(true)),
        ("Positive", Value::Float(-2.1)),
        ("Floor", Value::Float(-3.0)),
    ] {
        assert_eq!(
            output.field(field).and_then(Instance::as_scalar),
            Some(&expected),
            "{field}"
        );
    }
}

#[test]
fn interoperable_scalar_components_import_execute_and_roundtrip_warning_free()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_design(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_output(&execute(&imported.project)?);

    let exported_path = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported_path)?.is_empty());
    let exported = std::fs::read_to_string(&exported_path)?;
    for (name, library) in [
        ("format-dateTime", "xpath2"),
        ("boolean", "xpath2"),
        ("positive", "core"),
        ("floor", "xpath2"),
    ] {
        assert!(exported.contains(&format!("name=\"{name}\" library=\"{library}\"")));
    }

    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_output(&execute(&roundtrip.project)?);
    Ok(())
}
