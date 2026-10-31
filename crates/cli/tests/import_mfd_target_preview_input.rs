//! An XML target whose design-time input instance is a wildcard imports as an
//! ordinary runnable target and survives a native export.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;

const DESIGN: &str = r#"<mapping version="22"><component name="defaultmap" uid="1"><properties SelectedLanguage="builtin"/><structure><children>
<component name="Source" library="xml" uid="2" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Label" outkey="1"/></entry></entry></entry></root><document schema="source.xsd" instanceroot="{}Envelope" inputinstance="input.xml"/></data></component>
<component name="Target" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1" AddSchemaLocation="0"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Envelope"><entry name="Label" inpkey="2"/></entry></entry></entry></root><document schema="target.xsd" instanceroot="{}Envelope" inputinstance="prior*.xml" outputinstance="output.xml"/></data></component>
</children><graph directed="1"><edges/><vertices><vertex vertexkey="1"><edges><edge vertexkey="2"/></edges></vertex></vertices></graph></structure></component></mapping>"#;

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-target-preview-input-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ferrule(directory: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn label(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "Envelope");
    root.children()
        .find(|node| node.has_tag_name("Label"))
        .and_then(|node| node.text())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn wildcard_target_preview_input_imports_runs_and_reexports() {
    let directory = Dir::new();
    let dir = &directory.0;
    std::fs::write(dir.join("source.xsd"), SCHEMA).unwrap();
    std::fs::write(dir.join("target.xsd"), SCHEMA).unwrap();
    std::fs::write(dir.join("mapping.mfd"), DESIGN).unwrap();
    std::fs::write(
        dir.join("input.xml"),
        "<Envelope><Label>x</Label></Envelope>",
    )
    .unwrap();

    let imported = ferrule(
        dir,
        &[
            "import-mfd",
            "--mfd",
            "mapping.mfd",
            "--out",
            "project.json",
            "--require-executable",
        ],
    );
    assert_eq!(imported.trim(), "wrote project.json (0 warning(s))");
    ferrule(dir, &["run", "project.json", "input.xml", "output.xml"]);
    assert_eq!(label(&dir.join("output.xml")), "x");

    ferrule(
        dir,
        &[
            "export-mfd",
            "--project",
            "project.json",
            "--out",
            "export/roundtrip.mfd",
            "--profile",
            "native-mfd",
        ],
    );
    ferrule(
        dir,
        &[
            "import-mfd",
            "--mfd",
            "export/roundtrip.mfd",
            "--out",
            "roundtrip.json",
            "--require-executable",
        ],
    );
    ferrule(
        dir,
        &["run", "roundtrip.json", "input.xml", "roundtrip-output.xml"],
    );
    assert_eq!(label(&dir.join("roundtrip-output.xml")), "x");
}
