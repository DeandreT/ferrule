use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarTargetDomain,
    TargetConstruction, TargetScope,
};
use ir::{PropertyCountRange, ScalarType, SchemaNode};

#[test]
fn emitted_package_enforces_source_named_and_normalized_target_property_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let exact_one =
        PropertyCountRange::new(1, Some(1)).ok_or("exact-one property-count range is valid")?;
    let root_range =
        PropertyCountRange::new(4, Some(5)).ok_or("root property-count range is valid")?;
    let exact_two =
        PropertyCountRange::new(2, Some(2)).ok_or("exact-two property-count range is valid")?;

    let nested = SchemaNode::group(
        "Nested",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Other", ScalarType::String),
        ],
    )
    .with_property_count_range(exact_one)
    .ok_or("nested object accepts an exact-one property range")?;
    let rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Other", ScalarType::String),
        ],
    )
    .repeating()
    .with_property_count_range(exact_one)
    .ok_or("repeated object items accept an exact-one property range")?;
    let maybe = SchemaNode::group("Maybe", vec![SchemaNode::scalar("Value", ScalarType::Int)])
        .nullable_container()
        .ok_or("object accepts container nullability")?
        .with_property_count_range(exact_one)
        .ok_or("nullable object accepts an exact-one property range")?;
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Id", ScalarType::String),
            nested,
            rows,
            maybe,
            SchemaNode::scalar("Optional", ScalarType::String)
                .nullable()
                .ok_or("source scalar accepts nullability")?,
        ],
    )
    .with_property_count_range(root_range)
    .ok_or("source root accepts its property-count range")?;
    let named_source = SchemaNode::group(
        "Config",
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Other", ScalarType::String),
        ],
    )
    .with_property_count_range(exact_one)
    .ok_or("named source accepts its property-count range")?;
    let target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("Id", ScalarType::String),
            SchemaNode::scalar("Maybe", ScalarType::String)
                .nullable()
                .ok_or("target scalar accepts nullability")?,
        ],
    )
    .with_property_count_range(exact_two)
    .ok_or("target accepts an exact-two property range")?;

    let program = Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Config".into(),
            source: named_source,
            dynamic: None,
        }],
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Id".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Optional".into()],
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
                    target_field: "Id".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
                },
                Binding {
                    target_field: "Maybe".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
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
        file.path.as_str() == "Runtime/Json/FerruleJson.PropertyCounts.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidateOutputPropertyCount"))
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
        "generated JSON property counts passed"
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

const HARNESS: &str = r###"using System.Text;
using Ferrule.Generated;
using Ferrule.Runtime;

const string validSource = """
{
  "Id": "first",
  "Id": "last",
  "Nested": { "Name": "nested" },
  "Rows": [{ "Code": "one" }, { "Code": "two" }],
  "Maybe": null,
  "Optional": null
}
""";
var named = new[] { new NamedJsonInput("Config", """{"Label":"configured"}""") };
var output = GeneratedMapping.ExecuteJsonWithSources(validSource, named);
if (!string.Equals(
        output,
        "{\n  \"Id\": \"last\",\n  \"Maybe\": null\n}\n",
        StringComparison.Ordinal))
{
    throw new Exception($"valid property-count output changed: {output}");
}

var byteOutput = Encoding.UTF8.GetString(
    GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(validSource),
        new[]
        {
            new NamedJsonBytesInput(
                "Config",
                Encoding.UTF8.GetBytes("""{"Label":"configured"}""")),
        }));
if (!string.Equals(byteOutput, output, StringComparison.Ordinal))
{
    throw new Exception($"byte property-count output changed: {byteOutput}");
}

PropertyCountError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "\"Nested\":",
            "\"unexpected\": true, \"Nested\":",
            StringComparison.Ordinal),
        named),
    "Source",
    6);
PropertyCountError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            """{ "Name": "nested" }""",
            "{}",
            StringComparison.Ordinal),
        named),
    "Nested",
    0);
PropertyCountError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            """{ "Code": "one" }""",
            "{}",
            StringComparison.Ordinal),
        named),
    "Rows",
    0);
PropertyCountError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "\"Maybe\": null",
            "\"Maybe\": {}",
            StringComparison.Ordinal),
        named),
    "Maybe",
    0);
PropertyCountError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource,
        new[] { new NamedJsonInput("Config", "{}") }),
    "Config",
    0);
PropertyCountError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            ",\n  \"Optional\": null",
            "",
            StringComparison.Ordinal),
        named),
    "Target",
    1);

Console.WriteLine("generated JSON property counts passed");

static void PropertyCountError(Action action, string objectName, int count)
{
    try
    {
        action();
        throw new Exception(
            $"property-count violation should fail for {objectName}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains(
                  $"object '{objectName}' has {count} properties",
                  StringComparison.Ordinal))
    {
    }
}
"###;

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
            "ferrule_json_property_counts_dotnet_{}_{}",
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
