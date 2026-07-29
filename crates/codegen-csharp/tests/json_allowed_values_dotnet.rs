use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction,
    TargetScope,
};
use ir::{FiniteF64, JsonAllowedValue, JsonAllowedValues, ScalarType, SchemaNode};

#[test]
fn emitted_package_enforces_source_and_normalized_target_allowed_values()
-> Result<(), Box<dyn std::error::Error>> {
    let source = SchemaNode::group(
        "Source",
        vec![
            allowed_scalar(
                "Status",
                ScalarType::String,
                [
                    JsonAllowedValue::JsonNull,
                    JsonAllowedValue::String("pending".into()),
                    JsonAllowedValue::String("ready".into()),
                ],
            )?,
            allowed_scalar(
                "Quantity",
                ScalarType::Float,
                [
                    JsonAllowedValue::Int(1),
                    JsonAllowedValue::Float(FiniteF64::new(1.5).ok_or("test float is finite")?),
                ],
            )?,
            SchemaNode::scalar("Raw", ScalarType::String),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![allowed_scalar(
            "Code",
            ScalarType::Int,
            [JsonAllowedValue::Int(1), JsonAllowedValue::Int(2)],
        )?],
    );
    let program = Program {
        source,
        extra_sources: Vec::new(),
        target,
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Raw".into()],
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Code".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::Int),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    run_generated(&program)
}

fn allowed_scalar<const N: usize>(
    name: &str,
    ty: ScalarType,
    values: [JsonAllowedValue; N],
) -> Result<SchemaNode, Box<dyn std::error::Error>> {
    let values = JsonAllowedValues::new(values)?;
    SchemaNode::scalar(name, ty)
        .with_json_allowed_values(values)
        .ok_or_else(|| "test JSON allowed values match scalar domain".into())
}

fn run_generated(program: &Program) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(program)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.AllowedValues.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("JsonAllowedValues"))
    }));
    let directory = TempDirectory::new()?;
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.contents)?;
    }
    write_harness(directory.path())?;

    let build = Command::new("dotnet")
        .args([
            "build",
            "-warnaserror",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(directory.path())
        .output()?;
    assert_command_succeeded("dotnet build", &build);

    let run = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--no-build",
        ])
        .current_dir(directory.path())
        .output()?;
    assert_command_succeeded("generated harness", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated JSON allowed values passed"
    );
    Ok(())
}

fn write_harness(root: &Path) -> Result<(), std::io::Error> {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(directory.join("Program.cs"), HARNESS)?;
    Ok(())
}

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

foreach (var (input, expected) in new[]
         {
             (
                 "{\"Status\":\"ready\",\"Quantity\":1,\"Raw\":\"2\"}",
                 "{\n  \"Code\": 2\n}\n"),
             (
                 "{\"Status\":null,\"Quantity\":1.0,\"Raw\":\"1\"}",
                 "{\n  \"Code\": 1\n}\n"),
             (
                 "{\"Status\":\"pending\",\"Quantity\":1.5,\"Raw\":\"2\"}",
                 "{\n  \"Code\": 2\n}\n"),
         })
{
    var actual = GeneratedMapping.ExecuteJson(input);
    if (!string.Equals(actual, expected, StringComparison.Ordinal))
    {
        throw new Exception(
            $"allowed-values output mismatch: expected {expected}, got {actual}");
    }
}

foreach (var input in new[]
         {
             "{\"Status\":\"other\",\"Quantity\":1,\"Raw\":\"2\"}",
             "{\"Status\":\"ready\",\"Quantity\":2,\"Raw\":\"2\"}",
             "{\"Status\":\"ready\",\"Quantity\":1.5000000000000002,\"Raw\":\"2\"}",
             "{\"Status\":\"ready\",\"Quantity\":1,\"Raw\":\"3\"}",
         })
{
    try
    {
        _ = GeneratedMapping.ExecuteJson(input);
        throw new Exception($"allowed-values mismatch should fail: {input}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary)
    {
    }
}

Console.WriteLine("generated JSON allowed values passed");
"#;

fn assert_command_succeeded(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_allowed_values_dotnet_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
