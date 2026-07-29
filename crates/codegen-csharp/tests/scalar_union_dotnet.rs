use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, Program, ScalarTargetDomain, TargetConstruction,
    TargetScope,
};
use ir::{ScalarType, ScalarTypeSet, SchemaNode};

#[test]
fn generated_union_targets_preserve_tags_and_adapt_exact_numbers() {
    let artifacts = codegen_csharp::emit(&union_program()).expect("scalar union program emits");
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated mapping is present UTF-8");
    assert!(generated.contains("TargetScalarDomain.String | TargetScalarDomain.Int64"));
    assert!(generated.contains("TargetScalarDomain.Double | TargetScalarDomain.Bool"));
    assert!(generated.contains(r#"\"kind\":\"scalar_union\""#));

    let directory = TempDirectory::new();
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent is created");
        }
        std::fs::write(path, &file.contents).expect("artifact is written");
    }
    write_harness(directory.path());

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
        .expect("dotnet build starts");
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
        .output()
        .expect("generated harness starts");
    assert_command_succeeded("generated harness", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated scalar union passed"
    );
}

#[test]
fn scalar_root_construction_renders_union_adaptation() {
    let (_, types) = scalar_union("Target", [ScalarType::Float, ScalarType::Bool]);
    let program = Program {
        source: SchemaNode::scalar("Source", ScalarType::Int),
        extra_sources: Vec::new(),
        target: SchemaNode::scalar_union("Target", types),
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
                target_domain: ScalarTargetDomain::Union(types),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };

    let artifacts = codegen_csharp::emit(&program).expect("scalar root union emits");
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated mapping is present UTF-8");
    assert!(generated.contains(
        "TargetBuilder.Scalar(Node_1(context), \
         TargetScalarDomain.Double | TargetScalarDomain.Bool)"
    ));
}

fn union_program() -> Program {
    let (value_source, value_types) = scalar_union("Value", [ScalarType::String, ScalarType::Int]);
    let (value_target, _) = scalar_union("Value", [ScalarType::String, ScalarType::Int]);
    let (number_target, number_types) =
        scalar_union("Number", [ScalarType::Float, ScalarType::Bool]);
    let (items_target, _) = scalar_union("Items", [ScalarType::Float, ScalarType::Bool]);
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                value_source,
                SchemaNode::scalar("ExactInteger", ScalarType::Int),
                SchemaNode::scalar("Flag", ScalarType::Bool),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![value_target, number_target, items_target.repeating()],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["ExactInteger".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Flag".into()],
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
                    target_field: "Value".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Union(value_types),
                    repeating: false,
                },
                Binding {
                    target_field: "Number".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Union(number_types),
                    repeating: false,
                },
                Binding {
                    target_field: "Items".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Union(number_types),
                    repeating: true,
                },
                Binding {
                    target_field: "Items".into(),
                    expression: 3,
                    target_domain: ScalarTargetDomain::Union(number_types),
                    repeating: true,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn scalar_union(name: &str, types: [ScalarType; 2]) -> (SchemaNode, ScalarTypeSet) {
    let types = ScalarTypeSet::new(types).expect("test union contains distinct scalar types");
    (SchemaNode::scalar_union(name, types), types)
}

fn write_harness(root: &Path) {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory).expect("harness directory is created");
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
    )
    .expect("harness project is written");
    std::fs::write(directory.join("Program.cs"), HARNESS).expect("harness source is written");
}

fn assert_command_succeeded(name: &str, result: &std::process::Output) {
    assert!(
        result.status.success(),
        "{name} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
}

const HARNESS: &str = r#"
using Ferrule.Generated;
using Ferrule.Runtime;
using System.Text.Json;

static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);

var exact = (1L << 53) + 2;
var source = Group(
    Field("Value", Scalar(FerruleValue.FromInt64(7))),
    Field("ExactInteger", Scalar(FerruleValue.FromInt64(exact))),
    Field("Flag", Scalar(FerruleValue.FromBoolean(true))));
var output = GeneratedMapping.Execute(source);
if (output is not FerruleGroup group ||
    !group.TryGetField("Value", out var value) ||
    value is not FerruleScalar valueScalar ||
    valueScalar.Value != FerruleValue.FromInt64(7) ||
    !group.TryGetField("Number", out var number) ||
    number is not FerruleScalar numberScalar ||
    numberScalar.Value != FerruleValue.FromDouble(exact) ||
    !group.TryGetField("Items", out var items) ||
    items is not FerruleRepeated repeated ||
    repeated.Items.Count != 2 ||
    repeated.Items[0] is not FerruleScalar first ||
    first.Value != FerruleValue.FromDouble(exact) ||
    repeated.Items[1] is not FerruleScalar second ||
    second.Value != FerruleValue.FromBoolean(true))
{
    throw new Exception("direct scalar union output does not match");
}

var inexact = (1L << 53) + 1;
var inexactOutput = GeneratedMapping.Execute(
    Group(
        Field("Value", Scalar(FerruleValue.FromString("inexact"))),
        Field("ExactInteger", Scalar(FerruleValue.FromInt64(inexact))),
        Field("Flag", Scalar(FerruleValue.FromBoolean(false)))));
if (inexactOutput is not FerruleGroup inexactGroup ||
    !inexactGroup.TryGetField("Number", out var inexactNumber) ||
    inexactNumber is not FerruleScalar inexactScalar ||
    inexactScalar.Value != FerruleValue.FromInt64(inexact))
{
    throw new Exception("inexact integer must retain its Int64 tag");
}

var json = GeneratedMapping.ExecuteJson(
    "{\"Value\":\"external\",\"ExactInteger\":9007199254740994,\"Flag\":true}");
using var document = JsonDocument.Parse(json);
var root = document.RootElement;
var jsonItems = root.GetProperty("Items");
if (root.GetProperty("Value").GetString() != "external" ||
    root.GetProperty("Number").GetRawText() != "9007199254740994.0" ||
    jsonItems[0].GetRawText() != "9007199254740994.0" ||
    !jsonItems[1].GetBoolean())
{
    throw new Exception($"JSON scalar union output differs:\n{json}");
}

try
{
    _ = GeneratedMapping.ExecuteJson(
        "{\"Value\":\"inexact\",\"ExactInteger\":9007199254740993,\"Flag\":false}");
    throw new Exception("inexact union JSON output should fail");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.JsonBoundary)
{
}

Console.WriteLine("generated scalar union passed");
"#;

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_scalar_union_dotnet_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&path).expect("temporary directory is created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
