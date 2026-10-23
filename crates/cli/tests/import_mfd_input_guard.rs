use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use mfd::{ImportIssue, ImportIssueKind, ImportProfile, ImportReport, MfdError};

/// Removes a per-test directory on drop, including after a panic.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn retain(directory: &Path, name: &str, value: impl std::fmt::Debug) -> std::io::Result<()> {
    std::fs::write(directory.join(name), format!("{value:#?}\n"))
}

#[test]
fn conflicting_physical_inputs_refuse_before_destination_publication() -> Result<(), Box<dyn Error>>
{
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/input_guard");
    for design in ["modern-80.mfd", "legacy-80.mfd", "mixed-reintroduce.mfd"] {
        let directory = std::env::temp_dir().join(format!(
            "ferrule_cli_input_guard335_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory)?;
        let _cleanup = TempDir(directory.clone());
        eprintln!("CLI_INPUT_GUARD335_ORIGINALS={}", directory.display());
        for name in ["source.xsd", "target.xsd"] {
            std::fs::copy(fixtures.join(name), directory.join(name))?;
        }
        std::fs::copy(fixtures.join(design), directory.join("design.mfd"))?;
        let expected = ImportReport {
            executable: false,
            issues: vec![ImportIssue {
                kind: ImportIssueKind::ImportWarning,
                message: "input port 20 has conflicting source feeds [80, 81]; all feeds to this input were skipped".into(),
            }],
        };
        retain(&directory, "expected-before-import.txt", &expected)?;
        for existing in [false, true] {
            let destination = directory.join(if existing {
                "existing.json"
            } else {
                "absent/project.json"
            });
            let sentinel = b"existing project\n";
            let modified = if existing {
                std::fs::write(&destination, sentinel)?;
                Some(std::fs::metadata(&destination)?.modified()?)
            } else {
                None
            };
            let api = cli::import_mfd_with_profile(
                &directory.join("design.mfd"),
                &destination,
                Some(&directory),
                None,
                &[],
                &[],
                ImportProfile::Executable,
            );
            retain(&directory, &format!("api-{existing}.original.txt"), &api)?;
            let error = api.expect_err("executable admission must refuse");
            let Some(MfdError::IncompatibleImport(actual)) = error.downcast_ref::<MfdError>()
            else {
                return Err(format!("unexpected typed error: {error:#?}").into());
            };
            assert_eq!(**actual, expected);
            let arguments = vec![
                "--diagnostics".into(),
                "json".into(),
                "import-mfd".into(),
                "--mfd".into(),
                "design.mfd".into(),
                "--out".into(),
                destination.as_os_str().to_owned(),
                "--require-executable".into(),
            ];
            retain(
                &directory,
                &format!("binary-{existing}.argv.txt"),
                &arguments,
            )?;
            let binary = Command::new(env!("CARGO_BIN_EXE_ferrule"))
                .current_dir(&directory)
                .args(&arguments)
                .output();
            retain(
                &directory,
                &format!("binary-{existing}.original.txt"),
                &binary,
            )?;
            let binary = binary?;
            std::fs::write(
                directory.join(format!("binary-{existing}.stdout.bin")),
                &binary.stdout,
            )?;
            std::fs::write(
                directory.join(format!("binary-{existing}.stderr.bin")),
                &binary.stderr,
            )?;
            assert_eq!(binary.status.code(), Some(1));
            assert!(binary.stdout.is_empty());
            let diagnostics = std::str::from_utf8(&binary.stderr)?
                .lines()
                .map(serde_json::from_str::<serde_json::Value>)
                .collect::<Result<Vec<_>, _>>()?;
            assert!(!diagnostics.is_empty());
            assert!(
                diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic["command"] == "import-mfd"
                        && diagnostic["severity"] == "error")
            );
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(&expected.issues[0].message))
            }));
            if existing {
                assert_eq!(std::fs::read(&destination)?, sentinel);
                assert_eq!(
                    std::fs::metadata(&destination)?.modified()?,
                    modified.unwrap()
                );
            } else {
                assert!(!destination.parent().unwrap().exists());
            }
        }
        let repaired = directory.join("repair.json");
        let result = cli::import_mfd_with_profile(
            &directory.join("design.mfd"),
            &repaired,
            Some(&directory),
            None,
            &[],
            &[],
            ImportProfile::BestEffort,
        );
        retain(&directory, "repair.original.txt", &result)?;
        assert_eq!(result?, vec![expected.issues[0].message.clone()]);
        let decoded = mapping::project_file::decode_bytes(&std::fs::read(&repaired)?);
        retain(&directory, "repair-project.original.txt", &decoded)?;
        let project = decoded?;
        let source = format_xml::from_str("<Seed><Token>kept</Token></Seed>", &project.source)?;
        let expected_value = ir::Instance::Group(
            vec![(
                "Safe".to_owned(),
                ir::Instance::Scalar(ir::Value::String("kept".to_owned())),
            )]
            .into(),
        );
        retain(&directory, "repair-value.expected.txt", &expected_value)?;
        let value = engine::run(&project, &source);
        retain(&directory, "repair-value.original.txt", &value)?;
        assert_eq!(value?, expected_value);
    }
    Ok(())
}
