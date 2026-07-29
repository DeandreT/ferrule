use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, TargetConstruction,
    TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

fn open_group(name: &str) -> SchemaNode {
    let Some(schema) = SchemaNode::group(name, Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
    else {
        panic!("a group schema accepts dynamic fields");
    };
    schema
}

fn fixture() -> Program {
    let binding = |target_field: &str, expression| Binding {
        target_field: target_field.into(),
        expression,
        target_type: ScalarType::String,
        repeating: false,
    };
    Program {
        source: SchemaNode::group("Source", vec![open_group("Properties")]),
        extra_sources: vec![NamedSourceProgram {
            name: "Config".into(),
            source: open_group("Config"),
        }],
        target: SchemaNode::group(
            "Target",
            ["Found", "Missing", "ExplicitNull", "WrongKey", "Named"]
                .into_iter()
                .map(|name| SchemaNode::scalar(name, ScalarType::String))
                .collect(),
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::Const {
                    value: Value::String("selected".into()),
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::String("missing".into()),
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const {
                    value: Value::String("nil".into()),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 1,
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 2,
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 3,
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 4,
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::DynamicSourceField {
                    object: vec!["Config".into()],
                    frame: None,
                    key: 1,
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
                binding("Found", 5),
                binding("Missing", 6),
                binding("ExplicitNull", 7),
                binding("WrongKey", 8),
                binding("Named", 9),
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_dynamic_source_fields_execute_exact_null_semantics() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("dynamic source program emits");
    let directory = TempDirectory::new("dotnet-dynamic-source");
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent directory is created");
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
}

fn write_harness(directory: &Path) {
    let harness = directory.join("Harness");
    std::fs::create_dir_all(&harness).expect("harness directory is created");
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
    .expect("harness project is written");
    std::fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleValue Text(string value) => FerruleValue.FromString(value);

var source = Group(Field(
    "Properties",
    Group(
        Field("selected", Scalar(Text("primary"))),
        Field("nil", Scalar(FerruleValue.JsonNull)))));
var config = Group(Field("selected", Scalar(Text("named"))));
var result = (FerruleGroup)GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput("Config", config) });

Equal(Text("primary"), Read(result, "Found"));
Equal(FerruleValue.Null, Read(result, "Missing"));
Equal(FerruleValue.JsonNull, Read(result, "ExplicitNull"));
Equal(FerruleValue.Null, Read(result, "WrongKey"));
Equal(Text("named"), Read(result, "Named"));

var structural = Group(Field(
    "Properties",
    Group(Field("selected", Group()))));
result = (FerruleGroup)GeneratedMapping.ExecuteWithSources(
    structural,
    new[] { new NamedInput("Config", config) });
Equal(FerruleValue.Null, Read(result, "Found"));

static FerruleValue Read(FerruleGroup group, string name)
{
    if (!group.TryGetField(name, out var field) || field is not FerruleScalar scalar)
    {
        throw new InvalidOperationException($"missing scalar output {name}");
    }
    return scalar.Value;
}

static void Equal(FerruleValue expected, FerruleValue actual)
{
    if (expected != actual)
    {
        throw new InvalidOperationException($"expected {expected.Kind}, got {actual.Kind}");
    }
}
"#,
    )
    .expect("harness source is written");
}

fn assert_command_succeeded(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ferrule_{label}_{}_{}", std::process::id(), serial));
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
