use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    DynamicTargetBinding, Expression, ExpressionNode, NamedSourceProgram, Program,
    ScalarTargetDomain, TargetConstruction, TargetScope,
};
use ir::{
    JsonFormatAnnotations, JsonPatternConstraints, JsonPropertyNameConstraints,
    JsonPropertyNameSet, ScalarType, SchemaNode, StringLengthRange,
};

fn arbitrary_json(name: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .ok_or("test arbitrary JSON field is valid")
}

fn property_names(
    allowed: Option<&[&str]>,
    length: Option<(u64, Option<u64>)>,
    patterns: Option<&[&[&str]]>,
    formats: &[&str],
) -> Result<JsonPropertyNameConstraints, &'static str> {
    let allowed = allowed
        .map(|names| JsonPropertyNameSet::new(names.iter().map(|name| (*name).to_string())))
        .transpose()
        .map_err(|_| "test property-name set is bounded")?;
    let length = length
        .map(|(minimum, maximum)| {
            StringLengthRange::new(minimum, maximum)
                .ok_or("test property-name length is constrained")
        })
        .transpose()?;
    let patterns = patterns
        .map(|alternatives| {
            JsonPatternConstraints::new(
                alternatives
                    .iter()
                    .map(|terms| terms.iter().copied().map(str::to_string)),
            )
        })
        .transpose()
        .map_err(|_| "test property-name patterns are bounded")?;
    let formats = JsonFormatAnnotations::new(formats.iter().map(|format| (*format).to_string()))
        .map_err(|_| "test property-name formats are bounded")?;
    JsonPropertyNameConstraints::schema(allowed, length, patterns, formats)
        .ok_or("test property-name constraints are not tautological")
}

fn constrained_group(
    name: &str,
    children: Vec<SchemaNode>,
    dynamic: SchemaNode,
    constraints: JsonPropertyNameConstraints,
) -> Result<SchemaNode, &'static str> {
    SchemaNode::group(name, children)
        .with_dynamic_fields(dynamic)
        .and_then(|schema| schema.with_json_property_names(constraints))
        .ok_or("test object property-name constraints are feasible")
}

fn property_name_program() -> Result<Program, &'static str> {
    let nested_names = property_names(None, Some((0, Some(8))), Some(&[&["^$|^[a-z]+$"]]), &[])?;
    let root_names = property_names(
        Some(&[
            "",
            "Budget",
            "EmptyOnly",
            "Extra",
            "Key",
            "Maybe",
            "Nested",
            "Rows",
            "Value",
        ]),
        Some((0, Some(16))),
        Some(&[&["^$|^[A-Z][A-Za-z]*$"]]),
        &["ferrule-property-name"],
    )?;
    let named_names = property_names(
        Some(&["", "extra", "named"]),
        None,
        Some(&[&["^$|^[a-z]+$"]]),
        &["named-property"],
    )?;
    let output_names = property_names(
        None,
        Some((0, Some(8))),
        Some(&[&["^$|^[a-z]+$"]]),
        &["output-property"],
    )?;
    let expensive_names = property_names(None, None, Some(&[&["^(a?){8000}$"]]), &[])?;

    let mut maybe = constrained_group(
        "Maybe",
        vec![SchemaNode::scalar("a", ScalarType::Int)],
        SchemaNode::scalar("*", ScalarType::Int),
        nested_names.clone(),
    )?;
    maybe.container_nullable = true;
    let source = constrained_group(
        "Source",
        vec![
            constrained_group(
                "Nested",
                vec![SchemaNode::scalar("known", ScalarType::Int)],
                SchemaNode::scalar("*", ScalarType::Int),
                nested_names.clone(),
            )?,
            constrained_group(
                "Rows",
                vec![SchemaNode::scalar("a", ScalarType::Int)],
                SchemaNode::scalar("*", ScalarType::Int),
                nested_names,
            )?
            .repeating(),
            maybe,
            constrained_group(
                "EmptyOnly",
                Vec::new(),
                arbitrary_json("*")?,
                JsonPropertyNameConstraints::never(),
            )?,
            constrained_group(
                "Budget",
                Vec::new(),
                SchemaNode::scalar("*", ScalarType::Int),
                expensive_names,
            )?,
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
        arbitrary_json("*")?,
        root_names,
    )?;
    let target = constrained_group(
        "Target",
        Vec::new(),
        SchemaNode::scalar("*", ScalarType::String),
        output_names,
    )?;

    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: constrained_group(
                "Named",
                vec![SchemaNode::scalar("named", ScalarType::Int)],
                SchemaNode::scalar("*", ScalarType::Int),
                named_names,
            )?,
            dynamic: None,
        }],
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Key".into()],
                },
            },
            ExpressionNode {
                id: 2,
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
                fixed_fields: Vec::new(),
                bindings: vec![DynamicTargetBinding {
                    key: 1,
                    value: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                }],
                children: Vec::new(),
                merge: false,
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn emitted_package_enforces_property_names_on_all_json_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(&property_name_program()?)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.PropertyNames.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidateOutputPropertyNames"))
    }));
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated mapping source is present UTF-8")?;
    assert!(generated.contains(r#"\"json_property_names\":{\"kind\":\"schema\""#));
    assert!(generated.contains(r#"\"formats\":[\"ferrule-property-name\"]"#));
    assert!(generated.contains(r#"\"json_property_names\":{\"kind\":\"never\"}"#));

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
        "generated JSON property names passed"
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
  "Nested":{"known":1,"":2,"extra":3},
  "Rows":[{"a":1,"other":2}],
  "Maybe":null,
  "EmptyOnly":{},
  "Key":"valid",
  "Value":"mapped",
  "Extra":{"retained":true},
  "":"empty root name"
}
""";
var named = new[] { new NamedJsonInput("Named", """{"named":1,"":2}""") };
Equal(
    "{\n  \"valid\": \"mapped\"\n}\n",
    GeneratedMapping.ExecuteJsonWithSources(valid, named));
Equal(
    "{\n  \"\": \"mapped\"\n}\n",
    GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"valid\"", "\"\"", StringComparison.Ordinal),
        named));

PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Extra\"", "\"bad-key\"", StringComparison.Ordinal),
        named),
    "bad-key");
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"extra\":3", "\"bad-key\":3", StringComparison.Ordinal),
        named),
    "bad-key");
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"other\":2", "\"bad-key\":2", StringComparison.Ordinal),
        named),
    "bad-key");
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace(
            "\"Maybe\":null",
            "\"Maybe\":{\"a\":1,\"bad-key\":2}",
            StringComparison.Ordinal),
        named),
    "bad-key");
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"EmptyOnly\":{}", "\"EmptyOnly\":{\"x\":1}", StringComparison.Ordinal),
        named),
    "x");
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid,
        new[] { new NamedJsonInput("Named", """{"named":1,"bad-key":2}""") }),
    "bad-key");
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"valid\"", "\"bad-key\"", StringComparison.Ordinal),
        named),
    "bad-key");
Equal(
    "{}\n",
    GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"valid\"", "\"bad-key\"", StringComparison.Ordinal)
            .Replace("  \"Value\":\"mapped\",\n", "", StringComparison.Ordinal),
        named));

var byteOutput = Encoding.UTF8.GetString(
    GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(valid),
        new[]
        {
            new NamedJsonBytesInput(
                "Named",
                Encoding.UTF8.GetBytes("""{"named":1,"":2}""")),
        }));
Equal("{\n  \"valid\": \"mapped\"\n}\n", byteOutput);
PropertyNameError(
    () => GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(
            valid.Replace("\"Extra\"", "\"bad-key\"", StringComparison.Ordinal)),
        new[]
        {
            new NamedJsonBytesInput(
                "Named",
                Encoding.UTF8.GetBytes("""{"named":1,"":2}""")),
        }),
    "bad-key");

var costlyName = new string('a', 8_000);
PatternBudgetError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace(
            "\"EmptyOnly\":{}",
            $"\"EmptyOnly\":{{}},\"Budget\":{{\"{costlyName}\":1}}",
            StringComparison.Ordinal),
        named));

Console.WriteLine("generated JSON property names passed");

static void Equal(string expected, string actual)
{
    if (!string.Equals(expected, actual, StringComparison.Ordinal))
    {
        throw new Exception($"expected {expected}, got {actual}");
    }
}

static void PropertyNameError(Action action, string property)
{
    try
    {
        action();
        throw new Exception($"invalid property name '{property}' should fail");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains("property name", StringComparison.Ordinal) &&
              error.Message.Contains(property, StringComparison.Ordinal))
    {
    }
}

static void PatternBudgetError(Action action)
{
    try
    {
        action();
        throw new Exception("property-name pattern work exhaustion should fail");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains("work", StringComparison.Ordinal) &&
              error.Message.Contains("limit", StringComparison.Ordinal))
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
            "ferrule_json_property_names_dotnet_{}_{}",
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
