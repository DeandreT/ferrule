use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarTargetDomain,
    TargetConstruction, TargetScope,
};
use ir::{JsonPropertyDependencies, ScalarType, SchemaNode};

fn dependent_group(
    name: &str,
    rules: &[(&str, &[&str])],
    children: Vec<SchemaNode>,
) -> Result<SchemaNode, &'static str> {
    let rules = rules
        .iter()
        .map(|(trigger, requirements)| {
            (
                (*trigger).to_string(),
                requirements
                    .iter()
                    .map(|requirement| (*requirement).to_string())
                    .collect(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let dependencies =
        JsonPropertyDependencies::new(rules).map_err(|_| "test dependencies are valid")?;
    SchemaNode::group(name, children)
        .with_json_property_dependencies(dependencies)
        .ok_or("test object accepts property dependencies")
}

#[test]
fn emitted_package_enforces_source_named_and_target_property_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    let maybe = dependent_group(
        "Maybe",
        &[("A", &["B"])],
        vec![
            SchemaNode::scalar("A", ScalarType::Int),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    )?
    .nullable_container()
    .ok_or("test dependent object accepts container nullability")?;
    let source = dependent_group(
        "Source",
        &[("Trigger", &["Required"])],
        vec![
            SchemaNode::scalar("Trigger", ScalarType::String)
                .nullable()
                .ok_or("test trigger accepts explicit JSON null")?,
            SchemaNode::scalar("Required", ScalarType::String)
                .nullable()
                .ok_or("test dependent value accepts explicit JSON null")?,
            dependent_group(
                "Nested",
                &[("A", &["B"])],
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?,
            dependent_group(
                "Rows",
                &[("A", &["B"])],
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?
            .repeating(),
            maybe,
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Optional", ScalarType::String),
        ],
    )?;
    let named_source = dependent_group(
        "Named",
        &[("X", &["Y"])],
        vec![
            SchemaNode::scalar("X", ScalarType::Int),
            SchemaNode::scalar("Y", ScalarType::Int),
        ],
    )?;
    let target = dependent_group(
        "Target",
        &[("Label", &["Present"])],
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Present", ScalarType::String),
        ],
    )?;
    let program = Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: named_source,
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
    };

    run_generated(&program)
}

fn run_generated(program: &Program) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(program)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.PropertyDependencies.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("ValidateOutputPropertyDependencies"))
    }));
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated mapping source is present UTF-8")?;
    assert!(generated.contains(r#"\"json_property_dependencies\":{\"Trigger\":[\"Required\"]}"#));

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
        "generated JSON property dependencies passed"
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
  "Trigger": null,
  "Required": null,
  "Nested": { "A": 1, "B": 2 },
  "Rows": [{ "A": 1, "B": 2 }],
  "Maybe": null,
  "Label": "kept",
  "Optional": "present"
}
""";
var named = new[] { new NamedJsonInput("Named", """{"X":1,"Y":2}""") };
var output = GeneratedMapping.ExecuteJsonWithSources(validSource, named);
if (!string.Equals(
        output,
        "{\n  \"Label\": \"kept\",\n  \"Present\": \"present\"\n}\n",
        StringComparison.Ordinal))
{
    throw new Exception($"valid property-dependency output changed: {output}");
}

var byteOutput = Encoding.UTF8.GetString(
    GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(validSource),
        new[]
        {
            new NamedJsonBytesInput(
                "Named",
                Encoding.UTF8.GetBytes("""{"X":1,"Y":2}""")),
        }));
if (!string.Equals(byteOutput, output, StringComparison.Ordinal))
{
    throw new Exception($"byte property-dependency output changed: {byteOutput}");
}

DependencyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "  \"Required\": null,\n",
            "",
            StringComparison.Ordinal),
        named),
    "Source",
    "Trigger",
    "Required");
DependencyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            """{ "A": 1, "B": 2 }""",
            """{ "A": 1 }""",
            StringComparison.Ordinal),
        named),
    "Nested",
    "A",
    "B");
DependencyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            """[{ "A": 1, "B": 2 }]""",
            """[{ "A": 1 }]""",
            StringComparison.Ordinal),
        named),
    "Rows",
    "A",
    "B");
DependencyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "\"Maybe\": null",
            "\"Maybe\": { \"A\": 1 }",
            StringComparison.Ordinal),
        named),
    "Maybe",
    "A",
    "B");
DependencyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource,
        new[] { new NamedJsonInput("Named", """{"X":1}""") }),
    "Named",
    "X",
    "Y");
DependencyError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        validSource.Replace(
            "  \"Optional\": \"present\"\n",
            "",
            StringComparison.Ordinal).Replace(
                "\"Label\": \"kept\",\n}",
                "\"Label\": \"kept\"\n}",
                StringComparison.Ordinal),
        named),
    "Target",
    "Label",
    "Present");

Console.WriteLine("generated JSON property dependencies passed");

static void DependencyError(
    Action action,
    string objectName,
    string trigger,
    string required)
{
    try
    {
        action();
        throw new Exception(
            $"property-dependency violation should fail for {objectName}");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains($"object '{objectName}'", StringComparison.Ordinal) &&
              error.Message.Contains($"property '{trigger}'", StringComparison.Ordinal) &&
              error.Message.Contains($"property '{required}'", StringComparison.Ordinal))
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
            "ferrule_json_property_dependencies_dotnet_{}_{}",
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
