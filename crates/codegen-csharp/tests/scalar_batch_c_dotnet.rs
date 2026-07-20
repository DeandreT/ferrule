use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{Binding, Expression, ExpressionNode, Program, ScalarFunction, TargetScope};
use ir::{ScalarType, SchemaNode};

#[test]
fn generated_scalar_batch_c_preserves_runtime_semantics() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("scalar batch C fixture emits");
    let directory = TempDirectory::new("scalar-batch-c-dotnet");
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated scalar batch C passed"
    );
}

fn fixture() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Text", ScalarType::String),
                SchemaNode::scalar("Numeric", ScalarType::String),
                SchemaNode::scalar("Duration", ScalarType::Float),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Trimmed", ScalarType::String),
                SchemaNode::scalar("Numeric", ScalarType::Bool),
                SchemaNode::scalar("Number", ScalarType::String),
                SchemaNode::scalar("Delayed", ScalarType::String),
            ],
        ),
        expressions: vec![
            source_field(1, "Text"),
            source_field(2, "Numeric"),
            source_field(3, "Duration"),
            call(4, ScalarFunction::Trim, &[1]),
            call(5, ScalarFunction::IsNumeric, &[2]),
            call(6, ScalarFunction::ToNumber, &[2]),
            call(7, ScalarFunction::DelayPassthrough, &[4, 3]),
        ],
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![
                binding("Trimmed", 4, ScalarType::String),
                binding("Numeric", 5, ScalarType::Bool),
                binding("Number", 6, ScalarType::String),
                binding("Delayed", 7, ScalarType::String),
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn source_field(id: u32, field: &str) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: None,
            path: vec![field.into()],
        },
    }
}

fn call(id: u32, function: ScalarFunction, args: &[u32]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Call {
            function,
            args: args.to_vec(),
        },
    }
}

fn binding(target_field: &str, expression: u32, target_type: ScalarType) -> Binding {
    Binding {
        target_field: target_field.into(),
        expression,
        target_type,
        repeating: false,
    }
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

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var output = Execute("\u0085\u2003value\u3000", Text("6.022e23"), 0.0);
Equal(Text("value"), Field(output, "Trimmed"));
Equal(Bool(true), Field(output, "Numeric"));
Equal(FerruleValue.FromDouble(6.022e23), Field(output, "Number"));
Equal(Text("value"), Field(output, "Delayed"));

var boundary = Execute(" value ", Text("9223372036854775807"), 0.25);
Equal(FerruleValue.FromInt64(long.MaxValue), Field(boundary, "Number"));

var beyondBoundary = Execute(" value ", Text("9223372036854775808"), -0.0);
Equal(FerruleValueKind.Double, Field(beyondBoundary, "Number").Kind);

var missing = Execute(" value ", FerruleValue.Null, 0.0);
Equal(Bool(false), Field(missing, "Numeric"));
Equal(FerruleValue.Null, Field(missing, "Number"));

RuntimeError(
    FerruleRuntimeError.FunctionInvalidArgument,
    "to_number",
    "requires a finite numeric value",
    () => Execute("value", Bool(true), 0.0));
RuntimeError(
    FerruleRuntimeError.FunctionInvalidArgument,
    "delay_passthrough",
    "requires a finite nonnegative duration",
    () => Execute("value", Text("1"), -0.01));

Console.WriteLine("generated scalar batch C passed");

static FerruleGroup Execute(string text, FerruleValue numeric, double duration) =>
    (FerruleGroup)GeneratedMapping.Execute(Group(
        new FerruleField("Text", Scalar(Text(text))),
        new FerruleField("Numeric", Scalar(numeric)),
        new FerruleField("Duration", Scalar(FerruleValue.FromDouble(duration)))));

static FerruleValue Field(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;

static void RuntimeError(
    FerruleRuntimeError expected,
    string function,
    string detail,
    Action action)
{
    try
    {
        action();
    }
    catch (FerruleRuntimeException exception)
    {
        Equal(expected, exception.Error);
        Equal(function, exception.Function);
        Equal(detail, exception.Detail);
        return;
    }
    throw new InvalidOperationException($"Expected runtime error {expected}.");
}

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
    }
}

static FerruleValue Text(string value) => FerruleValue.FromString(value);
static FerruleValue Bool(bool value) => FerruleValue.FromBoolean(value);
static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
"#;

fn assert_command_succeeded(name: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{name} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_codegen_csharp_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
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
