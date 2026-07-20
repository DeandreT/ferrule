use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_scalar_parity_{}_{}",
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
    <xs:element name="Path" type="xs:string"/>
    <xs:element name="Date" type="xs:dateTime"/>
    <xs:element name="Maybe" type="xs:string" minOccurs="0"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Output"><xs:complexType><xs:sequence>
    <xs:element name="Extension" type="xs:string"/>
    <xs:element name="Weekday" type="xs:int"/>
    <xs:element name="Maybe" type="xs:string" nillable="true"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Input"><entry name="Path" outkey="10"/><entry name="Date" outkey="11"/><entry name="Maybe" outkey="12"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Input"/>
  </data></component>
  <component name="get-fileext" library="core" kind="5"><sources><datapoint pos="0" key="20"/></sources><targets><datapoint pos="0" key="21"/></targets></component>
  <component name="weekday" library="lang" kind="5"><sources><datapoint pos="0" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="substitute-missing-with-xsi-nil" library="core" kind="5"><sources><datapoint pos="0" key="24"/></sources><targets><datapoint pos="0" key="25"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Output"><entry name="Extension" inpkey="30"/><entry name="Weekday" inpkey="31"/><entry name="Maybe" inpkey="32"/></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Output"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="25"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn execute(project: &mapping::Project) -> Result<Instance, Box<dyn std::error::Error>> {
    let source = format_xml::from_str(
        "<Input><Path>folder/archive.tar.gz</Path><Date>2024-02-29T23:00:00-12:00</Date></Input>",
        &project.source,
    )?;
    Ok(engine::run(project, &source)?)
}

fn assert_output(output: &Instance) {
    assert_eq!(
        output.field("Extension").and_then(Instance::as_scalar),
        Some(&Value::String(".gz".into()))
    );
    assert_eq!(
        output.field("Weekday").and_then(Instance::as_scalar),
        Some(&Value::Int(4))
    );
    assert!(
        output
            .field("Maybe")
            .and_then(Instance::as_scalar)
            .is_some_and(Value::is_xml_nil)
    );
}

#[test]
fn remaining_scalar_components_import_execute_and_roundtrip()
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
        ("get-fileext", "core"),
        ("weekday", "lang"),
        ("substitute-missing-with-xsi-nil", "core"),
    ] {
        assert!(exported.contains(&format!("name=\"{name}\" library=\"{library}\"")));
    }

    let roundtrip = mfd::import(&exported_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_output(&execute(&roundtrip.project)?);
    Ok(())
}
