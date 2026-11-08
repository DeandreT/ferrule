use std::path::PathBuf;

use test_support::TempDir;

fn write_design(dir: &TempDir, source_reference: &str) -> PathBuf {
    std::fs::create_dir_all(dir.join("schemas")).unwrap();
    let design = include_str!("fixtures/people.mfd")
        .replace("people-source.xsd", source_reference)
        .replace("people-target.xsd", "target.xsd");
    let path = dir.path().join("mapping.mfd");
    std::fs::write(&path, design).unwrap();
    std::fs::write(
        dir.path().join("target.xsd"),
        include_str!("fixtures/people-target.xsd"),
    )
    .unwrap();
    path
}

#[test]
fn imports_a_uniquely_case_mismatched_schema_in_the_referenced_directory() {
    let dir = TempDir::new("mfd_schema_import").unwrap();
    let design = write_design(&dir, "schemas\\people-source.xsd");
    std::fs::write(
        dir.path().join("schemas/People-Source.xsd"),
        include_str!("fixtures/people-source.xsd"),
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.source.child("Staff").unwrap().repeating);
}

#[test]
fn case_fallback_respects_distinct_physical_siblings() {
    let ambiguous_dir = TempDir::new("mfd_schema_import").unwrap();
    let ambiguous_design = write_design(&ambiguous_dir, "schemas/input.xsd");
    std::fs::write(
        ambiguous_dir.path().join("schemas/Input.xsd"),
        include_str!("fixtures/people-source.xsd"),
    )
    .unwrap();
    std::fs::write(
        ambiguous_dir.path().join("schemas/INPUT.XSD"),
        include_str!("fixtures/people-source.xsd"),
    )
    .unwrap();

    let physical_matches = std::fs::read_dir(ambiguous_dir.path().join("schemas"))
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("input.xsd"))
        })
        .count();
    assert!(matches!(physical_matches, 1 | 2));
    let imported = mfd::import(&ambiguous_design).unwrap();
    if physical_matches == 2 {
        assert!(imported.warnings.iter().any(|warning| {
            warning.contains("component `Company`: could not read schema `schemas/input.xsd`")
                && warning.contains("multiple case-insensitive sibling matches")
                && warning.contains("falling back to the entry tree")
        }));
    } else {
        // Case-insensitive filesystems retain one file, not an ambiguous pair.
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(imported.project.source.child("Staff").unwrap().repeating);
    }
}

#[test]
fn missing_case_fallback_keeps_the_existing_import_warning() {
    let missing_dir = TempDir::new("mfd_schema_import").unwrap();
    let missing_design = write_design(&missing_dir, "schemas/missing.xsd");
    let missing = mfd::import(&missing_design).unwrap();
    assert!(missing.warnings.iter().any(|warning| {
        warning.contains("component `Company`: could not read schema `schemas/missing.xsd`")
            && warning.contains("was not found")
            && warning.contains("falling back to the entry tree")
    }));
}
