use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_schema_import_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("schemas")).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_design(dir: &TempDir, source_reference: &str) -> PathBuf {
    let design = include_str!("fixtures/people.mfd")
        .replace("people-source.xsd", source_reference)
        .replace("people-target.xsd", "target.xsd");
    let path = dir.0.join("mapping.mfd");
    std::fs::write(&path, design).unwrap();
    std::fs::write(
        dir.0.join("target.xsd"),
        include_str!("fixtures/people-target.xsd"),
    )
    .unwrap();
    path
}

#[test]
fn imports_a_uniquely_case_mismatched_schema_in_the_referenced_directory() {
    let dir = TempDir::new();
    let design = write_design(&dir, "schemas\\people-source.xsd");
    std::fs::write(
        dir.0.join("schemas/People-Source.xsd"),
        include_str!("fixtures/people-source.xsd"),
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.source.child("Staff").unwrap().repeating);
}

#[test]
fn ambiguous_and_missing_case_fallbacks_keep_the_existing_import_warning() {
    let ambiguous_dir = TempDir::new();
    let ambiguous_design = write_design(&ambiguous_dir, "schemas/input.xsd");
    std::fs::write(
        ambiguous_dir.0.join("schemas/Input.xsd"),
        include_str!("fixtures/people-source.xsd"),
    )
    .unwrap();
    std::fs::write(
        ambiguous_dir.0.join("schemas/INPUT.XSD"),
        include_str!("fixtures/people-source.xsd"),
    )
    .unwrap();

    let ambiguous = mfd::import(&ambiguous_design).unwrap();
    assert!(ambiguous.warnings.iter().any(|warning| {
        warning.contains("component `Company`: could not read schema `schemas/input.xsd`")
            && warning.contains("multiple case-insensitive sibling matches")
            && warning.contains("falling back to the entry tree")
    }));

    let missing_dir = TempDir::new();
    let missing_design = write_design(&missing_dir, "schemas/missing.xsd");
    let missing = mfd::import(&missing_design).unwrap();
    assert!(missing.warnings.iter().any(|warning| {
        warning.contains("component `Company`: could not read schema `schemas/missing.xsd`")
            && warning.contains("was not found")
            && warning.contains("falling back to the entry tree")
    }));
}
