//! Opt-in generated-backend execution against two local, gitignored mappings.
//! Run with `cargo test -p cli --features codegen-tests --test code_generation
//! reference_corpus -- --ignored --nocapture` when the local sample corpus and
//! .NET 10 SDK are available. No sample contents are copied into this test.

use super::*;

#[derive(Clone, Copy)]
enum SourceKind {
    Json,
    Xml,
}

struct CorpusCase {
    sample: &'static str,
    input: &'static str,
    source_kind: SourceKind,
}

const CASES: [CorpusCase; 2] = [
    CorpusCase {
        sample: "EmployeesToJSONObject.mfd",
        input: "Altova_Hierarchical.json",
        source_kind: SourceKind::Json,
    },
    CorpusCase {
        sample: "Altova_Hierarchical_JSON.mfd",
        input: "Altova_Hierarchical.xml",
        source_kind: SourceKind::Xml,
    },
];

#[test]
#[ignore = "requires the local ignored ReferenceSamples corpus and .NET 10 SDK"]
fn generated_rust_and_csharp_execute_local_samples_like_engine() -> TestResult<()> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let directory = TempDir::new("reference_corpus")?;
    let rust_target = directory.0.join("rust-target");
    for (index, case) in CASES.iter().enumerate() {
        let case_dir = directory.0.join(format!("case-{index}"));
        std::fs::create_dir(&case_dir)?;
        run_case(&samples, &case_dir, &rust_target, case)?;
    }
    println!(
        "{} local mappings compiled and executed in generated Rust and C#",
        CASES.len()
    );
    Ok(())
}

fn run_case(
    samples: &Path,
    case_dir: &Path,
    rust_target: &Path,
    case: &CorpusCase,
) -> TestResult<()> {
    let sample = case.sample;
    let mapping_path = samples.join(sample);
    let input_path = samples.join(case.input);
    let imported = mfd::import_with_options(
        &mapping_path,
        &mfd::ImportOptions::default().with_package_root(samples),
    )?;
    assert!(
        imported.warnings.is_empty(),
        "{sample}: import warnings: {:?}",
        imported.warnings
    );
    let project = imported.project;
    assert!(
        project.runtime_dependencies().is_empty(),
        "{sample}: host dependencies prevent deterministic execution: {:?}",
        project.runtime_dependencies()
    );
    assert!(
        project.extra_sources.is_empty(),
        "{sample}: expected one input"
    );
    assert!(
        project.target_options.json_document,
        "{sample}: expected a JSON output boundary"
    );
    assert!(
        match case.source_kind {
            SourceKind::Json => project.source_options.json_document,
            SourceKind::Xml => project.source_options.xml_document,
        },
        "{sample}: unexpected input format"
    );
    assert_eq!(
        project
            .source_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(OsStr::to_str),
        Some(case.input),
        "{sample}: unexpected source instance"
    );
    let validation = engine::validate(&project);
    assert!(validation.is_empty(), "{sample}: {validation:?}");

    let source = match case.source_kind {
        SourceKind::Json => format_json::read(&input_path, &project.source)?,
        SourceKind::Xml => format_xml::read(&input_path, &project.source)?,
    };
    // Generated hosts expose a schema-shaped JSON API. For the XML sample,
    // preserve the native reader's typed instance while crossing that host API.
    let source_json = match case.source_kind {
        SourceKind::Json => std::fs::read_to_string(&input_path)?,
        SourceKind::Xml => format_json::to_string(&project.source, &source)?,
    };
    let expected = engine::run(&project, &source)?;
    let expected_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?;

    let generated_input = case_dir.join("source.json");
    std::fs::write(&generated_input, source_json)?;
    let project_path = case_dir.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let rust_output = case_dir.join("rust");
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
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let rust_run = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&generated_input)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    let rust_json: serde_json::Value = serde_json::from_slice(&rust_run.stdout)?;
    assert_eq!(
        rust_json, expected_json,
        "{sample}: generated Rust differs from engine"
    );

    let csharp_output = case_dir.join("csharp");
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
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
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
        .arg(&generated_input)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    let csharp_json: serde_json::Value = serde_json::from_slice(&csharp_run.stdout)?;
    assert_eq!(
        csharp_json, expected_json,
        "{sample}: generated C# differs from engine"
    );
    println!("{sample}: generated Rust and C# match the interpreter");
    Ok(())
}
