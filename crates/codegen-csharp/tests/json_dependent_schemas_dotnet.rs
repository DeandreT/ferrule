use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarTargetDomain,
    TargetConstruction, TargetScope,
};
use ir::{
    JsonDependentSchemaConstraint, JsonDependentSchemaConstraints, JsonPropertyDependencies,
    JsonSchemaPredicate, ScalarType, SchemaNode,
};

fn arbitrary_field() -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar("*", ScalarType::String)
        .json_any()
        .ok_or("test arbitrary JSON field is valid")
}

fn open_required_predicate(
    name: &str,
    required: &str,
    child: SchemaNode,
) -> Result<SchemaNode, &'static str> {
    SchemaNode::group(name, vec![child])
        .with_dynamic_fields(arbitrary_field()?)
        .and_then(|schema| schema.with_required_fields(vec![required.into()]))
        .ok_or("test dependent predicate is open and requires its field")
}

fn dependent_group(
    name: &str,
    children: Vec<SchemaNode>,
    rules: impl IntoIterator<Item = JsonDependentSchemaConstraint>,
) -> Result<SchemaNode, &'static str> {
    let constraints =
        JsonDependentSchemaConstraints::new(rules).ok_or("test dependent schemas are effective")?;
    SchemaNode::group(name, children)
        .with_json_dependent_schemas(constraints)
        .ok_or("test dependent schemas belong to an object")
}

fn required_only_group() -> Result<SchemaNode, &'static str> {
    let dependencies =
        JsonPropertyDependencies::new(BTreeMap::from([("Mode".into(), vec!["Value".into()])]))
            .map_err(|_| "test required-only dependency is valid")?;
    SchemaNode::group(
        "RequiredOnly",
        vec![
            SchemaNode::scalar("Mode", ScalarType::String)
                .nullable()
                .ok_or("test required-only trigger accepts JSON null")?,
            SchemaNode::scalar("Value", ScalarType::String)
                .nullable()
                .ok_or("test required-only value accepts JSON null")?,
        ],
    )
    .with_json_property_dependencies(dependencies)
    .ok_or("test object accepts required-only dependency")
}

fn program() -> Result<Program, &'static str> {
    let trigger = SchemaNode::scalar("Trigger", ScalarType::String)
        .nullable()
        .ok_or("test trigger accepts JSON null")?;
    let need = SchemaNode::scalar("Need", ScalarType::Int);
    let require_need = open_required_predicate(
        "require-Need",
        "Need",
        SchemaNode::scalar("Need", ScalarType::Int),
    )?;
    let fixed_need = SchemaNode::scalar("Need", ScalarType::Int)
        .with_fixed("7")
        .ok_or("test fixed dependent value is valid")?;
    let require_fixed_need = open_required_predicate("fixed-Need", "Need", fixed_need)?;

    let nested = dependent_group(
        "Nested",
        vec![
            SchemaNode::scalar("Mode", ScalarType::String)
                .nullable()
                .ok_or("test nested trigger accepts JSON null")?,
            SchemaNode::scalar("Value", ScalarType::Int),
        ],
        [JsonDependentSchemaConstraint::new(
            "Mode",
            JsonSchemaPredicate::schema(open_required_predicate(
                "nested-value",
                "Value",
                SchemaNode::scalar("Value", ScalarType::Int),
            )?),
        )],
    )?;
    let rows = dependent_group(
        "Rows",
        vec![
            SchemaNode::scalar("Mode", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::Int),
        ],
        [JsonDependentSchemaConstraint::new(
            "Mode",
            JsonSchemaPredicate::schema(open_required_predicate(
                "row-value",
                "Value",
                SchemaNode::scalar("Value", ScalarType::Int),
            )?),
        )],
    )?
    .repeating();
    let maybe = dependent_group(
        "Maybe",
        vec![
            SchemaNode::scalar("Block", ScalarType::String)
                .nullable()
                .ok_or("test never trigger accepts JSON null")?,
        ],
        [JsonDependentSchemaConstraint::new(
            "Block",
            JsonSchemaPredicate::never(),
        )],
    )?
    .nullable_container()
    .ok_or("test dependent object accepts container nullability")?;

    let source = dependent_group(
        "Source",
        vec![
            trigger,
            need,
            SchemaNode::scalar("Ban", ScalarType::String)
                .nullable()
                .ok_or("test ban trigger accepts JSON null")?,
            nested,
            rows,
            maybe,
            required_only_group()?,
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Optional", ScalarType::String),
        ],
        [
            JsonDependentSchemaConstraint::new(
                "Trigger",
                JsonSchemaPredicate::schema(require_need),
            ),
            JsonDependentSchemaConstraint::new(
                "Trigger",
                JsonSchemaPredicate::schema(require_fixed_need),
            ),
            JsonDependentSchemaConstraint::new("Ban", JsonSchemaPredicate::never()),
        ],
    )?;

    let named = dependent_group(
        "Config",
        vec![
            SchemaNode::scalar("Enabled", ScalarType::String)
                .nullable()
                .ok_or("test named trigger accepts JSON null")?,
            SchemaNode::scalar("Code", ScalarType::String),
        ],
        [JsonDependentSchemaConstraint::new(
            "Enabled",
            JsonSchemaPredicate::schema(open_required_predicate(
                "named-code",
                "Code",
                SchemaNode::scalar("Code", ScalarType::String),
            )?),
        )],
    )?;

    let target = dependent_group(
        "Target",
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Present", ScalarType::String),
        ],
        [JsonDependentSchemaConstraint::new(
            "Label",
            JsonSchemaPredicate::schema(open_required_predicate(
                "target-present",
                "Present",
                SchemaNode::scalar("Present", ScalarType::String),
            )?),
        )],
    )?;

    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Config".into(),
            source: named,
            dynamic: None,
        }],
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Label".into()],
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
                    target_field: "Label".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
                },
                Binding {
                    target_field: "Present".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn emitted_package_enforces_dependent_schemas_across_json_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(&program()?)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.DependentSchemas.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("WriteDependentObject"))
    }));
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated mapping source is present UTF-8")?;
    assert!(generated.contains(
        r#"\"json_dependent_schemas\":[{\"trigger\":\"Trigger\",\"predicate\":{\"kind\":\"schema\""#
    ));
    assert!(generated.contains(r#"\"predicate\":{\"kind\":\"never\"}"#));
    assert!(generated.contains(r#"\"json_property_dependencies\":{\"Mode\":[\"Value\"]}"#));

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
        "generated dependent schemas passed"
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

const string valid = """
{
  "Trigger":null,
  "Need":7,
  "Nested":{"Mode":null,"Value":1},
  "Rows":[{"Mode":"x","Value":2},{}],
  "Maybe":null,
  "RequiredOnly":{"Mode":null,"Value":null},
  "Label":"emit",
  "Optional":"present"
}
""";
var named = new[] { new NamedJsonInput("Config", """{"Enabled":null,"Code":"ok"}""") };
Equal(
    "{\n  \"Label\": \"emit\",\n  \"Present\": \"present\"\n}\n",
    GeneratedMapping.ExecuteJsonWithSources(valid, named));

DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Need\":7,", "", StringComparison.Ordinal),
        named));
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Need\":7", "\"Need\":8", StringComparison.Ordinal),
        named));
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Trigger\":null,", "\"Trigger\":null,\"Ban\":null,", StringComparison.Ordinal),
        named));
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Nested\":{\"Mode\":null,\"Value\":1}", "\"Nested\":{\"Mode\":null}", StringComparison.Ordinal),
        named));
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("{\"Mode\":\"x\",\"Value\":2}", "{\"Mode\":\"x\"}", StringComparison.Ordinal),
        named));
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Maybe\":null", "\"Maybe\":{\"Block\":null}", StringComparison.Ordinal),
        named));
BoundaryError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace(
            "\"RequiredOnly\":{\"Mode\":null,\"Value\":null}",
            "\"RequiredOnly\":{\"Mode\":null}",
            StringComparison.Ordinal),
        named),
    "requires");
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid,
        new[] { new NamedJsonInput("Config", """{"Enabled":null}""") }));
DependentError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace(",\n  \"Optional\":\"present\"", "", StringComparison.Ordinal),
        named));

var bytes = Encoding.UTF8.GetString(
    GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(valid),
        new[]
        {
            new NamedJsonBytesInput(
                "Config",
                Encoding.UTF8.GetBytes("""{"Enabled":null,"Code":"ok"}""")),
        }));
Equal("{\n  \"Label\": \"emit\",\n  \"Present\": \"present\"\n}\n", bytes);
DependentError(
    () => GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(
            valid.Replace("\"Need\":7", "\"Need\":8", StringComparison.Ordinal)),
        new[]
        {
            new NamedJsonBytesInput(
                "Config",
                Encoding.UTF8.GetBytes("""{"Enabled":null,"Code":"ok"}""")),
        }));

Console.WriteLine("generated dependent schemas passed");

static void Equal(string expected, string actual)
{
    if (!string.Equals(expected, actual, StringComparison.Ordinal))
    {
        throw new Exception($"expected {expected}, got {actual}");
    }
}

static void DependentError(Action action) => BoundaryError(action, "dependent schema");

static void BoundaryError(Action action, string message)
{
    try
    {
        action();
        throw new Exception("JSON boundary violation should fail");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains(message, StringComparison.Ordinal))
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
            "ferrule_json_dependent_schemas_dotnet_{}_{}",
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
