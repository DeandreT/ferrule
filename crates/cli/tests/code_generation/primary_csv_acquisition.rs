use super::*;
use mapping::{DocumentProject, PrimarySourcePlan};

pub(super) fn document(literal: bool) -> DocumentProject {
    DocumentProject::new(
        Project {
            source: SchemaNode::group(
                "Input",
                vec![
                    SchemaNode::scalar("Text", ScalarType::String),
                    SchemaNode::scalar("Count", ScalarType::Int),
                ],
            ),
            target: SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: Some("never-preview.csv".into()),
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
                        17,
                        if literal {
                            Node::Const {
                                value: Value::String(" selected *.csv ".into()),
                            }
                        } else {
                            Node::RuntimeParameter {
                                name: "Input".into(),
                                ty: ScalarType::String,
                                preview: Some("never-parameter-preview.csv".into()),
                            }
                        },
                    ),
                    (
                        18,
                        Node::RuntimeParameter {
                            name: "MappingOnly".into(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                ]
                .into(),
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 18,
                }],
                ..Default::default()
            },
        },
        PrimarySourcePlan { filename: 17 },
    )
}

#[test]
fn primary_csv_acquisition_compiles_and_preserves_host_order_and_identity() -> TestResult<()> {
    let directory = TempDir::new("primary_csv_acquisition")?;
    for literal in [false, true] {
        let lowered = codegen::lower_document(&document(literal))?;
        let policy = codegen::CsvInputPolicy {
            preserve_empty_strings: true,
            ..Default::default()
        };
        for language in ["rust", "csharp"] {
            let output = directory.0.join(format!("{literal}-{language}"));
            let rust_options = codegen_rust::Options {
                package_name: "primary_csv_fixture".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../codegen-runtime")
                        .display()
                        .to_string(),
                ),
            };
            let artifacts = if language == "rust" {
                codegen_rust::emit_with_primary_csv_acquisition(&lowered, &rust_options, &policy)
                    .map_err(|e| e.to_string())?
            } else {
                codegen_csharp::emit_with_primary_csv_acquisition(&lowered, &policy)
                    .map_err(|e| e.to_string())?
            };
            for file in artifacts.files() {
                let path = output.join(file.path.as_str());
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, &file.contents)?;
            }
            std::fs::write(
                output.join("first.csv"),
                "Text,Count\n\"Snow\nline\",7\n,8\n",
            )?;
            std::fs::write(output.join("empty.csv"), "Text,Count\n")?;
            std::fs::write(output.join("broken.csv"), "Text,Count\nvalid,2\nbad,nope\n")?;
            let mut wide = vec![0xff, 0xfe];
            for unit in "Text,Count\nWide☃,4\n".encode_utf16() {
                wide.extend_from_slice(&unit.to_le_bytes());
            }
            std::fs::write(output.join("wide.csv"), wide)?;
            let mode = if literal { "literal" } else { "parameter" };
            let mut run = if language == "rust" {
                std::fs::write(
                    output.join("src/main.rs"),
                    include_str!("fixtures/primary_csv_rust.rs.txt"),
                )?;
                let mut command = Command::new("cargo");
                command.args(["run", "--offline", "--quiet", "--", mode]);
                command
            } else {
                std::fs::create_dir(output.join("Harness"))?;
                std::fs::write(
                    output.join("Harness/Program.cs"),
                    include_str!("fixtures/primary_csv_csharp.cs.txt"),
                )?;
                std::fs::write(
                    output.join("Harness/Harness.csproj"),
                    r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
                )?;
                let mut command = dotnet_command(&output);
                command.args([
                    "run",
                    "--project",
                    "Harness/Harness.csproj",
                    "--configuration",
                    "Release",
                    "-p:UseSharedCompilation=false",
                    "--",
                    mode,
                ]);
                command
            };
            let result = host_policy::recorded_output(
                run.current_dir(&output),
                &output,
                "primary-csv-acquisition",
            )?;
            assert!(
                result.status.success(),
                "{language}/{mode}:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                String::from_utf8_lossy(&result.stdout)
                    .contains("primary CSV acquisition cases passed")
            );
        }
    }
    Ok(())
}

#[test]
fn primary_csv_acquisition_keeps_schema_and_dialect_admission() -> TestResult<()> {
    let mut project = document(true).project().clone();
    project.source = SchemaNode::scalar("Invalid", ScalarType::String);
    let document = DocumentProject::new(project, PrimarySourcePlan { filename: 17 });
    let lowered = codegen::lower_document(&document)?;
    assert!(
        codegen::validate_primary_csv_acquisition(lowered.program(), &Default::default()).is_err()
    );
    let lowered = codegen::lower_document(&self::document(true))?;
    let policy = codegen::CsvInputPolicy {
        delimiter: Some('\n'),
        ..Default::default()
    };
    assert!(codegen::validate_primary_csv_acquisition(lowered.program(), &policy).is_err());
    Ok(())
}
