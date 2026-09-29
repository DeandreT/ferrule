use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_pipeline_export_{}_{}",
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

#[test]
fn cli_import_then_export_serial_xml_pipeline() {
    let directory = TempDir::new();
    for (file, root, field) in [
        ("source.xsd", "Source", "Start"),
        ("buffer.xsd", "Buffer", "Value"),
        ("target.xsd", "Target", "Result"),
    ] {
        std::fs::write(
            directory.0.join(file),
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
            ),
        )
        .unwrap();
    }
    let source = directory.0.join("source.mfd");
    std::fs::write(
        &source,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    let pipeline = directory.0.join("pipeline.json");
    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--mfd"])
        .arg(&source)
        .arg("--out")
        .arg(&pipeline)
        .arg("--pipeline")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let exported = directory.0.join("exported.mfd");
    for check in [true, false] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .arg("export-mfd")
            .arg("--project")
            .arg(&pipeline)
            .arg("--out")
            .arg(&exported)
            .arg("--pipeline")
            .arg("--profile")
            .arg("native-mfd");
        if check {
            command.arg("--check");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(exported.exists(), !check);
    }
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.pipeline.stages.len(), 2);
}
