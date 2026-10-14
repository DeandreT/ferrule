use super::*;

#[path = "../../../mfd/tests/fixtures/edi-primary-global-json/project_oracle.rs"]
mod project_oracle;

#[test]
fn required_and_optional_edi_primary_parser_matches_generated_rust_and_csharp() -> TestResult<()> {
    let directory = TempDir::new("edi_primary_global_json")?;
    let host_target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.0.join("cargo-target"));
    assert!(host_target.is_absolute());
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/edi-primary-global-json");
    let mut all_cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture_root.join("cases.json"))?)?;
    assert_eq!(all_cases.len(), 36);
    let mixed_cases: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(
        fixture_root.join("optional-root-cases.json"),
    )?)?;
    assert_eq!(mixed_cases.len(), 14);
    all_cases.extend(mixed_cases);
    assert_eq!(all_cases.len(), 50);
    for design in ["root", "nested", "rows", "scalar", "optional-root"] {
        let fixture = fixture_root.join(if design == "optional-root" {
            "optional-edi-root.mfd".to_string()
        } else {
            format!("required-edi-{design}.mfd")
        });
        let imported = mfd::import_with_options(
            &fixture,
            &mfd::ImportOptions::default().with_package_root(fixture_root.parent().unwrap()),
        )?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project)?,
            serde_json::to_value(project_oracle::expected(design))?,
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let project_path = directory.0.join(format!("{design}.json"));
        std::fs::write(
            &project_path,
            mapping::project_file::encode_pretty(&imported.project)?,
        )?;
        let cases = all_cases
            .iter()
            .filter(|case| case["design"] == design)
            .collect::<Vec<_>>();
        assert!(!cases.is_empty());
        let cases = serde_json::to_vec_pretty(&cases)?;
        let source_schema = serde_json::to_vec_pretty(&imported.project.source)?;
        let target_schema = serde_json::to_vec_pretty(&imported.project.target)?;
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
        assert_eq!(manifest.matches("\n[workspace]").count(), 1);
        let dependency = |name: &str| -> TestResult<String> {
            Ok(serde_json::to_string(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("../{name}"))
                    .to_string_lossy()
                    .replace('\\', "/"),
            )?)
        };
        std::fs::write(manifest_path, manifest.replacen("\n[workspace]", &format!(
            "\nserde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\nir = {{ path = {} }}\nformat-edi = {{ path = {} }}\n\n[workspace]", dependency("ir")?, dependency("format-edi")?), 1))?;
        for (name, body) in [
            ("cases.json", &cases),
            ("source-schema.json", &source_schema),
            ("target-schema.json", &target_schema),
        ] {
            std::fs::write(rust_output.join(name), body)?;
        }
        std::fs::write(
            rust_output.join("src/main.rs"),
            include_str!("fixtures/edi_primary_global_json_rust.rs.txt"),
        )?;
        let mut rust_command = Command::new("cargo");
        rust_command
            .args(["run", "--quiet"])
            .current_dir(&rust_output)
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
        for (name, body) in [
            ("cases.json", &cases),
            ("source-schema.json", &source_schema),
            ("target-schema.json", &target_schema),
        ] {
            std::fs::write(csharp_output.join(name), body)?;
        }
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
            include_str!("fixtures/edi_primary_global_json_csharp.cs.txt"),
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
