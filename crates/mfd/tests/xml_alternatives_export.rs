use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xml_alternatives_export_{}_{}",
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

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn compatible_xml_type_alternatives_export_reimport_and_execute() {
    let imported = mfd::import(&fixture("json-alternatives.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let dir = TempDir::new().unwrap();
    let design = dir.0.join("mapping.mfd");

    let warnings = mfd::export(&imported.project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&design).unwrap();
    assert!(
        exported.contains("instanceroot=\"{urn:ferrule:alternatives}Result\""),
        "{exported}"
    );
    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());

    let original_address = imported
        .project
        .target
        .child("Row")
        .and_then(|row| row.child("Address"))
        .unwrap();
    let roundtrip_address = reimported
        .project
        .target
        .child("Row")
        .and_then(|row| row.child("Address"))
        .unwrap();
    assert_eq!(roundtrip_address, original_address);

    let source = format_json::read(
        &fixture("json-alternatives.json"),
        &reimported.project.source,
    )
    .unwrap();
    let output = engine::run(&reimported.project, &source).unwrap();
    let xml = format_xml::to_string(&reimported.project.target, &output).unwrap();
    assert!(xml.contains("xsi:type=\"ft:Domestic\""), "{xml}");
    assert!(xml.contains("xsi:type=\"ft:International\""), "{xml}");
}
