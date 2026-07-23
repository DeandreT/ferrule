use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xsd_defaults_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn assert_execution(project: &mapping::Project) {
    let source = format_xml::from_str("<Input><Code/></Input>", &project.source).unwrap();
    assert_eq!(
        source.field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("AUTO".to_string()))
    );
    assert_eq!(
        source.field("priority").and_then(Instance::as_scalar),
        Some(&Value::Int(3))
    );
    let target = engine::run(project, &source).unwrap();
    assert_eq!(
        target.field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("AUTO".to_string()))
    );
    assert_eq!(
        target.field("Priority").and_then(Instance::as_scalar),
        Some(&Value::Int(3))
    );
}

#[test]
fn xsd_defaults_remain_executable_across_mfd_export_and_reimport() {
    let directory = TempDir::new();
    write(
        &directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Input"><xs:complexType>
            <xs:sequence>
              <xs:element name="Code" type="xs:string" default="AUTO"/>
            </xs:sequence>
            <xs:attribute name="priority" type="xs:integer" default="3"/>
          </xs:complexType></xs:element>
        </xs:schema>"#,
    );
    write(
        &directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Result"><xs:complexType><xs:sequence>
            <xs:element name="Code" type="xs:string"/>
            <xs:element name="Priority" type="xs:integer"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    );
    write(
        &directory.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Input">
              <entry name="Code" outkey="10"/><entry name="@priority" outkey="11"/>
            </entry></entry></entry>
          </root><document schema="source.xsd" inputinstance="source.xml"
            instanceroot="Input"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
            <entry name="FileInstance"><entry name="document"><entry name="Result">
              <entry name="Code" inpkey="20"/><entry name="Priority" inpkey="21"/>
            </entry></entry></entry>
          </root><document schema="target.xsd" outputinstance="target.xml"
            instanceroot="Result"/></data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported
            .project
            .source
            .child("Code")
            .and_then(|node| node.default.as_deref()),
        Some("AUTO")
    );
    assert_eq!(
        imported
            .project
            .source
            .child("priority")
            .and_then(|node| node.default.as_deref()),
        Some("3")
    );
    assert_execution(&imported.project);

    let exported_path = directory.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtripped = mfd::import(&exported_path).unwrap();
    assert!(
        roundtripped.warnings.is_empty(),
        "{:?}",
        roundtripped.warnings
    );
    assert_eq!(
        roundtripped
            .project
            .source
            .child("Code")
            .and_then(|node| node.default.as_deref()),
        Some("AUTO")
    );
    assert_eq!(
        roundtripped
            .project
            .source
            .child("priority")
            .and_then(|node| node.default.as_deref()),
        Some("3")
    );
    assert_execution(&roundtripped.project);
}
