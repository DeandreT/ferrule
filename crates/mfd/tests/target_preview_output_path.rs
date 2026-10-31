//! A target's input instance is a design-time preview for every format: a
//! wildcard there never becomes the output path of a non-XML target.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const OUTPUT: &str = r#"outputinstance="people-out.csv""#;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-target-preview-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        for name in ["people-source.xsd", "people.xml"] {
            std::fs::copy(fixture(name), path.join(name)).unwrap();
        }
        Self(path)
    }
    /// The CSV target's design with `document` in place of its output instance.
    fn design(&self, name: &str, document: &str) -> PathBuf {
        let design = std::fs::read_to_string(fixture("people-to-csv.mfd")).unwrap();
        assert!(design.contains(OUTPUT));
        let path = self.0.join(name);
        std::fs::write(&path, design.replace(OUTPUT, document)).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn executable(path: &Path) -> mapping::Project {
    let outcome = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(
        outcome.imported.warnings.is_empty(),
        "{:?}",
        outcome.imported.warnings
    );
    outcome.imported.project
}

fn json(project: &mapping::Project) -> serde_json::Value {
    serde_json::to_value(project).unwrap()
}

fn rows(directory: &Directory, project: &mapping::Project) -> ir::Instance {
    let text = std::fs::read_to_string(directory.0.join("people.xml")).unwrap();
    let source = format_xml::from_str(&text, &project.source).unwrap();
    engine::run(project, &source).unwrap()
}

#[test]
fn a_target_preview_input_does_not_change_the_imported_project() {
    let directory = Directory::new();
    let without = executable(&directory.design("none.mfd", OUTPUT));
    assert_eq!(without.target_path.as_deref(), Some("people-out.csv"));
    for preview in [
        "prior.csv",
        "prior*.csv",
        "prior?.csv",
        "https://example.invalid/prior.csv",
        "prior*/prior.csv",
    ] {
        let project = executable(&directory.design(
            "preview.mfd",
            &format!(r#"inputinstance="{preview}" {OUTPUT}"#),
        ));
        assert_eq!(json(&project), json(&without), "{preview}");
    }
}

#[test]
fn a_wildcard_preview_names_no_absent_output() {
    let directory = Directory::new();
    let plain = mfd::import(&directory.design("plain.mfd", r#"inputinstance="prior.csv""#))
        .unwrap()
        .project;
    assert_eq!(plain.target_path.as_deref(), Some("prior.csv"));
    let none = mfd::import(&directory.design("none.mfd", ""))
        .unwrap()
        .project;
    for preview in ["prior*.csv", "data/prior?.csv"] {
        let wildcard =
            mfd::import(&directory.design("set.mfd", &format!(r#"inputinstance="{preview}""#)))
                .unwrap()
                .project;
        assert_eq!(wildcard.target_path, None, "{preview}");
        assert_eq!(json(&wildcard), json(&none), "{preview}");
        assert!(engine::validate(&wildcard).is_empty());
    }
}

#[test]
fn a_wildcard_preview_exports_and_reimports_like_a_plain_preview() {
    let directory = Directory::new();
    let mut exported = Vec::new();
    for (name, preview) in [("plain", "prior.csv"), ("set", "prior*.csv")] {
        let project = executable(&directory.design(
            &format!("{name}.mfd"),
            &format!(r#"inputinstance="{preview}" {OUTPUT}"#),
        ));
        let out = directory.0.join(format!("export-{name}/roundtrip.mfd"));
        let report =
            mfd::export_with_profile(&project, &out, mfd::ExportProfile::NativeMfd).unwrap();
        assert!(report.is_native_compatible(), "{report}");
        std::fs::copy(
            directory.0.join("people.xml"),
            directory.0.join(format!("export-{name}/people.xml")),
        )
        .unwrap();
        let reimported = executable(&out);
        assert_eq!(reimported.target_options, project.target_options);
        assert_eq!(reimported.target_path.as_deref(), Some("people-out.csv"));
        assert_eq!(rows(&directory, &reimported), rows(&directory, &project));
        exported.push(std::fs::read_to_string(&out).unwrap());
    }
    assert_eq!(exported[0], exported[1]);
}
