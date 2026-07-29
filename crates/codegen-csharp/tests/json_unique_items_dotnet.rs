use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction,
    TargetScope,
};
use ir::{ScalarType, SchemaNode};

#[test]
fn emitted_package_enforces_raw_source_and_normalized_target_unique_items()
-> Result<(), Box<dyn std::error::Error>> {
    let source_items = SchemaNode::scalar("Items", ScalarType::Float)
        .repeating()
        .with_json_unique_items()
        .ok_or("test source array accepts uniqueItems")?;
    let target_codes = SchemaNode::scalar("Codes", ScalarType::Int)
        .repeating()
        .with_json_unique_items()
        .ok_or("test target array accepts uniqueItems")?;
    let program = Program {
        source: SchemaNode::group(
            "Source",
            vec![
                source_items,
                SchemaNode::scalar("First", ScalarType::String),
                SchemaNode::scalar("Second", ScalarType::String),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", vec![target_codes]),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["First".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Second".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![
                Binding {
                    target_field: "Codes".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Single(ScalarType::Int),
                    repeating: true,
                },
                Binding {
                    target_field: "Codes".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::Int),
                    repeating: true,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    run_generated(&program)
}

fn run_generated(program: &Program) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(program)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/FerruleJson.UniqueItems.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidateUniqueInputItems"))
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
        "generated JSON uniqueItems passed"
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

var output = GeneratedMapping.ExecuteJson(
    "{\"Items\":[1,2.5],\"First\":\"1\",\"Second\":\"2\"}");
if (!string.Equals(
        output,
        "{\n  \"Codes\": [\n    1,\n    2\n  ]\n}\n",
        StringComparison.Ordinal))
{
    throw new Exception($"uniqueItems output mismatch: {output}");
}

foreach (var input in new[]
         {
             "{\"Items\":[1,1.0],\"First\":\"1\",\"Second\":\"2\"}",
             "{\"Items\":[1,2.5],\"First\":\"1\",\"Second\":\"1\"}",
         })
{
    try
    {
        _ = GeneratedMapping.ExecuteJson(input);
        throw new Exception($"uniqueItems violation should fail: {input}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains("uniqueItems", StringComparison.Ordinal))
    {
    }
}

Console.WriteLine("generated JSON uniqueItems passed");
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
            "ferrule_json_unique_items_dotnet_{}_{}",
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
