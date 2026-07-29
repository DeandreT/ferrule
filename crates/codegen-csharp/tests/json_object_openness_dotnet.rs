use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarTargetDomain,
    TargetConstruction, TargetScope,
};
use ir::{ScalarType, SchemaNode};

#[test]
fn emitted_package_enforces_closed_objects_and_preserves_dynamic_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let open = SchemaNode::group(
        "Open",
        vec![SchemaNode::scalar("Fixed", ScalarType::String)],
    )
    .with_dynamic_fields(
        SchemaNode::scalar("*", ScalarType::String)
            .json_any()
            .ok_or("test dynamic field accepts arbitrary JSON")?,
    )
    .ok_or("test group accepts dynamic fields")?;
    let maybe = SchemaNode::group("Maybe", vec![SchemaNode::scalar("Value", ScalarType::Int)])
        .nullable_container()
        .ok_or("test group accepts container nullability")?;
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Id", ScalarType::String),
            SchemaNode::group(
                "Nested",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Code", ScalarType::String)])
                .repeating(),
            maybe,
            open,
        ],
    );
    let program = Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Config".into(),
            source: SchemaNode::group(
                "Config",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            ),
            dynamic: None,
        }],
        target: SchemaNode::group("Target", vec![SchemaNode::scalar("Id", ScalarType::String)]),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Id".into()],
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
                target_field: "Id".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    run_generated(&program)
}

fn run_generated(program: &Program) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(program)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/FerruleJson.ObjectOpenness.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidateDeclaredProperties"))
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
        "generated JSON object openness passed"
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
  "Id": "A",
  "Nested": { "Name": "nested" },
  "Rows": [{ "Code": "one" }, { "Code": "two" }],
  "Maybe": null,
  "Open": {
    "Fixed": "known",
    "metadata": { "levels": [1, true, null] }
  }
}
""";
var named = new[] { new NamedJsonInput("Config", """{"Label":"configured"}""") };
var output = GeneratedMapping.ExecuteJsonWithSources(validSource, named);
if (!string.Equals(output, "{\n  \"Id\": \"A\"\n}\n", StringComparison.Ordinal))
{
    throw new Exception($"valid object output changed: {output}");
}

foreach (var (input, objectName, propertyName) in new[]
         {
             (
                 validSource.Replace(
                     "\"Nested\":",
                     "\"unexpected\": 1, \"later\": 2, \"Nested\":",
                     StringComparison.Ordinal),
                 "Source",
                 "unexpected"),
             (
                 validSource.Replace(
                     """{ "Name": "nested" }""",
                     """{ "Name": "nested", "unexpected": 1 }""",
                     StringComparison.Ordinal),
                 "Nested",
                 "unexpected"),
             (
                 validSource.Replace(
                     """{ "Code": "one" }""",
                     """{ "Code": "one", "unexpected": 1 }""",
                     StringComparison.Ordinal),
                 "Rows",
                 "unexpected"),
             (
                 validSource.Replace(
                     "\"Maybe\": null",
                     "\"Maybe\": { \"Value\": 1, \"unexpected\": 1 }",
                     StringComparison.Ordinal),
                 "Maybe",
                 "unexpected"),
         })
{
    ClosedError(
        () => GeneratedMapping.ExecuteJsonWithSources(input, named),
        objectName,
        propertyName);
}

ClosedError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource,
        new[]
        {
            new NamedJsonInput(
                "Config",
                """{"Label":"configured","unexpected":1}"""),
        }),
    "Config",
    "unexpected");

var byteNamed = new[]
{
    new NamedJsonBytesInput("Config", Encoding.UTF8.GetBytes("""{"Label":"configured"}""")),
};
ClosedError(
    () => GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(
            validSource.Replace(
                "\"Nested\":",
                "\"unexpected\": 1, \"Nested\":",
                StringComparison.Ordinal)),
        byteNamed),
    "Source",
    "unexpected");

Console.WriteLine("generated JSON object openness passed");

static void ClosedError(Action action, string objectName, string propertyName)
{
    try
    {
        action();
        throw new Exception(
            $"closed object {objectName} accepted property {propertyName}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains(
                  $"object '{objectName}' does not allow property '{propertyName}'",
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
            "ferrule_json_object_openness_dotnet_{}_{}",
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
