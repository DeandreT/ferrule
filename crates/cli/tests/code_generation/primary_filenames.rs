use super::*;
use mapping::{DocumentProject, PrimarySourcePlan};

pub(super) fn document(literal: bool) -> DocumentProject {
    DocumentProject::new(
        Project {
            source: SchemaNode::group("Input", vec![]),
            target: SchemaNode::group("Output", vec![string("Value")]),
            source_path: Some("ignored-preview.csv".into()),
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [
                    (
                        1,
                        Node::RuntimeParameter {
                            name: "Output".into(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                    (
                        17,
                        if literal {
                            Node::Const {
                                value: Value::String(" ./input-雪-\"*.csv ".into()),
                            }
                        } else {
                            Node::RuntimeParameter {
                                name: "Input".into(),
                                ty: ScalarType::String,
                                preview: Some("ignored-parameter-preview.csv".into()),
                            }
                        },
                    ),
                ]
                .into(),
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Default::default()
            },
        },
        PrimarySourcePlan::new(17),
    )
}

#[test]
fn lowering_keeps_filename_separate_and_refuses_unsupported_plans() -> TestResult<()> {
    for literal in [false, true] {
        let mut document = document(literal);
        document.prune_unreachable_nodes();
        let program = codegen::lower_document(&document)?;
        assert_eq!(program.program(), &codegen::lower(document.project())?);
        if literal {
            assert_eq!(
                program.primary_filename(),
                &codegen::PrimaryFilenamePlan::Literal {
                    node: 17,
                    text: " ./input-雪-\"*.csv ".into(),
                }
            );
        } else {
            assert_eq!(
                program.primary_filename(),
                &codegen::PrimaryFilenamePlan::Parameter {
                    node: 17,
                    name: "Input".into(),
                }
            );
        }
    }
    let original = document(false);
    let mut source = original.project().clone();
    source.graph.nodes.insert(17, Node::SourceDocumentPath);
    assert!(matches!(
        codegen::lower_document(&DocumentProject::new(source, PrimarySourcePlan::new(17))),
        Err(codegen::DocumentLowerError::Filename(
            mapping::PrimaryFilenameError::UnsupportedExpression { node: 17 }
        ))
    ));
    let mut source = original.project().clone();
    source.root.bindings[0].node = 999;
    assert!(matches!(
        codegen::lower_document(&DocumentProject::new(source, PrimarySourcePlan::new(17))),
        Err(codegen::DocumentLowerError::Mapping(_))
    ));
    Ok(())
}

#[test]
fn generated_primary_filenames_execute_without_any_source_instance() -> TestResult<()> {
    let directory = TempDir::new("primary_filenames")?;
    for literal in [false, true] {
        let document = document(literal);
        let program = codegen::lower_document(&document)?;
        for language in ["rust", "csharp"] {
            let output = directory.0.join(format!("{literal}-{language}"));
            let rust_options = codegen_rust::Options {
                package_name: "primary_filename_fixture".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../codegen-runtime")
                        .display()
                        .to_string(),
                ),
            };
            let emit = || {
                if language == "rust" {
                    codegen_rust::emit_with_primary_filename(&program, &rust_options)
                        .map_err(|e| e.to_string())
                } else {
                    codegen_csharp::emit_with_primary_filename(&program).map_err(|e| e.to_string())
                }
            };
            let artifacts = emit()?;
            assert_eq!(artifacts, emit()?, "deterministic filename companion");
            for file in artifacts.files() {
                let destination = output.join(file.path.as_str());
                std::fs::create_dir_all(destination.parent().unwrap())?;
                std::fs::write(destination, &file.contents)?;
            }
            let mut run = if language == "rust" {
                std::fs::write(
                    output.join("src/main.rs"),
                    include_str!("fixtures/primary_filename_rust.rs.txt"),
                )?;
                let mut run = Command::new("cargo");
                run.args([
                    "run",
                    "--offline",
                    "--quiet",
                    "--",
                    if literal { "literal" } else { "parameter" },
                ]);
                run
            } else {
                std::fs::create_dir(output.join("Harness"))?;
                std::fs::write(
                    output.join("Harness/Program.cs"),
                    include_str!("fixtures/primary_filename_csharp.cs.txt"),
                )?;
                std::fs::write(
                    output.join("Harness/Harness.csproj"),
                    r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
                )?;
                let mut run = dotnet_command(&output);
                run.args([
                    "run",
                    "--project",
                    "Harness/Harness.csproj",
                    "--configuration",
                    "Release",
                    "-p:UseSharedCompilation=false",
                    "--",
                    if literal { "literal" } else { "parameter" },
                ]);
                run
            };
            let result = host_policy::recorded_output(
                run.current_dir(&output),
                &output,
                "primary-filename",
            )?;
            assert!(
                result.status.success(),
                "{language}/{literal}:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                String::from_utf8_lossy(&result.stdout).contains("primary filename cases passed")
            );
        }
    }
    Ok(())
}
