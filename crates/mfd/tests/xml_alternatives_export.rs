use std::path::{Path, PathBuf};

use test_support::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn compatible_xml_type_alternatives_export_reimport_and_execute() {
    let imported = mfd::import(&fixture("json-alternatives.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let dir = TempDir::new("mfd_xml_alternatives_export").unwrap();
    let design = dir.path().join("mapping.mfd");

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
