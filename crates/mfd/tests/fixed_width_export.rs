use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{FixedFieldWidth, FixedWidthLayout};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_fixed_width_{tag}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn width(value: u32) -> FixedFieldWidth {
    FixedFieldWidth::new(value).unwrap()
}

#[test]
fn exports_and_reimports_fixed_width_source() {
    let project = mfd::import(&fixture("fixed-width.mfd")).unwrap().project;
    let dir = TempDir::new("source");
    let design = dir.0.join("mapping.mfd");

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("<text type=\"flf\" inputinstance=\"fixed-width.txt\""));
    assert!(text.contains("delimiter=\"true\" fillchar=\"_\" removeempty=\"true\""));
    assert!(text.contains("name=\"Code\" type=\"integer\" length=\"3\""));
    assert!(text.contains("name=\"Name\" type=\"string\" length=\"6\""));
    assert!(text.contains("name=\"Active\" type=\"boolean\" length=\"5\""));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(
        reimported.project.source_options.fixed_width,
        project.source_options.fixed_width
    );
}

#[test]
fn exports_and_reimports_fixed_width_target() {
    let mut project = mfd::import(&fixture("people-to-csv.mfd")).unwrap().project;
    project.target_path = Some("people.txt".into());
    project.target_options.delimiter = None;
    project.target_options.has_header_row = None;
    project.target_options.fixed_width =
        Some(FixedWidthLayout::new(vec![width(24), width(4)], ' ', false, false).unwrap());
    let dir = TempDir::new("target");
    let design = dir.0.join("mapping.mfd");

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains("<text type=\"flf\" outputinstance=\"people.txt\""));
    assert!(text.contains("delimiter=\"false\" fillchar=\" \" removeempty=\"false\""));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.target, project.target);
    assert_eq!(
        reimported.project.target_options.fixed_width,
        project.target_options.fixed_width
    );
}

#[test]
fn invalid_fixed_width_exports_preserve_existing_design() {
    let base = mfd::import(&fixture("fixed-width.mfd")).unwrap().project;
    let dir = TempDir::new("atomic");

    let cases = [
        (
            "width-count",
            {
                let mut project = base.clone();
                project.source_options.fixed_width =
                    Some(FixedWidthLayout::new(vec![width(3), width(6)], ' ', true, true).unwrap());
                project
            },
            "width(s)",
        ),
        (
            "schema",
            {
                let mut project = base.clone();
                project.source = SchemaNode::group(
                    "Nested",
                    vec![SchemaNode::group(
                        "Record",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )],
                );
                project
            },
            "not a flat group",
        ),
        (
            "csv-options",
            {
                let mut project = base;
                project.source_options.delimiter = Some(',');
                project
            },
            "conflicts with CSV",
        ),
    ];

    for (name, project, expected) in cases {
        let design = dir.0.join(format!("{name}.mfd"));
        std::fs::write(&design, "existing design").unwrap();
        assert!(matches!(
            mfd::export(&project, &design),
            Err(mfd::MfdError::Unsupported(message)) if message.contains(expected)
        ));
        assert_eq!(std::fs::read_to_string(&design).unwrap(), "existing design");
    }
}
