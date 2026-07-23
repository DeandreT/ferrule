use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Node;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_not_exists_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

fn setup() -> Result<(TempDir, PathBuf), Box<dyn Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Missing" type="xs:string" minOccurs="0"/>
    <xs:element name="EmptyText" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="MissingResult" type="xs:boolean"/>
    <xs:element name="EmptyTextResult" type="xs:boolean"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.0.join("not-exists.mfd");
    write(
        &design,
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="source" library="xml" kind="14"><data>
      <root><entry name="Source">
        <entry name="Missing" outkey="10"/>
        <entry name="EmptyText" outkey="11"/>
      </entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="not-exists" library="core" kind="5">
      <sources><datapoint pos="0" key="20"/></sources>
      <targets><datapoint pos="0" key="21"/></targets>
    </component>
    <component name="not-exists" library="core" kind="5">
      <sources><datapoint pos="0" key="22"/></sources>
      <targets><datapoint pos="0" key="23"/></targets>
    </component>
    <component name="target" library="xml" kind="14">
      <properties XSLTDefaultOutput="1"/>
      <data><root><entry name="Target">
        <entry name="MissingResult" inpkey="30"/>
        <entry name="EmptyTextResult" inpkey="31"/>
      </entry></root>
      <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="22"/></edges></vertex>
    <vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="23"><edges><edge vertexkey="31"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    )?;
    Ok((dir, design))
}

fn source() -> Instance {
    Instance::Group(vec![
        ("Missing".to_string(), Instance::Scalar(Value::Null)),
        (
            "EmptyText".to_string(),
            Instance::Scalar(Value::String(String::new())),
        ),
    ])
}

fn assert_result(result: &Instance) {
    assert_eq!(
        result.field("MissingResult").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        result
            .field("EmptyTextResult")
            .and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
}

#[test]
fn generic_not_exists_imports_executes_and_round_trips() -> Result<(), Box<dyn Error>> {
    let (dir, design) = setup()?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let not_calls = imported
        .project
        .graph
        .nodes
        .values()
        .filter(|node| matches!(node, Node::Call { function, .. } if function == "not"))
        .count();
    let exists_calls = imported
        .project
        .graph
        .nodes
        .values()
        .filter(|node| matches!(node, Node::Call { function, .. } if function == "exists"))
        .count();
    assert_eq!((not_calls, exists_calls), (2, 2));

    let input = source();
    assert_result(&engine::run(&imported.project, &input)?);

    let exported = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_result(&engine::run(&reimported.project, &input)?);
    Ok(())
}
