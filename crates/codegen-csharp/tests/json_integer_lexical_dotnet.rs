use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction, TargetScope,
};
use ir::{
    IntegerRange, JsonMultipleOf, JsonMultipleOfConstraints, NumericRange, ScalarType, SchemaNode,
};

fn program() -> Program {
    let range = IntegerRange::new(Some(6), Some(8)).unwrap();
    let divisor = JsonMultipleOf::from_decimal_lexical("3").unwrap();
    let multiples = JsonMultipleOfConstraints::new([[divisor]]).unwrap();
    let target = SchemaNode::scalar("Target", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(range))
        .unwrap()
        .with_json_multiple_of(multiples)
        .unwrap();
    Program {
        source: SchemaNode::scalar("Source", ScalarType::String),
        extra_sources: Vec::new(),
        target,
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::Int),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_integer_json_output_normalizes_exact_decimal_strings() {
    let artifacts = codegen_csharp::emit(&program()).unwrap();
    let directory = TempDirectory::new();
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, &file.contents).unwrap();
    }
    let harness = directory.path().join("Harness");
    std::fs::create_dir_all(&harness).unwrap();
    std::fs::write(
        harness.join("Harness.csproj"),
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
    )
    .unwrap();
    std::fs::write(harness.join("Program.cs"), HARNESS).unwrap();

    let build = Command::new("dotnet")
        .args([
            "build",
            "-warnaserror",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert_succeeded("dotnet build", &build);
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
        .output()
        .unwrap();
    assert_succeeded("generated harness", &run);
}

const HARNESS: &str = r#"
using Ferrule.Generated;
using Ferrule.Runtime;
using System.Text;

var typed = GeneratedMapping.Execute(
    new FerruleScalar(FerruleValue.FromString("6.000")));
if (typed is not FerruleScalar scalar ||
    scalar.Value != FerruleValue.FromString("6.000"))
{
    throw new Exception("typed output changed before JSON serialization");
}
if (GeneratedMapping.ExecuteJson("\"6.000\"") != "6\n" ||
    Encoding.UTF8.GetString(
        GeneratedMapping.ExecuteJsonBytes(Encoding.UTF8.GetBytes("\"6.000\""))) != "6\n")
{
    throw new Exception("string and byte JSON output should normalize the exact integer");
}
foreach (var source in new[]
         {
             "\"6.001\"", "\"7.000\"", "\"9.000\"",
             "\"9223372036854775808.0\"",
         })
{
    try
    {
        _ = GeneratedMapping.ExecuteJson(source);
        throw new Exception($"invalid output was accepted: {source}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary)
    {
    }
}
try
{
    _ = GeneratedMapping.ExecuteJson("6");
    throw new Exception("integer JSON input was accepted under a string source schema");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.JsonBoundary)
{
}
"#;

fn assert_succeeded(name: &str, result: &std::process::Output) {
    assert!(
        result.status.success(),
        "{name} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_integer_lexical_dotnet_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
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
