use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_record_udf_{}_{}",
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

fn setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Profile"><xs:complexType><xs:sequence><xs:element name="First" type="xs:string"/><xs:element name="Last" type="xs:string"/><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Details"><xs:complexType><xs:sequence><xs:element name="Display" type="xs:string"/><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Profile" outkey="10"><entry name="First" outkey="11"/><entry name="Last" outkey="12"/><entry name="Code" outkey="13"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="FormatProfile" library="user" kind="19"><data>
    <root><entry name="Profile" inpkey="30" componentid="101"/></root>
    <root rootindex="1"><entry name="Details" outkey="31" componentid="102"/></root>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Details" inpkey="20"><entry name="Display" inpkey="21"/><entry name="Code" inpkey="22"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="31"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component>
<component name="FormatProfile" library="user" editable="1"><structure><children>
  <component name="Profile" library="xml" uid="101" kind="14"><properties UsageKind="input"/><data><root><entry name="Profile"><entry name="First" outkey="201"/><entry name="Last" outkey="202"/><entry name="Code" outkey="203"/></entry></root><document schema="source.xsd" instanceroot="{}Source/{}Profile"/><parameter usageKind="input" name="Profile"/></data></component>
  <component name="concat" library="core" uid="103" kind="5"><sources><datapoint key="204"/><datapoint key="205"/></sources><targets><datapoint key="206"/></targets></component>
  <component name="Details" library="xml" uid="102" kind="14"><properties UsageKind="output"/><data><root><entry name="Details"><entry name="Display" inpkey="207"/><entry name="Code" inpkey="208"/></entry></root><document schema="target.xsd" instanceroot="{}Target/{}Details"/><parameter usageKind="output" name="Details"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="201"><edges><edge vertexkey="204"/></edges></vertex>
  <vertex vertexkey="202"><edges><edge vertexkey="205"/></edges></vertex>
  <vertex vertexkey="206"><edges><edge vertexkey="207"/></edges></vertex>
  <vertex vertexkey="203"><edges><edge vertexkey="208"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );
    dir
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.to_owned()))
}

#[test]
fn flat_record_parameter_udf_imports_and_executes() {
    let dir = setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Profile".to_owned(),
        Instance::Group(vec![
            ("First".to_owned(), text("Ada")),
            ("Last".to_owned(), text("Lovelace")),
            ("Code".to_owned(), text("A-1")),
        ]),
    )]);
    let output = engine::run(&imported.project, &source).unwrap();
    let details = output.field("Details").unwrap();

    assert_eq!(
        details.field("Display").and_then(Instance::as_scalar),
        Some(&Value::String("AdaLovelace".to_owned()))
    );
    assert_eq!(
        details.field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("A-1".to_owned()))
    );
}
