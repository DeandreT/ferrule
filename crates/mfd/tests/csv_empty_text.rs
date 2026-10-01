use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mfd::{ExportProfile, ImportOptions, ImportProfile, MfdError};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_csv_empty_text_{}_{}",
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

fn design(policy: &str, second_type: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="text" kind="16"><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" outkey="10"><entry name="A" outkey="11"/><entry name="B" outkey="12"/></entry></entry></entry></root>
<text type="csv" inputinstance="input.csv"><settings separator="," quote="&quot;" firstrownames="false" {policy}><names root="Row" block="Rows"><field0 name="A" type="string"/><field1 name="B" type="{second_type}"/></names></settings></text></data></component>
<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
<root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="20"><entry name="A" inpkey="21"/><entry name="B" inpkey="22"/></entry></entry></entry></root>
<text type="csv" outputinstance="output.csv"><settings separator="," quote="&quot;" firstrownames="false" removeempty="true"><names root="Row" block="Rows"><field0 name="A" type="string"/><field1 name="B" type="{second_type}"/></names></settings></text></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex></vertices></graph></structure></component></mapping>"#
    )
}

#[test]
fn native_empty_text_policy_survives_two_strict_cycles() -> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    for (case, policy, preserve) in [
        ("present", "removeempty=\"false\"", true),
        ("absent", "removeempty=\"true\"", false),
        ("one", "removeempty=\"1\"", false),
        ("zero", "removeempty=\"0\"", true),
        ("legacy", "", false),
    ] {
        let path = temp.0.join(format!("{case}.mfd"));
        std::fs::write(&path, design(policy, "string"))?;
        let imported =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
        assert!(imported.report.executable);
        let mut project = imported.imported.project;
        for cycle in 0..3 {
            assert_eq!(project.source_options.csv_preserve_empty_strings, preserve);
            project = mapping::project_file::decode_str(&mapping::project_file::encode_pretty(
                &project,
            )?)?;
            let rows = format_csv::from_str_with_options(
                ",beta\n\"\",gamma\ndelta\n",
                &project.source,
                &format_csv::CsvReadOptions::from(&project.source_options),
            )?;
            let result = engine::run(&project, &Instance::Repeated(rows))?;
            let rows = result.as_repeated().ok_or("expected rows")?;
            for row in &rows[..2] {
                assert_eq!(
                    row.field("A").and_then(Instance::as_scalar),
                    Some(&if preserve {
                        Value::String(String::new())
                    } else {
                        Value::Null
                    })
                );
            }
            assert_eq!(
                rows[2].field("B").and_then(Instance::as_scalar),
                Some(&Value::Null)
            );
            if cycle < 2 {
                let path = temp.0.join(format!("{case}-cycle-{cycle}.mfd"));
                let report = mfd::export_with_profile(&project, &path, ExportProfile::NativeMfd)?;
                assert!(report.is_native_compatible());
                let encoded = std::fs::read_to_string(&path)?;
                assert!(encoded.contains(&format!("removeempty=\"{}\"", !preserve)));
                let next = mfd::import_with_profile(
                    &path,
                    &ImportOptions::default(),
                    ImportProfile::Executable,
                )?;
                project = next.imported.project;
            }
        }
    }
    Ok(())
}

#[test]
fn invalid_or_unverified_typed_empty_policy_rejects_executable_import_and_faithful_export()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    for (name, policy, ty, message) in [
        (
            "invalid",
            "removeempty=\"nope\"",
            "string",
            "invalid removeempty",
        ),
        (
            "typed",
            "removeempty=\"false\"",
            "integer",
            "non-text columns",
        ),
    ] {
        let path = temp.0.join(format!("{name}.mfd"));
        std::fs::write(&path, design(policy, ty))?;
        let imported = mfd::import(&path)?;
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains(message))
        );
        assert!(matches!(
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable),
            Err(MfdError::IncompatibleImport(_))
        ));
        if name == "typed" {
            for profile in [ExportProfile::NativeMfd, ExportProfile::FerruleExtensions] {
                let path = temp.0.join(format!("blocked-{profile:?}/mapping.mfd"));
                assert!(mfd::export_with_profile(&imported.project, &path, profile).is_err());
                assert!(!path.parent().unwrap().exists());
            }
        }
    }
    let path = temp.0.join("bounded.mfd");
    std::fs::write(
        &path,
        design(&format!("removeempty=\"{}\"", "x".repeat(16384)), "string"),
    )?;
    let imported = mfd::import(&path)?;
    assert_eq!(imported.warnings.len(), 1);
    assert!(imported.warnings[0].len() < 250);
    Ok(())
}
