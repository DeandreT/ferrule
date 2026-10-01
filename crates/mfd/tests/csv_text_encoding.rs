use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::Project;
use mfd::{ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-encoding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn component(name: &str, source: bool, encoding: Option<&str>) -> String {
    let port = if source { "outkey" } else { "inpkey" };
    let base = if source { 10 } else { 20 };
    let properties = if source {
        ""
    } else {
        "<properties XSLTDefaultOutput=\"1\"/>"
    };
    let encoding = encoding
        .map(|value| format!(" encoding=\"{value}\""))
        .unwrap_or_default();
    format!(
        r#"<component name="{name}" library="text" kind="16">{properties}<data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" {port}="{base}">
<entry name="Text" {port}="{}"/><entry name="Count" {port}="{}"/>
</entry></entry></entry></root><text type="csv"{encoding}>
<settings separator="," firstrownames="false"><names root="{name}" block="Rows">
<field0 name="Text" type="string"/><field1 name="Count" type="integer"/>
</names></settings></text></data></component>"#,
        base + 1,
        base + 2,
    )
}

fn design(source: Option<&str>, target: Option<&str>) -> String {
    let source = component("source", true, source);
    let target = component("target", false, target);
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>{source}{target}
</children><graph><vertices>
<vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
<vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex>
<vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
    )
}

fn execute(project: &Project) -> Result<String, Box<dyn Error>> {
    assert!(engine::validate(project).is_empty());
    let rows = format_csv::from_str("café,7\n", &project.source, None, false)?;
    assert_eq!(
        rows[0].field("Text").and_then(Instance::as_scalar),
        Some(&Value::String("café".into()))
    );
    assert_eq!(
        rows[0].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    let output = engine::run(project, &Instance::Repeated(rows))?;
    Ok(format_csv::to_string(
        &project.target,
        output.as_repeated().ok_or("expected CSV target rows")?,
        None,
        false,
    )?)
}

#[test]
fn utf8_and_absent_encoding_preserve_typed_mapping_through_strict_cycles()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    for (index, (source, target)) in [
        (None, None),
        (Some("1000"), None),
        (None, Some("1000")),
        (Some("1000"), Some("1000")),
    ]
    .into_iter()
    .enumerate()
    {
        let path = temp.0.join(format!("utf8-{index}.mfd"));
        std::fs::write(&path, design(source, target))?;
        let outcome =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
        assert!(outcome.report.executable);
        assert!(outcome.imported.warnings.is_empty());
        let mut project = outcome.imported.project;
        let source_schema = project.source.clone();
        let target_schema = project.target.clone();
        for cycle in 0..2 {
            assert_eq!(execute(&project)?, "café,7\n");
            let exported = temp.0.join(format!("strict-{index}-{cycle}.mfd"));
            let report =
                mfd::export_with_profile(&project, &exported, mfd::ExportProfile::NativeMfd)?;
            assert!(report.is_native_compatible());
            let reimported = mfd::import_with_profile(
                &exported,
                &ImportOptions::default(),
                ImportProfile::Executable,
            )?;
            assert!(reimported.imported.warnings.is_empty());
            project = reimported.imported.project;
            assert_eq!(project.source, source_schema);
            assert_eq!(project.target, target_schema);
        }
        assert_eq!(execute(&project)?, "café,7\n");
    }
    Ok(())
}

#[test]
fn unsupported_source_and_target_encodings_warn_for_repair_and_reject_executable_import()
-> Result<(), Box<dyn Error>> {
    let temp = TempDir::new()?;
    let long_code = "9".repeat(16_384);
    for (index, code) in ["52", "utf-16", "", "unknown", long_code.as_str()]
        .into_iter()
        .enumerate()
    {
        for source in [true, false] {
            let path = temp.0.join(format!("unsupported-{index}-{source}.mfd"));
            std::fs::write(
                &path,
                if source {
                    design(Some(code), None)
                } else {
                    design(None, Some(code))
                },
            )?;
            let repaired = mfd::import_with_profile(
                &path,
                &ImportOptions::default(),
                ImportProfile::BestEffort,
            )?;
            assert!(!repaired.report.executable);
            assert_eq!(repaired.imported.warnings.len(), 1);
            let warning = &repaired.imported.warnings[0];
            assert!(warning.contains(if source {
                "csv component `source`"
            } else {
                "csv component `target`"
            }));
            assert!(warning.contains("unsupported text encoding"));
            assert!(warning.contains("UTF-8 encoding code 1000"));
            assert!(warning.len() < 200);
            assert_eq!(execute(&repaired.imported.project)?, "café,7\n");
            assert!(matches!(
                mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable),
                Err(MfdError::IncompatibleImport(report))
                    if !report.executable && report.issues.iter().any(|issue|
                        issue.kind == ImportIssueKind::ImportWarning && issue.message == *warning)
            ));
        }
    }
    Ok(())
}
