use std::path::PathBuf;

use ir::Instance;
use mapping::ScopeConstruction;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_repeating_scalar_import_{}",
            std::process::id()
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

#[test]
fn direct_repeating_scalar_wire_imports_as_scalar_iteration() {
    let directory = TempDir::new();
    std::fs::write(
        directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let design = directory.0.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Value" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Value" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let value_scope = &imported.project.root.children[0];
    assert_eq!(value_scope.target_field, "Value");
    assert!(matches!(
        value_scope.construction,
        ScopeConstruction::Scalar { .. }
    ));
    let source = format_xml::from_str(
        "<Source><Value>first</Value><Value>second</Value></Source>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let values = output
        .field("Value")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(
        values[0].as_scalar(),
        Some(&ir::Value::String("first".into()))
    );
    assert_eq!(
        values[1].as_scalar(),
        Some(&ir::Value::String("second".into()))
    );

    let exported_design = directory.0.join("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &exported_design)
            .unwrap()
            .is_empty()
    );
    let roundtrip = mfd::import(&exported_design).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(output, engine::run(&roundtrip.project, &source).unwrap());
}
