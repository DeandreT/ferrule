use super::*;
use codegen::{Json5BoundaryPolicyError, Json5BoundarySide};
use codegen_schema::json5_profile::{Json5ProfileError, Json5ProfileResource};

fn target(language: &str) -> GenerateTarget {
    if language == "rust" {
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        }
    } else {
        GenerateTarget::CSharp
    }
}

fn expected_artifacts(
    project: &Project,
    selected: GenerateTarget,
    json5: bool,
) -> TestResult<ArtifactFiles> {
    let program = codegen::lower(project)?;
    let artifacts = match selected {
        GenerateTarget::Rust { runtime_path } => {
            let runtime = std::fs::canonicalize(runtime_path)?;
            let options = codegen_rust::Options {
                package_name: "ferrule-generated-mapping".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    runtime.to_str().ok_or("runtime must be UTF-8")?.into(),
                ),
            };
            if json5 {
                codegen_rust::emit_with_json5(&program, &options)?
            } else {
                codegen_rust::emit(&program, &options)?
            }
        }
        GenerateTarget::CSharp => {
            if json5 {
                codegen_csharp::emit_with_json5(&program)?
            } else {
                codegen_csharp::emit(&program)?
            }
        }
    };
    let mut files = artifacts
        .files()
        .iter()
        .map(|file| (file.path.as_str().to_owned(), file.contents.clone()))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

#[test]
fn json5_cli_default_is_exact_and_explicit_selection_is_complete() -> TestResult<()> {
    let root = RetainedDirectory::new("facade_default")?;
    let mut candidate = projects::project("I");
    candidate.source_options.json5 = true;
    candidate.target_options.json5 = true;
    candidate.source_path = Some("input.json5".into());
    candidate.target_path = Some("output.json5".into());
    let project = write_project(&root, "identity", &candidate)?;
    for language in ["rust", "csharp"] {
        let default = root.0.join(format!("{language}-default"));
        let original = generate_project(&project, &default, target(language));
        root.record(&format!("{language}-DEFAULT-ORIGINAL"), &original)?;
        original?;
        let actual = artifact_files(&default)?;
        let expected = expected_artifacts(&candidate, target(language), false)?;
        root.record(
            &format!("{language}-DEFAULT-FULL-COMPARISON"),
            &(&actual, &expected),
        )?;
        assert_eq!(
            actual, expected,
            "ordinary complete artifacts must remain exact"
        );
        let selected = root.0.join(format!("{language}-selected"));
        generate(
            &root,
            &project,
            &selected,
            target(language),
            &format!("{language}-SELECTED"),
        )?;
        let actual = artifact_files(&selected)?;
        let expected = expected_artifacts(&candidate, target(language), true)?;
        root.record(
            &format!("{language}-SELECTED-FULL-COMPARISON"),
            &(&actual, &expected),
        )?;
        assert_eq!(
            actual, expected,
            "the public writer must publish the full optional ArtifactSet"
        );
    }
    Ok(())
}

#[test]
fn json5_cli_refuses_options_and_schema_before_lowering_without_publication() -> TestResult<()> {
    let root = RetainedDirectory::new("facade_admission")?;
    for language in ["rust", "csharp"] {
        for (index, side) in [Json5BoundarySide::Source, Json5BoundarySide::Target]
            .into_iter()
            .enumerate()
        {
            for defect in ["option", "schema", "wide"] {
                let mut candidate = projects::project("I");
                // A lowerer failure must not displace the earlier typed admission cause.
                candidate.root.bindings[0].node = 999;
                match (side, defect) {
                    (Json5BoundarySide::Source, "option") => {
                        candidate.source_options.json_lines = true
                    }
                    (Json5BoundarySide::Target, "option") => {
                        candidate.target_options.json_lines = true
                    }
                    (Json5BoundarySide::Source, "schema") => candidate.source.repeating = true,
                    (Json5BoundarySide::Target, "schema") => candidate.target.repeating = true,
                    (side, "wide") => {
                        let schema = SchemaNode::group(
                            "TooWide",
                            (0..4096)
                                .map(|index| {
                                    SchemaNode::scalar(format!("field{index}"), ScalarType::Int)
                                })
                                .collect(),
                        );
                        if side == Json5BoundarySide::Source {
                            candidate.source = schema;
                        } else {
                            candidate.target = schema;
                        }
                    }
                    _ => unreachable!("finite admission matrix"),
                }
                let label = format!("{language}-side{index}-{defect}");
                let project = write_project(&root, &label, &candidate)?;
                for existing in [false, true] {
                    let output = root.0.join(format!("{label}-existing{existing}"));
                    if existing {
                        std::fs::create_dir(&output)?;
                        std::fs::write(
                            output.join("sentinel.bin"),
                            b"\0original destination\xff\n",
                        )?;
                    }
                    let before = if existing {
                        Some(artifact_files(&output)?)
                    } else {
                        None
                    };
                    let original =
                        generate_project_with_json5_adapters(&project, &output, target(language));
                    root.record(&format!("{label}-existing{existing}-ORIGINAL"), &original)?;
                    let after = if existing {
                        Some(artifact_files(&output)?)
                    } else {
                        None
                    };
                    root.record(
                        &format!("{label}-existing{existing}-DESTINATION-ORIGINALS"),
                        &(&before, &after, output.exists()),
                    )?;
                    let error = original.expect_err("unsupported selection must fail");
                    let cause = error
                        .downcast_ref::<Json5BoundaryPolicyError>()
                        .expect("shared typed admission cause must remain owned");
                    match (defect, cause) {
                        (
                            "option",
                            Json5BoundaryPolicyError::FormatOption {
                                side: actual,
                                field: "json_lines",
                            },
                        ) => assert_eq!(*actual, side),
                        (
                            "schema",
                            Json5BoundaryPolicyError::Schema {
                                side: actual,
                                error: Json5ProfileError::UnsupportedMetadata { field: "repeating" },
                            },
                        ) => assert_eq!(*actual, side),
                        (
                            "wide",
                            Json5BoundaryPolicyError::Schema {
                                side: actual,
                                error:
                                    Json5ProfileError::Limit {
                                        resource: Json5ProfileResource::SchemaNodes,
                                        requested: 4097,
                                        max: 4096,
                                    },
                            },
                        ) => assert_eq!(*actual, side),
                        _ => panic!("wrong complete original policy payload: {cause:#?}"),
                    }
                    if existing {
                        assert_eq!(
                            before, after,
                            "existing sentinel tree must remain byte-exact"
                        );
                    } else {
                        assert!(
                            !output.exists(),
                            "unsupported fresh destination must remain absent"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn json5_cli_conflict_usage_and_no_flag_default_remain_exact() -> TestResult<()> {
    let root = RetainedDirectory::new("cli_flags")?;
    let project = write_project(&root, "identity", &projects::project("I"))?;
    let mut help = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    help.args(["generate", "--help"]);
    let help = root.command("HELP", &mut help)?;
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--json5-adapters"));
    for language in ["rust", "csharp"] {
        let conflict = root.0.join(format!("{language}-conflict"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .arg("generate")
            .arg("--project")
            .arg(root.0.join("missing-project.json"))
            .arg("--language")
            .arg(language)
            .arg("--out")
            .arg(&conflict)
            .args(["--csv-output", "--json5-adapters"]);
        let result = root.command(&format!("{language}-CONFLICT"), &mut command)?;
        assert_eq!(result.status.code(), Some(2));
        assert!(!conflict.exists());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("--csv-output") && stderr.contains("--json5-adapters"));
        for json5 in [false, true] {
            let output = root.0.join(format!("{language}-json5{json5}"));
            let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
            command
                .arg("generate")
                .arg("--project")
                .arg(&project)
                .arg("--language")
                .arg(language)
                .arg("--out")
                .arg(&output);
            if language == "rust" {
                command
                    .arg("--rust-runtime-path")
                    .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"));
            }
            if json5 {
                command.arg("--json5-adapters");
            }
            let result = root.command(&format!("{language}-CLI-JSON5{json5}"), &mut command)?;
            assert!(result.status.success(), "actual CLI result retained");
            let actual = artifact_files(&output)?;
            let expected = expected_artifacts(&projects::project("I"), target(language), json5)?;
            root.record(
                &format!("{language}-CLI-JSON5{json5}-COMPLETE-ARTIFACTS"),
                &(&actual, &expected),
            )?;
            assert_eq!(actual, expected);
        }
    }
    Ok(())
}

#[test]
fn json5_cli_program_profile_and_supported_existing_destination_guards() -> TestResult<()> {
    let root = RetainedDirectory::new("program_and_existing")?;
    let mut unsupported = projects::project("I");
    unsupported.extra_targets.push(mapping::NamedTarget {
        name: "other".into(),
        path: None,
        schema: SchemaNode::group("Other", Vec::new()),
        options: Default::default(),
        root: Scope::default(),
    });
    let unsupported_project = write_project(&root, "unsupported", &unsupported)?;
    let supported_project = write_project(&root, "supported", &projects::project("I"))?;
    for language in ["rust", "csharp"] {
        // The ordinary path remains able to generate this named-target project.
        let ordinary = root.0.join(format!("{language}-ordinary-named"));
        let original = generate_project(&unsupported_project, &ordinary, target(language));
        root.record(&format!("{language}-ORDINARY-NAMED-ORIGINAL"), &original)?;
        original?;
        for existing in [false, true] {
            let output = root
                .0
                .join(format!("{language}-unsupported-existing{existing}"));
            if existing {
                std::fs::create_dir(&output)?;
                std::fs::write(output.join("sentinel.bin"), b"original\0\xff")?;
            }
            let before = if existing {
                Some(artifact_files(&output)?)
            } else {
                None
            };
            let original = generate_project_with_json5_adapters(
                &unsupported_project,
                &output,
                target(language),
            );
            root.record(
                &format!("{language}-UNSUPPORTED-existing{existing}-ORIGINAL"),
                &original,
            )?;
            let after = if existing {
                Some(artifact_files(&output)?)
            } else {
                None
            };
            root.record(
                &format!("{language}-UNSUPPORTED-existing{existing}-DESTINATION"),
                &(&before, &after, output.exists()),
            )?;
            let error = original.expect_err("named targets are outside the companion profile");
            let policy = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<Json5BoundaryPolicyError>())
                .expect("complete optional emitter policy chain must remain typed");
            assert!(matches!(
                policy,
                Json5BoundaryPolicyError::ProgramField {
                    field: "extra_targets"
                }
            ));
            if existing {
                assert_eq!(before, after);
            } else {
                assert!(!output.exists());
            }
        }
        let output = root.0.join(format!("{language}-supported-existing"));
        std::fs::create_dir(&output)?;
        std::fs::write(output.join("sentinel.bin"), b"original destination\0\xff")?;
        let before = artifact_files(&output)?;
        let original =
            generate_project_with_json5_adapters(&supported_project, &output, target(language));
        let after = artifact_files(&output)?;
        root.record(
            &format!("{language}-SUPPORTED-EXISTING-ORIGINAL"),
            &(&original, &before, &after),
        )?;
        assert!(original.is_err());
        assert_eq!(before, after);
    }
    Ok(())
}
