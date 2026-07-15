use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_multiple_targets_{}_{}",
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

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

#[test]
fn every_connected_target_imports_and_executes() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("first.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="First"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("second.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Second"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document schema="source.xsd" instanceroot="{}Source"/></data></component>
  <component name="first" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="First"><entry name="Value" inpkey="20"/></entry></root><document schema="first.xsd" outputinstance="first.xml" instanceroot="{}First"/></data></component>
  <component name="second" library="xml" kind="14"><data><root><entry name="Second"><entry name="Value" inpkey="30"/></entry></root><document schema="second.xsd" outputinstance="second.xml" instanceroot="{}Second"/></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.target.name, "First");
    assert_eq!(imported.project.target_path.as_deref(), Some("first.xml"));
    assert_eq!(imported.project.extra_targets.len(), 1);
    let second = &imported.project.extra_targets[0];
    assert_eq!(second.name, "second");
    assert_eq!(second.schema.name, "Second");
    assert_eq!(second.path.as_deref(), Some("second.xml"));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String("shared".into())),
    )]);
    let outputs = engine::run_outputs(&imported.project, &source)?;
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("shared".into()))
    );
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(outputs.extras[0].name, "second");
    assert_eq!(
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("shared".into()))
    );
    Ok(())
}
