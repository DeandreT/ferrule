use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_multi_case_if_else_{}_{}",
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

fn source(value: f64) -> Instance {
    Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::Float(value)),
    )])
}

fn label(project: &mapping::Project, value: f64) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(engine::run(project, &source(value))?
        .field("Label")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or("missing label")?)
}

#[test]
fn growable_if_else_evaluates_condition_value_pairs_before_default()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    std::fs::write(
        dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let mapping = dir.0.join("mapping.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="1"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="greater" library="core" kind="5"><sources><datapoint pos="0" key="10"/><datapoint pos="1" key="11"/></sources><targets><datapoint pos="0" key="12"/></targets></component>
  <component name="less" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="30"/></targets><data><constant value="20" datatype="decimal"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="31"/></targets><data><constant value="5" datatype="decimal"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="32"/></targets><data><constant value="high" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="33"/></targets><data><constant value="low" datatype="string"/></data></component>
  <component name="if-else" library="core" kind="4"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/><datapoint pos="2" key="42"/><datapoint pos="3" key="43"/><datapoint pos="4"/></sources><targets><datapoint pos="0" key="44"/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Label" inpkey="50"/></entry></root><document schema="target.xsd" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="10"/><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="30"><edges><edge vertexkey="11"/></edges></vertex>
  <vertex vertexkey="31"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="40"/></edges></vertex>
  <vertex vertexkey="32"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="33"><edges><edge vertexkey="43"/></edges></vertex>
  <vertex vertexkey="44"><edges><edge vertexkey="50"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&mapping)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        label(&imported.project, 25.0)?,
        Value::String("high".into())
    );
    assert_eq!(label(&imported.project, 2.0)?, Value::String("low".into()));
    assert_eq!(label(&imported.project, 10.0)?, Value::Null);

    let exported_path = dir.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported_path)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let exported = std::fs::read_to_string(&exported_path)?;
    assert!(
        exported.contains("component name=\"set-empty\" library=\"core\""),
        "{exported}"
    );
    let reimported = mfd::import(&exported_path)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(
        label(&reimported.project, 25.0)?,
        Value::String("high".into())
    );
    assert_eq!(
        label(&reimported.project, 2.0)?,
        Value::String("low".into())
    );
    assert_eq!(label(&reimported.project, 10.0)?, Value::Null);
    Ok(())
}
