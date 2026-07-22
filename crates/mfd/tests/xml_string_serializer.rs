use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_string_serializer_{}_{}",
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

fn input() -> Instance {
    Instance::Group(vec![(
        "Person".into(),
        Instance::Repeated(vec![Instance::Group(vec![
            ("active".into(), Instance::Scalar(Value::Bool(true))),
            (
                "Name".into(),
                Instance::Scalar(Value::String("A & B".into())),
            ),
            (
                "Tag".into(),
                Instance::Repeated(vec![
                    Instance::Scalar(Value::String("x".into())),
                    Instance::Scalar(Value::String("y".into())),
                ]),
            ),
        ])]),
    )])
}

fn payload(output: &Instance) -> &str {
    let value = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .and_then(|rows| rows.first())
        .and_then(|row| row.field("Payload"))
        .and_then(Instance::as_scalar)
        .unwrap();
    let Value::String(value) = value else {
        panic!("expected a string payload, got {value:?}");
    };
    value
}

#[test]
fn imports_executes_and_exports_structured_xml_string_serializer() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:company" xmlns:c="urn:company" elementFormDefault="qualified">
  <xs:element name="Person"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Tag" type="xs:string" maxOccurs="unbounded"/></xs:sequence><xs:attribute name="active" type="xs:boolean"/></xs:complexType></xs:element>
  <xs:element name="Company"><xs:complexType><xs:sequence><xs:element ref="c:Person" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Payload" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Company"><entry name="Person" outkey="10"><entry name="active"/><entry name="Name"/><entry name="Tag"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{urn:company}Company"/></data></component>
  <component name="serialize" library="xml" kind="14"><properties XSLTTargetEncoding="UTF-8" WriteXMLDeclaration="0"/><data><root><entry name="FileInstance" outkey="20"><entry name="document"><entry name="Person" inpkey="21"><entry name="active"/><entry name="Name"/><entry name="Tag"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{urn:company}Person"/><parameter usageKind="stringserialize"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Row" inpkey="30"><entry name="Payload" inpkey="31"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="21" type="2"/><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let output = engine::run(&imported.project, &input()).unwrap();
    assert_eq!(
        payload(&output),
        "<Person xmlns=\"urn:company\" active=\"true\">\n  <Name>A &amp; B</Name>\n  <Tag>x</Tag>\n  <Tag>y</Tag>\n</Person>"
    );

    let exported_path = dir.0.join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    assert!(dir.0.join("roundtrip-serializer-0.xsd").is_file());

    let roundtrip = mfd::import(&exported_path).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    let roundtrip_output = engine::run(&roundtrip.project, &input()).unwrap();
    assert_eq!(payload(&roundtrip_output), payload(&output));
}
