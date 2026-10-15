use super::*;

#[test]
fn required_runtime_parser_fields_match_generated_rust_and_csharp() -> TestResult<()> {
    let directory = TempDir::new("constructed_parser_inputs")?;
    let host_target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.0.join("cargo-target"));
    assert!(
        host_target.is_absolute(),
        "generated host target must be absolute"
    );
    for design in [
        "json-scalar.mfd",
        "edi-scalar.mfd",
        "json-copy-all.mfd",
        "edi-copy-all.mfd",
        "nested-json-copy-all.mfd",
        "nested-edi-copy-all.mfd",
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/constructed-parser-runtime-inputs")
            .join(design);
        let imported = mfd::import_with_options(
            &fixture,
            &mfd::ImportOptions::default()
                .with_package_root(fixture.parent().unwrap().parent().unwrap()),
        )?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let label = design.trim_end_matches(".mfd");
        let project_path = directory.0.join(format!("{label}.json"));
        std::fs::write(&project_path, serde_json::to_vec_pretty(&imported.project)?)?;
        let cases = std::fs::read(fixture.parent().unwrap().join(
            if design.starts_with("nested-") {
                "nested-cases.json"
            } else {
                "cases.json"
            },
        ))?;
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
        std::fs::write(
            rust_output.join("src/main.rs"),
            include_str!("fixtures/constructed_parser_runtime_rust.rs.txt"),
        )?;
        let mut rust_command = Command::new("cargo");
        rust_command
            .args(["run", "--quiet"])
            .current_dir(&rust_output)
            .env("CARGO_BUILD_BUILD_DIR", &host_target)
            .env("CARGO_TARGET_DIR", &host_target)
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env("RUSTFLAGS", "-Dwarnings");
        let rust = host_policy::recorded_output(&mut rust_command, &rust_output, "rust-run")?;
        assert!(
            rust.status.success(),
            "{design}: generated Rust failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&rust.stdout),
            String::from_utf8_lossy(&rust.stderr)
        );
        let csharp_output = directory.0.join(format!("{label}-csharp"));
        generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
        std::fs::write(csharp_output.join("cases.json"), &cases)?;
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
            include_str!("fixtures/constructed_parser_runtime_csharp.cs.txt"),
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
            "{design}: generated C# failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&csharp.stdout),
            String::from_utf8_lossy(&csharp.stderr)
        );
    }
    Ok(())
}
