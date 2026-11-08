use super::*;

/// The interpreter checks the same cases in `mfd/tests/valuemap_input_mode_admission.rs`.
#[test]
fn string_value_map_input_mode_matches_generated_rust_and_csharp() -> TestResult<()> {
    let directory = TempDir::new("valuemap_input_mode")?;
    let host_target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.0.join("cargo-target"));
    assert!(
        host_target.is_absolute(),
        "generated host target must be absolute"
    );
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/valuemap-input-mode");
    let cases = std::fs::read(fixtures.join("cases.json"))?;
    for design in ["direct", "function", "named-library"] {
        let strict = mfd::import_with_profile(
            &fixtures.join(format!("{design}.mfd")),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )?;
        assert!(strict.imported.warnings.is_empty(), "{design}");
        let project_path = directory.0.join(format!("{design}.json"));
        std::fs::write(
            &project_path,
            serde_json::to_vec_pretty(&strict.imported.project)?,
        )?;
        let rust_output = directory.0.join(format!("{design}-rust"));
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
            include_str!("fixtures/lazy_string_substitution_rust.rs.txt"),
        )?;
        host_policy::name_generated_host(&rust_output, "valuemap-input-mode-host")?;
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
        let csharp_output = directory.0.join(format!("{design}-csharp"));
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
            include_str!("fixtures/lazy_string_substitution_csharp.cs.txt"),
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
