use super::*;

#[path = "../../../mfd/tests/fixtures/enclosing-frame-string-branches/expected_project.rs"]
mod oracle;

#[test]
fn strict_string_branches_generated_complete_projects_values_xml_and_bool_errors() -> TestResult<()>
{
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/enclosing-frame-string-branches/cases.json"
    ))?;
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/enclosing-frame-string-branches");
    for design in ["physical-branches", "mixed-branches"] {
        let imported = mfd::import(&directory.join(format!("{design}.mfd")))?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project)?,
            mapping::project_file::encode_pretty(&oracle::expected(design == "mixed-branches"))?,
        );
        assert!(engine::validate(&imported.project).is_empty());
        let selected = cases
            .iter()
            .filter(|case| case["design"] == design)
            .cloned()
            .collect::<Vec<_>>();
        json_text_boundaries::run_generated_boundary_cases(
            &imported.project,
            &selected,
            &format!("conditional_strings_{design}"),
        )?;
        let xml_case = selected
            .iter()
            .find(|case| case.get("expected_xml").is_some())
            .unwrap();
        generated_xml(&imported.project, xml_case, design)?;
    }
    Ok(())
}

// These hosts compare literal expected bytes after executing the generated
// mapping. The expectation never comes from an interpreter or another host.
fn generated_xml(project: &Project, case: &serde_json::Value, name: &str) -> TestResult<()> {
    let directory = TempDir::new(&format!("conditional_strings_xml_{name}"))?;
    let root = &directory.0;
    let project_path = root.join("project.json");
    let input_path = root.join("input.json");
    let source_path = root.join("source-schema.json");
    let target_path = root.join("target-schema.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(project)?,
    )?;
    std::fs::write(&input_path, case["input"].as_str().unwrap())?;
    std::fs::write(&source_path, serde_json::to_vec(&project.source)?)?;
    std::fs::write(&target_path, serde_json::to_vec(&project.target)?)?;
    let expected = case["expected_xml"].as_str().unwrap().as_bytes();

    let rust_output = root.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        r#"use codegen_runtime::{Value, parse_json, serialize_xml};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = std::fs::read_to_string(args.next().expect("input path"))?;
    let source_schema = std::fs::read_to_string(args.next().expect("source schema path"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema path"))?;
    let source = parse_json(&source_schema, &input)?;
    let output = execute(&source)?;
    let Value::String(xml) = serialize_xml(0, &target_schema, &output, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    print!("{xml}");
    Ok(())
}
"#,
    )?;
    let rust_target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_absolute(), "generated host target must be absolute");
            path
        }
        None => root.join("cargo-target"),
    };
    host_policy::name_generated_host(&rust_output, "enclosing-frame-strings-host")?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&input_path)
        .arg(&source_path)
        .arg(&target_path)
        .current_dir(&rust_output)
        .env("CARGO_BUILD_BUILD_DIR", &rust_target)
        .env("CARGO_TARGET_DIR", rust_target)
        .env("RUSTFLAGS", "-Dwarnings")
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "{name}: Rust XML failed: {}",
        String::from_utf8_lossy(&rust.stderr)
    );
    assert_eq!(rust.stdout, expected, "{name}: Rust XML bytes");

    let csharp_output = root.join("csharp");
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
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var input = File.ReadAllText(args[0]);
var sourceSchema = File.ReadAllText(args[1]);
var targetSchema = File.ReadAllText(args[2]);
var source = FerruleJson.Parse(sourceSchema, input);
var output = GeneratedMapping.Execute(source);
Console.Out.Write(FerruleXml.Serialize(0, targetSchema, output, false, false, null).StringValue);
"#,
    )?;
    let build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        build.status.success(),
        "{name}: C# XML build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let csharp = dotnet_command(&csharp_output)
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
        .arg(&source_path)
        .arg(&target_path)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "{name}: C# XML failed: {}",
        String::from_utf8_lossy(&csharp.stderr)
    );
    assert_eq!(csharp.stdout, expected, "{name}: C# XML bytes");
    Ok(())
}
