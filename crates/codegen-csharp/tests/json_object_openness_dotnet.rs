use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarTargetDomain,
    TargetConstruction, TargetScope,
};
use ir::{JsonPatternPropertyNames, ScalarType, SchemaNode};

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
    .and_then(|schema| {
        schema.with_json_pattern_property_names(
            JsonPatternPropertyNames::new(["^[a-z]+$", "data"]).ok()?,
        )
    })
    .ok_or("test group accepts dynamic fields")?;
    let maybe = SchemaNode::group("Maybe", vec![SchemaNode::scalar("Value", ScalarType::Int)])
        .nullable_container()
        .ok_or("test group accepts container nullability")?;
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Id", ScalarType::String),
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
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
    let target = SchemaNode::group("Target", vec![SchemaNode::scalar("Id", ScalarType::String)])
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .and_then(|schema| {
            schema.with_json_pattern_property_names(
                JsonPatternPropertyNames::new(["^[a-z]+$", "put"]).ok()?,
            )
        })
        .ok_or("test target accepts selected dynamic fields")?;
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
                    path: vec!["Key".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::DynamicGroup {
                fixed_fields: vec!["Id".into()],
                bindings: vec![codegen::DynamicTargetBinding {
                    key: 2,
                    value: 3,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                }],
                children: Vec::new(),
                merge: false,
            },
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
        file.path.as_str() == "Runtime/Json/FerruleJson.ObjectOpenness.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidateDeclaredProperties"))
    }));
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.PatternProperties.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidatePatternProperties"))
    }));
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "GeneratedMapping.cs"
            && std::str::from_utf8(&file.contents).is_ok_and(|source| {
                source.contains(
                    r#"\"json_pattern_property_names\":{\"sources\":[\"^[a-z]+$\",\"data\"]}"#,
                ) && source.contains(
                    r#"\"json_pattern_property_names\":{\"sources\":[\"^[a-z]+$\",\"put\"]}"#,
                )
            })
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
  "Key": "output",
  "Value": "mapped",
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
if (!string.Equals(
        output,
        "{\n  \"Id\": \"A\",\n  \"output\": \"mapped\"\n}\n",
        StringComparison.Ordinal))
{
    throw new Exception($"valid object output changed: {output}");
}

const string explicitDefaultsSchema = """
{
  "name": "Root",
  "json_pattern_property_names": { "sources": ["^x-"] },
  "kind": {
    "kind": "group",
    "children": [
      {
        "name": "x-known",
        "repeating": false,
        "nullable": false,
        "container_nullable": false,
        "json_any": false,
        "json_formats": [],
        "json_unique_items": false,
        "alternative_mode": "exclusive",
        "kind": { "kind": "scalar", "ty": "string" }
      }
    ],
    "dynamic": {
      "name": "*",
      "kind": { "kind": "scalar", "ty": "string" }
    }
  }
}
""";
_ = FerruleJson.Parse(explicitDefaultsSchema, """{"x-known":"value"}""");

const string mismatchedOverlapSchema = """
{
  "name": "Root",
  "json_pattern_property_names": { "sources": ["^x-"] },
  "kind": {
    "kind": "group",
    "children": [
      {
        "name": "x-known",
        "nullable": true,
        "kind": { "kind": "scalar", "ty": "string" }
      }
    ],
    "dynamic": {
      "name": "*",
      "kind": { "kind": "scalar", "ty": "string" }
    }
  }
}
""";
PatternSchemaError(
    () => FerruleJson.Parse(mismatchedOverlapSchema, "{}"),
    "x-known");

var expensiveName = new string('a', 6_000);
var expensiveSelector =
    System.Text.Json.JsonSerializer.Serialize(expensiveName);
var sameSchemaChildren = string.Join(
    ",",
    Enumerable.Range(0, 3).Select(index =>
        $"{{\"name\":{System.Text.Json.JsonSerializer.Serialize(expensiveName + index)}," +
        "\"repeating\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}"));
var sameSchemaBudget =
    "{\"name\":\"Root\"," +
    $"\"json_pattern_property_names\":{{\"sources\":[{expensiveSelector}]}}," +
    "\"kind\":{\"kind\":\"group\"," +
    $"\"children\":[{sameSchemaChildren}]," +
    "\"dynamic\":{\"name\":\"*\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}}}";
_ = FerruleJson.Parse(sameSchemaBudget, "{}");

var costlyRequired = System.Text.Json.JsonSerializer.Serialize(expensiveName);
var triggerChildren = string.Join(
    ",",
    Enumerable.Range(1, 3).Select(index =>
        $"{{\"name\":\"t{index}\",\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}"));
var deduplicatedExpectationSchema =
    "{\"name\":\"Root\"," +
    $"\"json_pattern_property_names\":{{\"sources\":[{expensiveSelector}]}}," +
    "\"json_property_dependencies\":{" +
    $"\"t1\":[{costlyRequired}],\"t2\":[{costlyRequired}],\"t3\":[{costlyRequired}]" +
    "}," +
    "\"kind\":{\"kind\":\"group\"," +
    $"\"children\":[{triggerChildren}]," +
    $"\"required\":[{costlyRequired}]," +
    "\"dynamic\":{\"name\":\"*\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}}}";
_ = FerruleJson.Parse(
    deduplicatedExpectationSchema,
    $"{{{costlyRequired}:\"value\"}}");

PatternPropertyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "\"metadata\":",
            "\"bad-key\":",
            StringComparison.Ordinal),
        named),
    "Open",
    "bad-key");
PatternPropertyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "\"Key\": \"output\"",
            "\"Key\": \"bad-key\"",
            StringComparison.Ordinal),
        named),
    "Target",
    "bad-key");

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

static void PatternPropertyError(Action action, string objectName, string propertyName)
{
    try
    {
        action();
        throw new Exception(
            $"patternProperties object {objectName} accepted property {propertyName}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains(objectName, StringComparison.Ordinal) &&
              error.Message.Contains(propertyName, StringComparison.Ordinal) &&
              error.Message.Contains("patternProperties", StringComparison.Ordinal))
    {
    }
}

static void PatternSchemaError(Action action, string propertyName)
{
    try
    {
        action();
        throw new Exception(
            $"patternProperties schema accepted mismatched property {propertyName}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains(propertyName, StringComparison.Ordinal) &&
              error.Message.Contains("differs", StringComparison.Ordinal))
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
