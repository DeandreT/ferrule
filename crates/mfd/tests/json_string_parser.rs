use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_string_parser_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

#[test]
fn imports_and_executes_a_connected_json_string_parser() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Payload" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("payload.schema.json"),
        r#"{"type":"object","properties":{"Shares":{"type":"integer"},"Leaves":{"type":"object","properties":{"Total":{"type":"number"}}}}}"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Report"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Shares" type="xs:integer"/><xs:element name="Total" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Payload" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="payload" library="json" kind="31"><data><root><entry name="FileInstance" inpkey="20"><entry name="document"><entry name="root"><entry name="object"><entry name="Shares" type="json-property"><entry name="number" outkey="21"/></entry><entry name="Leaves" type="json-property"><entry name="object"><entry name="Total" type="json-property"><entry name="number" outkey="22"/></entry></entry></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="payload.schema.json" inputinstance="missing-design-time.json"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Report"><entry name="Row" inpkey="30"><entry name="Shares" inpkey="31"/><entry name="Total" inpkey="32"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source_path.as_deref(), Some("source.xml"));
    assert!(imported.project.extra_sources.is_empty());
    assert!(engine::validate(&imported.project).is_empty());

    let input = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "Payload".into(),
            Instance::Scalar(Value::String(
                r#"{"Shares":7,"Leaves":{"Total":3.5}}"#.into(),
            )),
        )])]),
    )]);
    let output = engine::run(&imported.project, &input).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].field("Shares").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        rows[0].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(3.5))
    );
}
