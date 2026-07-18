use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-connected-file-paths-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn constant_input_parameters_become_static_source_and_target_paths() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    std::fs::write(
        directory.0.join("document.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Document"><xs:complexType><xs:sequence>
            <xs:element name="Value" type="xs:string"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )?;
    std::fs::write(
        directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="constant" library="core" kind="2"><targets><datapoint key="1"/></targets><data><constant value="input.xml" datatype="string"/></data></component>
          <component name="input-parameter" library="core" kind="6"><sources><datapoint key="2"/></sources><targets><datapoint key="3"/></targets><data><input datatype="string"/><parameter usageKind="input" name="InputFile"/></data></component>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance" inpkey="4"><entry name="document"><entry name="Document"><entry name="Value" outkey="5"/></entry></entry></entry>
          </root><document schema="document.xsd" instanceroot="{}Document"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="6"/></targets><data><constant value="output.xml" datatype="string"/></data></component>
          <component name="output-parameter" library="core" kind="6"><sources><datapoint key="7"/></sources><targets><datapoint key="8"/></targets><data><input datatype="string"/><parameter usageKind="input" name="OutputFile"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance" inpkey="9"><entry name="document"><entry name="Document"><entry name="Value" inpkey="10"/></entry></entry></entry>
          </root><document schema="document.xsd" instanceroot="{}Document"/></data></component>
        </children><graph><vertices>
          <vertex vertexkey="1"><edges><edge vertexkey="2"/></edges></vertex>
          <vertex vertexkey="3"><edges><edge vertexkey="4"/></edges></vertex>
          <vertex vertexkey="6"><edges><edge vertexkey="7"/></edges></vertex>
          <vertex vertexkey="8"><edges><edge vertexkey="9"/></edges></vertex>
          <vertex vertexkey="5"><edges><edge vertexkey="10"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&directory.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source_path.as_deref(), Some("input.xml"));
    assert_eq!(imported.project.target_path.as_deref(), Some("output.xml"));
    assert!(imported.project.root.output_path().is_none());
    assert!(engine::validate(&imported.project).is_empty());

    let exported = directory.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &exported)?.is_empty());
    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.source_path.as_deref(), Some("input.xml"));
    assert_eq!(roundtrip.project.target_path.as_deref(), Some("output.xml"));
    Ok(())
}
