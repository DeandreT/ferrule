//! Opt-in generated-backend execution against one local, gitignored mapping.
//! Run with `cargo test -p cli --features codegen-tests --test code_generation
//! reference_corpus -- --ignored --nocapture` when the local sample corpus and
//! .NET 10 SDK are available. No sample contents are copied into this test.

use super::*;

const SAMPLE: &str = "EmployeesToJSONObject.mfd";
const INPUT: &str = "Altova_Hierarchical.json";

#[test]
#[ignore = "requires the local ignored ReferenceSamples corpus and .NET 10 SDK"]
fn generated_rust_and_csharp_execute_local_json_sample_like_engine() -> TestResult<()> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let mapping_path = samples.join(SAMPLE);
    let input_path = samples.join(INPUT);
    let imported = mfd::import_with_options(
        &mapping_path,
        &mfd::ImportOptions::default().with_package_root(&samples),
    )?;
    assert!(
        imported.warnings.is_empty(),
        "{SAMPLE}: import warnings: {:?}",
        imported.warnings
    );
    let project = imported.project;
    assert!(
        project.runtime_dependencies().is_empty(),
        "{SAMPLE}: host dependencies prevent deterministic execution: {:?}",
        project.runtime_dependencies()
    );
    assert!(
        project.extra_sources.is_empty(),
        "{SAMPLE}: expected one input"
    );
    assert!(
        project.source_options.json_document && project.target_options.json_document,
        "{SAMPLE}: expected JSON document boundaries"
    );
    assert_eq!(
        project
            .source_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(OsStr::to_str),
        Some(INPUT),
        "{SAMPLE}: unexpected source instance"
    );
    let validation = engine::validate(&project);
    assert!(validation.is_empty(), "{SAMPLE}: {validation:?}");

    let source_text = std::fs::read_to_string(&input_path)?;
    let source = format_json::from_str(&source_text, &project.source)?;
    let expected = engine::run(&project, &source)?;
    let expected_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?;

    let directory = TempDir::new("reference_corpus_employees_json")?;
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let rust_output = directory.0.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        r#"fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = std::env::args_os().nth(1).expect("input path");
    let input = std::fs::read_to_string(input_path)?;
    print!("{}", ferrule_generated_mapping::execute_json(&input)?);
    Ok(())
}
"#,
    )?;
    let rust_target = directory.0.join("rust-target");
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", &rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{SAMPLE}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let rust_run = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&input_path)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", &rust_target)
        .isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{SAMPLE}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    let rust_json: serde_json::Value = serde_json::from_slice(&rust_run.stdout)?;
    assert_eq!(
        rust_json, expected_json,
        "{SAMPLE}: generated Rust differs from engine"
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
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
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;

var input = File.ReadAllText(args[0]);
Console.Out.Write(GeneratedMapping.ExecuteJson(input));
"#,
    )?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{SAMPLE}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let csharp_run = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--no-build",
            "--no-restore",
            "--",
        ])
        .arg(&input_path)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{SAMPLE}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    let csharp_json: serde_json::Value = serde_json::from_slice(&csharp_run.stdout)?;
    assert_eq!(
        csharp_json, expected_json,
        "{SAMPLE}: generated C# differs from engine"
    );
    Ok(())
}
