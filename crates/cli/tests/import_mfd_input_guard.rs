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

#[test]
fn competing_singular_entries_refuse_native_and_generated_public_routes()
-> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixtures = root.join("../mfd/tests/fixtures");
    let runtime = root.join("../codegen-runtime").canonicalize()?;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "ferrule_singular_feed_routes_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory)?;
    let _cleanup = TempDir(directory.clone());

    for (name, fixture, design, same_value, location) in [
        (
            "distinct",
            "input_guard",
            "competing-singular.mfd",
            false,
            "root scope",
        ),
        (
            "identical",
            "input_guard",
            "competing-singular.mfd",
            true,
            "root scope",
        ),
        (
            "nested",
            "nested_clone_owners",
            "aliased_cell_values.mfd",
            false,
            "scope `Group/<segment 1>/Row/<segment 2>/Cell/<segment 1>`",
        ),
    ] {
        let case = directory.join(name);
        std::fs::create_dir(&case)?;
        let source = fixtures.join(fixture);
        for schema in [
            "source.xsd",
            "target.xsd",
            "source-deep.xsd",
            "target-deep.xsd",
        ] {
            if source.join(schema).exists() {
                std::fs::copy(source.join(schema), case.join(schema))?;
            }
        }
        let mut design_text = std::fs::read_to_string(source.join(design))?;
        if same_value {
            assert!(design_text.contains("value=\"violet\""));
            design_text = design_text.replace("value=\"violet\"", "value=\"amber\"");
        }
        let mapping_file = case.join("mapping.mfd");
        std::fs::write(&mapping_file, design_text)?;
        let imported = mfd::import(&mapping_file)?;
        assert!(
            imported.warnings.is_empty(),
            "{name}: {:?}",
            imported.warnings
        );
        let message = "target field `Value` is bound more than once";
        let issues = engine::validate(&imported.project);
        assert_eq!(issues.len(), 1, "{name}: {issues:?}");
        assert_eq!(issues[0].location, location);
        assert_eq!(issues[0].message, message);
        let expected = vec![codegen::Diagnostic::Validation {
            location: location.to_owned(),
            message: message.to_owned(),
        }];
        assert_eq!(
            codegen::lower(&imported.project).unwrap_err().diagnostics(),
            expected
        );
        let project_file = case.join("project.json");
        std::fs::write(
            &project_file,
            mapping::project_file::encode_pretty(&imported.project)?,
        )?;
        let native_output = case.join("native/result.xml");
        let native_error = cli::run_project(
            &project_file,
            &case.join("missing-input.xml"),
            &native_output,
        )
        .expect_err("validation must precede input acquisition");
        assert_eq!(
            native_error.to_string(),
            format!("project validation failed with 1 issue(s):\n  - {location}: {message}")
        );
        assert!(!native_output.parent().unwrap().exists());

        for (language, target) in [
            (
                "rust",
                cli::GenerateTarget::Rust {
                    runtime_path: runtime.clone(),
                },
            ),
            ("csharp", cli::GenerateTarget::CSharp),
        ] {
            for existing in [false, true] {
                let output = case.join(format!("{language}-{existing}/generated"));
                let marker = output.join("keep.txt");
                if existing {
                    std::fs::create_dir_all(&output)?;
                    std::fs::write(&marker, b"original destination\n")?;
                }
                let error = cli::generate_project(&project_file, &output, target.clone())
                    .expect_err("invalid project cannot generate artifacts");
                assert_eq!(
                    error.to_string(),
                    format!(
                        "mapping cannot be lowered for code generation (1 diagnostic):\n  - {location}: {message}"
                    )
                );

                let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
                command
                    .args([
                        "--diagnostics",
                        "json",
                        "generate",
                        "--language",
                        language,
                        "--project",
                    ])
                    .arg(&project_file)
                    .arg("--out")
                    .arg(&output);
                if language == "rust" {
                    command.arg("--rust-runtime-path").arg(&runtime);
                }
                let result = command.output()?;
                assert_eq!(
                    result.status.code(),
                    Some(1),
                    "{name}/{language}: {result:?}"
                );
                assert!(result.stdout.is_empty());
                let diagnostics: Vec<serde_json::Value> = std::str::from_utf8(&result.stderr)?
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()?;
                assert!(
                    diagnostics
                        .iter()
                        .all(|d| d["command"] == "generate" && d["severity"] == "error")
                );
                assert!(
                    diagnostics
                        .iter()
                        .any(|d| d["message"].as_str().is_some_and(|m| m.contains(message)))
                );
                if existing {
                    assert_eq!(std::fs::read(&marker)?, b"original destination\n");
                    assert_eq!(std::fs::read_dir(&output)?.count(), 1);
                } else {
                    assert!(!output.parent().unwrap().exists());
                }
            }
        }
    }
    Ok(())
}
