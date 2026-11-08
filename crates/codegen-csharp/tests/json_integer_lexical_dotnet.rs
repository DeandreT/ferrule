use test_support::{DotnetTemp, TempDir};

use std::process::Command;

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
        execution_contract: None,
        xml_boundary: None,
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
    let directory = TempDir::new("integer-lexical-dotnet").expect("temporary directory is created");
    for file in artifacts.files() {
        directory.write(file.path.as_str(), &file.contents).unwrap();
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
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .dotnet_temp(directory.path())
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
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .dotnet_temp(directory.path())
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
