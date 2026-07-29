use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction, TargetScope,
};
use ir::{JsonPatternConstraints, ScalarType, SchemaNode};

#[test]
fn emitted_package_enforces_source_and_normalized_target_patterns()
-> Result<(), Box<dyn std::error::Error>> {
    let source_patterns = JsonPatternConstraints::new([["^A"]])?;
    let target_patterns = JsonPatternConstraints::new([["Z$"]])?;
    let source = SchemaNode::scalar("Source", ScalarType::String)
        .with_json_patterns(source_patterns)
        .ok_or("source pattern metadata is valid")?;
    let target = SchemaNode::scalar("Target", ScalarType::String)
        .with_json_patterns(target_patterns)
        .ok_or("target pattern metadata is valid")?;
    let program = Program {
        source,
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
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    run_generated(&program, PATTERN_HARNESS, "generated JSON patterns passed")
}

#[test]
fn emitted_package_uses_rust_float_lexicals_for_string_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let target_patterns = JsonPatternConstraints::new([
        ["^1$"],
        ["^-0$"],
        ["^100000000000000000000$"],
        [r"^0\.0000001$"],
    ])?;
    let target = SchemaNode::scalar("Target", ScalarType::String)
        .with_json_patterns(target_patterns)
        .ok_or("target pattern metadata is valid")?;
    let program = Program {
        source: SchemaNode::scalar("Source", ScalarType::Float),
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
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    assert_eq!(1.0_f64.to_string(), "1");
    assert_eq!((-0.0_f64).to_string(), "-0");
    assert_eq!(1e20_f64.to_string(), "100000000000000000000");
    assert_eq!(1e-7_f64.to_string(), "0.0000001");
    run_generated(
        &program,
        FLOAT_STRING_HARNESS,
        "generated float string lexicals passed",
    )
}

fn run_generated(
    program: &Program,
    harness: &str,
    expected: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(program)?;
    let directory = TempDirectory::new();
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.contents)?;
    }
    write_harness(directory.path(), harness)?;

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
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), expected);
    Ok(())
}

fn write_harness(root: &Path, program: &str) -> Result<(), std::io::Error> {
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
    std::fs::write(directory.join("Program.cs"), program)?;
    Ok(())
}

const PATTERN_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

if (GeneratedMapping.ExecuteJson("\"ABZ\"") != "\"ABZ\"\n")
{
    throw new Exception("matching source and target should execute");
}

try
{
    _ = GeneratedMapping.ExecuteJson("\"BBZ\"");
    throw new Exception("source pattern mismatch should fail");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.JsonBoundary)
{
}

try
{
    _ = GeneratedMapping.ExecuteJson("\"AX\"");
    throw new Exception("normalized target pattern mismatch should fail");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.JsonBoundary)
{
}

Console.WriteLine("generated JSON patterns passed");
"#;

const FLOAT_STRING_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

foreach (var (input, expected) in new[]
         {
             ("1.0", "\"1\"\n"),
             ("-0.0", "\"-0\"\n"),
             ("1e20", "\"100000000000000000000\"\n"),
             ("1e-7", "\"0.0000001\"\n"),
         })
{
    var actual = GeneratedMapping.ExecuteJson(input);
    if (!string.Equals(actual, expected, StringComparison.Ordinal))
    {
        throw new Exception(
            $"float string lexical mismatch for {input}: expected {expected}, got {actual}");
    }
}

try
{
    _ = GeneratedMapping.ExecuteJson("2.0");
    throw new Exception("unlisted normalized float should fail target pattern validation");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.JsonBoundary)
{
}

Console.WriteLine("generated float string lexicals passed");
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
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_pattern_dotnet_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&path).expect("temporary directory is created");
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
