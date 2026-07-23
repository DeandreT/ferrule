use std::error::Error;
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
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Profile"><xs:complexType><xs:sequence><xs:element name="First" type="xs:string"/><xs:element name="Last" type="xs:string"/><xs:element name="Code" type="xs:string"/><xs:element name="UseNil" type="xs:boolean"/><xs:element name="State" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Details"><xs:complexType><xs:sequence><xs:element name="Display" type="xs:string"/><xs:element name="Code" type="xs:string"/><xs:element name="State" type="xs:string" nillable="true"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Profile" outkey="10"><entry name="First" outkey="11"/><entry name="Last" outkey="12"/><entry name="Code" outkey="13"/><entry name="UseNil" outkey="14"/><entry name="State" outkey="15"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="FormatProfile" library="user" kind="19"><data>
    <root><entry name="Profile" inpkey="30" componentid="101"/></root>
    <root rootindex="1"><entry name="Details" outkey="31" componentid="102"/></root>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Details" inpkey="20"><entry name="Display" inpkey="21"/><entry name="Code" inpkey="22"/><entry name="State" inpkey="23"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="31"><edges><edge vertexkey="20"/></edges></vertex>
</vertices></graph></structure></component>
<component name="FormatProfile" library="user" editable="1"><structure><children>
  <component name="Profile" library="xml" uid="101" kind="14"><properties UsageKind="input"/><data><root><entry name="Profile"><entry name="First" outkey="201"/><entry name="Last" outkey="202"/><entry name="Code" outkey="203"/><entry name="UseNil" outkey="209"/><entry name="State" outkey="210"/></entry></root><document schema="source.xsd" instanceroot="{}Source/{}Profile"/><parameter usageKind="input" name="Profile"/></data></component>
  <component name="concat" library="core" uid="103" kind="5"><sources><datapoint key="204"/><datapoint key="205"/></sources><targets><datapoint key="206"/></targets></component>
  <component name="set-xsi-nil" library="core" uid="104" kind="5"><targets><datapoint key="211"/></targets></component>
  <component name="if-else" library="core" uid="105" kind="4"><sources><datapoint pos="0" key="212"/><datapoint pos="1" key="213"/><datapoint pos="2" key="214"/></sources><targets><datapoint key="215"/></targets></component>
  <component name="Details" library="xml" uid="102" kind="14"><properties UsageKind="output"/><data><root><entry name="Details"><entry name="Display" inpkey="207"/><entry name="Code" inpkey="208"/><entry name="State" inpkey="216"/></entry></root><document schema="target.xsd" instanceroot="{}Target/{}Details"/><parameter usageKind="output" name="Details"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="201"><edges><edge vertexkey="204"/></edges></vertex>
  <vertex vertexkey="202"><edges><edge vertexkey="205"/></edges></vertex>
  <vertex vertexkey="206"><edges><edge vertexkey="207"/></edges></vertex>
  <vertex vertexkey="203"><edges><edge vertexkey="208"/></edges></vertex>
  <vertex vertexkey="209"><edges><edge vertexkey="212"/></edges></vertex>
  <vertex vertexkey="210"><edges><edge vertexkey="214"/></edges></vertex>
  <vertex vertexkey="211"><edges><edge vertexkey="213"/></edges></vertex>
  <vertex vertexkey="215"><edges><edge vertexkey="216"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );
    dir
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.to_owned()))
}

fn source(use_nil: bool) -> Instance {
    Instance::Group(vec![(
        "Profile".to_owned(),
        Instance::Group(vec![
            ("First".to_owned(), text("Ada")),
            ("Last".to_owned(), text("Lovelace")),
            ("Code".to_owned(), text("A-1")),
            ("UseNil".to_owned(), Instance::Scalar(Value::Bool(use_nil))),
            ("State".to_owned(), text("active")),
        ]),
    )])
}

fn assert_output(project: &mapping::Project) -> Result<(), Box<dyn Error>> {
    let nil = engine::run(project, &source(true))?;
    let nil_details = nil.field("Details").ok_or("missing Details group")?;
    assert_eq!(
        nil_details.field("Display").and_then(Instance::as_scalar),
        Some(&Value::String("AdaLovelace".to_owned()))
    );
    assert_eq!(
        nil_details.field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("A-1".to_owned()))
    );
    assert!(
        nil_details
            .field("State")
            .and_then(Instance::as_scalar)
            .is_some_and(Value::is_xml_nil)
    );
    let xml = format_xml::to_string(&project.target, &nil)?;
    assert_eq!(xml.matches("xsi:nil=\"true\"").count(), 1);

    let ordinary = engine::run(project, &source(false))?;
    assert_eq!(
        ordinary
            .field("Details")
            .and_then(|details| details.field("State"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("active".to_owned()))
    );
    Ok(())
}

#[test]
fn flat_record_parameter_udf_imports_executes_and_roundtrips_xml_nil() -> Result<(), Box<dyn Error>>
{
    let dir = setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_output(&imported.project)?;

    let exported = dir.0.join("round-trip.mfd");
    let warnings = mfd::export(&imported.project, &exported)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let design = std::fs::read_to_string(&exported)?;
    assert!(design.contains("name=\"set-xsi-nil\" library=\"core\""));
    assert!(!design.contains("name=\"set-xsi-nil\" library=\"ferrule\""));

    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_output(&reimported.project)?;
    Ok(())
}
