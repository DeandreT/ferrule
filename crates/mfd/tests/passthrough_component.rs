use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_passthrough_{}_{}",
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

#[test]
fn xml_passthrough_components_are_intermediates_and_independent_targets() {
    let dir = TempDir::new();
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), schema);
    write(
        &dir.0.join("target.xsd"),
        &schema.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Source"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="source.xsd" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Value" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.extra_sources.is_empty());
    assert_eq!(imported.project.target.name, "Target");
    let [passthrough] = imported.project.extra_targets.as_slice() else {
        panic!("expected the pass-through component as an additional target");
    };
    assert_eq!(passthrough.name, "buffer");
    assert_eq!(passthrough.schema.name, "Source");
    assert_eq!(passthrough.path.as_deref(), Some("buffer.xml"));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Value".to_string(),
        Instance::Scalar(Value::String("passed through".to_string())),
    )]);
    let outputs = engine::run_outputs(&imported.project, &source).unwrap();
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("passed through".to_string()))
    );
    let [passthrough] = outputs.extras.as_slice() else {
        panic!("expected one pass-through output");
    };
    assert_eq!(passthrough.name, "buffer");
    assert_eq!(
        passthrough
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("passed through".to_string()))
    );

    let roundtrip_path = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&roundtrip_path).unwrap();
    assert_eq!(exported.matches("XSLTDefaultOutput=\"1\"").count(), 1);

    let roundtrip = mfd::import(&roundtrip_path).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.target.name, "Target");
    let [passthrough] = roundtrip.project.extra_targets.as_slice() else {
        panic!("expected one pass-through output after re-import");
    };
    assert_eq!(passthrough.name, "buffer");
    assert_eq!(passthrough.path.as_deref(), Some("buffer.xml"));
    assert!(engine::validate(&roundtrip.project).is_empty());

    let outputs = engine::run_outputs(&roundtrip.project, &source).unwrap();
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar)
    );
}
