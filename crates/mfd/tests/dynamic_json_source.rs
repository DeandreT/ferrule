use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_dynamic_json_source_{}_{}",
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
        dir.join("source.schema.json"),
        r#"{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"}}}}"#,
    )?;
    std::fs::write(
        dir.join("source.json"),
        r#"[{"id":"A","selected":true},{"id":"B","selected":false},{"id":"C"}]"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Id" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="10"><entry name="id" type="json-property"><entry name="string" outkey="11"/></entry><entry name="property" type="json-property"><entry name="name" type="json-propertyname" outkey="12"/><entry name="boolean" outkey="13"/></entry></entry></entry></entry></entry></entry></entry></root><json schema="source.schema.json" inputinstance="source.json"/></data></component>
  <component name="string" library="core" kind="5"><sources><datapoint pos="0" key="14"/></sources><targets><datapoint pos="0" key="15"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="20"/></targets><data><constant value="selected" datatype="string"/></data></component>
  <component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="21"/><datapoint pos="1" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="logical-and" library="core" kind="5"><sources><datapoint pos="0" key="24"/><datapoint pos="1" key="25"/></sources><targets><datapoint pos="0" key="26"/></targets></component>
  <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="27"/><datapoint pos="1" key="28"/></sources><targets><datapoint pos="0" key="29"/><datapoint/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="30"><entry name="Id" inpkey="31"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="27"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="14"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="15"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="26"><edges><edge vertexkey="28"/></edges></vertex>
  <vertex vertexkey="29"><edges><edge vertexkey="30"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn write_scalar_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("open-source.schema.json"),
        r#"{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"}},"additionalProperties":{"type":"string"}}}"#,
    )?;
    std::fs::write(
        dir.join("open-source.json"),
        r#"[{"id":"A","birthday":"2001-01-02"},{"id":"B","nickname":"Bee"},{"id":"C","birthday":"1998-05-06"}]"#,
    )?;
    std::fs::write(
        dir.join("scalar-target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Id" type="xs:string"/><xs:element name="Birthday" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = dir.join("scalar.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="10"><entry name="id" type="json-property"><entry name="string" outkey="11"/></entry><entry name="property" type="json-property"><entry name="name" type="json-propertyname" outkey="12"/><entry name="string" outkey="13"/></entry></entry></entry></entry></entry></entry></entry></root><json schema="open-source.schema.json" inputinstance="open-source.json"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="20"/></targets><data><constant value="birthday" datatype="string"/></data></component>
  <component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="21"/><datapoint pos="1" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="string" library="core" kind="3"><sources><datapoint pos="0" key="24"/><datapoint pos="1" key="25"/></sources><targets><datapoint pos="0" key="26"/><datapoint/></targets></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="30"><entry name="Id" inpkey="31"/><entry name="Birthday" inpkey="32"/></entry></entry></root><document schema="scalar-target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="26"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn equality_selected_dynamic_boolean_source_fields_filter_objects()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.source.dynamic_fields().is_some());

    let input = format_json::read(&dir.0.join("source.json"), &imported.project.source)?;
    let output = engine::run(&imported.project, &input)?;
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        1,
        "output={output:?}; root={:?}",
        imported.project.root
    );
    assert_eq!(
        rows[0].field("Id").and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );

    let export_path = dir.0.join("dynamic-source-export.mfd");
    let warnings = mfd::export(&imported.project, &export_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let design = std::fs::read_to_string(&export_path)?;
    assert!(design.contains("type=\"json-propertyname\" outkey="));
    assert!(design.contains("name=\"logical-and\""));
    let roundtrip = mfd::import(&export_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(output, engine::run(&roundtrip.project, &input)?);
    Ok(())
}

#[test]
fn equality_selected_dynamic_string_source_fields_map_nullable_scalars()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_scalar_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let input = format_json::read(&dir.0.join("open-source.json"), &imported.project.source)?;
    let output = engine::run(&imported.project, &input)?;
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].field("Birthday").and_then(Instance::as_scalar),
        Some(&Value::String("2001-01-02".into()))
    );
    assert_eq!(
        rows[1].field("Birthday").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
    assert_eq!(
        rows[2].field("Birthday").and_then(Instance::as_scalar),
        Some(&Value::String("1998-05-06".into()))
    );

    let export_path = dir.0.join("dynamic-string-source-export.mfd");
    let warnings = mfd::export(&imported.project, &export_path)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let design = std::fs::read_to_string(&export_path)?;
    assert!(design.contains("name=\"string\" library=\"core\""));
    assert!(design.contains("type=\"json-propertyname\" outkey="));
    let roundtrip = mfd::import(&export_path)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(output, engine::run(&roundtrip.project, &input)?);
    Ok(())
}
