use super::*;

#[test]
fn csv_document_lists_execute_all_rust_and_csharp_apis() -> TestResult<()> {
    let directory = TempDir::new("csv-document-output")?;
    let root = &directory.0;
    std::fs::write(
        root.join("mapping.mfd"),
        include_str!("../../../mfd/tests/fixtures/csv_document_output/grouped.mfd"),
    )?;
    let imported = mfd::import(&root.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = imported.project;
    project.target_options.csv_utf8_bom = true;
    let path = root.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let generated = root.join("rust");
    cli::generate_project_with_profile(
        &path,
        &generated,
        GenerateTarget::Rust {
            runtime_path: runtime.clone(),
        },
        cli::GenerateProfile::CsvDocumentOutput,
    )?;
    let host = root.join("rust-host");
    std::fs::create_dir_all(host.join("src"))?;
    std::fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname=\"csv-document-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={}}}\ncodegen-runtime={{path={}}}\n",
            serde_json::to_string(&generated.to_string_lossy())?,
            serde_json::to_string(&runtime.to_string_lossy())?,
        ),
    )?;
    std::fs::write(
        host.join("src/main.rs"),
        include_str!("csv_document_output/Host.rs.txt"),
    )?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("cargo-target"));
    let rust = Command::new("cargo")
        .args(["run", "--quiet", "--offline", "--jobs", "1"])
        .current_dir(&host)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&rust.stdout).trim(),
        "csv documents passed"
    );

    let generated = root.join("csharp");
    cli::generate_project_with_profile(
        &path,
        &generated,
        GenerateTarget::CSharp,
        cli::GenerateProfile::CsvDocumentOutput,
    )?;
    let host = root.join("csharp-host");
    std::fs::create_dir_all(&host)?;
    std::fs::write(
        host.join("Host.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><EnableTargetingPackDownload>false</EnableTargetingPackDownload><EnableRuntimePackDownload>false</EnableRuntimePackDownload></PropertyGroup><ItemGroup><ProjectReference Include="../csharp/Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        host.join("Program.cs"),
        include_str!("csv_document_output/Host.cs.txt"),
    )?;
    let csharp = dotnet_command(root)
        .args([
            "run",
            "--project",
            "csharp-host/Host.csproj",
            "--configuration",
            "Release",
            "-p:EnableTargetingPackDownload=false",
            "-p:EnableRuntimePackDownload=false",
        ])
        .current_dir(root)
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&csharp.stdout).trim(),
        "csv documents passed"
    );
    Ok(())
}
