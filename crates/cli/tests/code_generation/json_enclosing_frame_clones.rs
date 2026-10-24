use super::*;

#[path = "fixtures/json_enclosing_frame_clones_project.rs.txt"]
mod oracle;

// Expected Projects, values and text are authored separately from the importer,
// interpreter and generated hosts. Every host receives the same fixed catalogue.
#[test]
fn ordinary_json_implicit_occurrences_match_generated_rust_and_csharp() -> TestResult<()> {
    let directory = TempDir::new("json_implicit_occurrences")?;
    let catalogue: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/json_enclosing_frame_clones_cases.json"
    ))?;
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/json-enclosing-frame-clones");
    for label in ["implicit", "reversed", "wrapped", "literals"] {
        let fixture = fixture_root.join(format!("{label}.mfd"));
        let imported = mfd::import(&fixture)?;
        assert!(
            imported.warnings.is_empty(),
            "{label}: {:?}",
            imported.warnings
        );
        let expected_project = oracle::expected_project(label == "literals");
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project)?,
            mapping::project_file::encode_pretty(&expected_project)?,
            "{label}: complete imported Project",
        );
        let executable = mfd::import_with_profile(
            &fixture,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )?;
        assert_eq!(
            mapping::project_file::encode_pretty(&executable.imported.project)?,
            mapping::project_file::encode_pretty(&expected_project)?,
            "{label}: complete Executable Project",
        );
        assert!(engine::validate(&imported.project).is_empty(), "{label}");
        let cases = catalogue[label].as_array().unwrap();
        for case in cases {
            if let Some(input) = case.get("input").and_then(serde_json::Value::as_str) {
                let source = format_json::from_str(input, &imported.project.source);
                if case["reject"] == true {
                    assert!(source.is_err(), "{label}: source boundary accepted {case}");
                    continue;
                }
                let source = source?;
                let saved_source = source.clone();
                let output = engine::run(&imported.project, &source)?;
                assert_eq!(
                    source, saved_source,
                    "{label}: caller source changed: {case}"
                );
                assert_eq!(
                    output,
                    oracle::expected_instance(&case["expected"]),
                    "{label}: complete typed ordered value: {case}"
                );
                let text = format_json::to_string(&imported.project.target, &output)?;
                assert_eq!(
                    text.as_bytes(),
                    case["expected_json"].as_str().unwrap().as_bytes(),
                    "{label}: literal native text bytes: {case}"
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&text)?,
                    case["expected"],
                    "{label}: complete output value: {case}"
                );
            } else {
                let bytes: Vec<u8> = serde_json::from_value(case["bytes"].clone())?;
                assert!(
                    std::str::from_utf8(&bytes).is_err(),
                    "{label}: UTF-8 refusal control"
                );
            }
        }
        for case in catalogue["target_rejections"].as_array().unwrap() {
            assert!(
                format_json::from_str(case["input"].as_str().unwrap(), &imported.project.target)
                    .is_err(),
                "{label}: target domain accepted {case}"
            );
        }
        let project_path = directory.0.join(format!("{label}.json"));
        std::fs::write(
            &project_path,
            mapping::project_file::encode_pretty(&imported.project)?,
        )?;
        let cases = serde_json::to_vec(cases)?;
        let rust_output = directory.0.join(format!("{label}-rust"));
        generate_project(
            &project_path,
            &rust_output,
            GenerateTarget::Rust {
                runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
            },
        )?;
        let manifest_path = rust_output.join("Cargo.toml");
        let manifest = std::fs::read_to_string(&manifest_path)?;
        assert!(manifest.contains("\n[workspace]"));
        std::fs::write(manifest_path, manifest.replace(
            "\n[workspace]",
            "\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n\n[workspace]",
        ))?;
        std::fs::write(rust_output.join("cases.json"), &cases)?;
        std::fs::write(rust_output.join("design.txt"), label)?;
        std::fs::write(
            rust_output.join("src/main.rs"),
            include_str!("fixtures/json_enclosing_frame_clones_rust.rs.txt"),
        )?;
        host_policy::name_generated_host(&rust_output, "json-enclosing-frame-clones-host")?;
        let mut rust_command = Command::new("cargo");
        rust_command
            .args(["run", "--quiet"])
            .current_dir(&rust_output)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env("RUSTFLAGS", "-Dwarnings");
        let rust = host_policy::recorded_output(&mut rust_command, &rust_output, "rust-run")?;
        assert!(
            rust.status.success(),
            "{label}: generated Rust failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&rust.stdout),
            String::from_utf8_lossy(&rust.stderr)
        );

        let csharp_output = directory.0.join(format!("{label}-csharp"));
        generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
        std::fs::write(csharp_output.join("cases.json"), &cases)?;
        std::fs::write(csharp_output.join("design.txt"), label)?;
        let harness = csharp_output.join("Harness");
        std::fs::create_dir(&harness)?;
        std::fs::write(
            harness.join("Harness.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
        )?;
        std::fs::write(
            harness.join("Program.cs"),
            include_str!("fixtures/json_enclosing_frame_clones_csharp.cs.txt"),
        )?;
        let mut csharp_command = dotnet_command(&csharp_output);
        csharp_command
            .args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
            ])
            .current_dir(&csharp_output);
        let csharp =
            host_policy::recorded_output(&mut csharp_command, &csharp_output, "csharp-run")?;
        assert!(
            csharp.status.success(),
            "{label}: generated C# failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&csharp.stdout),
            String::from_utf8_lossy(&csharp.stderr)
        );
    }
    Ok(())
}
