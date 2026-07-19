use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{Binding, Expression, ExpressionNode, Program, ScalarFunction, TargetScope};
use ir::{ScalarType, SchemaNode, Value};

#[test]
fn generated_library_builds_and_executes_without_packages() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("fixture emits");
    let directory = TempDirectory::new("dotnet-execution");
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
        "generated mapping passed"
    );
}

fn fixture() -> Program {
    let binding = |target_field: &str, expression, target_type, repeating| Binding {
        target_field: target_field.into(),
        expression,
        target_type,
        repeating,
    };
    Program {
        source: SchemaNode::group("source schema", Vec::new()),
        target: SchemaNode::group("target schema", Vec::new()),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    path: vec!["Account".into(), "Name".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::Float(7.0),
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const {
                    value: Value::Int(8),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const { value: Value::Null },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Const {
                    value: Value::String("first".into()),
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::Const {
                    value: Value::String("second".into()),
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::Const {
                    value: Value::Int(20),
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::Const {
                    value: Value::String("22".into()),
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::Call {
                    function: ScalarFunction::Add,
                    args: vec![7, 8],
                },
            },
            ExpressionNode {
                id: 10,
                expression: Expression::SourceField {
                    path: vec!["Condition".into()],
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::Const {
                    value: Value::Int(0),
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![11, 12],
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::If {
                    condition: 10,
                    then: 9,
                    else_: 13,
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::Call {
                    function: ScalarFunction::GreaterThan,
                    args: vec![14, 7],
                },
            },
        ],
        root: TargetScope {
            target_field: String::new(),
            repeating: true,
            bindings: vec![binding("RootInt", 2, ScalarType::Int, false)],
            children: vec![TargetScope {
                target_field: "Nested".into(),
                repeating: true,
                bindings: vec![
                    binding("Copied", 1, ScalarType::String, false),
                    binding("Lines", 5, ScalarType::String, true),
                    binding("Middle", 2, ScalarType::Int, false),
                    binding("Lines", 4, ScalarType::String, true),
                    binding("Lines", 6, ScalarType::String, true),
                    binding("ExactFloat", 3, ScalarType::Float, false),
                    binding("LazyValue", 14, ScalarType::Int, false),
                    binding("Compared", 15, ScalarType::Bool, false),
                ],
                children: Vec::new(),
            }],
        },
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
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )
    .expect("harness project is written");
    std::fs::write(
        directory.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = Source(FerruleValue.FromBoolean(true));

var outputRows = (FerruleRepeated)GeneratedMapping.Execute(source);
Assert(outputRows.Items.Count == 1);
var output = (FerruleGroup)outputRows.Items[0];
Assert(output.Fields.Select(field => field.Name).SequenceEqual(new[] { "RootInt", "Nested" }));
Assert(((FerruleScalar)output.Fields[0].Value).Value == FerruleValue.FromInt64(7));

var nestedRows = (FerruleRepeated)output.Fields[1].Value;
Assert(nestedRows.Items.Count == 1);
var nested = (FerruleGroup)nestedRows.Items[0];
Assert(nested.Fields.Select(field => field.Name).SequenceEqual(
    new[] { "Copied", "Lines", "Middle", "ExactFloat", "LazyValue", "Compared" }));
Assert(((FerruleScalar)nested.Fields[0].Value).Value == FerruleValue.FromString("Ada"));
var lines = (FerruleRepeated)nested.Fields[1].Value;
Assert(lines.Items.Count == 2);
Assert(((FerruleScalar)lines.Items[0]).Value == FerruleValue.FromString("first"));
Assert(((FerruleScalar)lines.Items[1]).Value == FerruleValue.FromString("second"));
Assert(((FerruleScalar)nested.Fields[2].Value).Value == FerruleValue.FromInt64(7));
Assert(((FerruleScalar)nested.Fields[3].Value).Value == FerruleValue.FromDouble(8.0));
Assert(((FerruleScalar)nested.Fields[4].Value).Value == FerruleValue.FromInt64(42));
Assert(((FerruleScalar)nested.Fields[5].Value).Value == FerruleValue.FromBoolean(true));

Error(
    FerruleRuntimeError.DivideByZero,
    () => GeneratedMapping.Execute(Source(FerruleValue.FromBoolean(false))));
var notBoolean = Error(
    FerruleRuntimeError.NotABool,
    () => GeneratedMapping.Execute(Source(FerruleValue.FromString("true"))));
Assert(notBoolean.Node == 10U);
Assert(notBoolean.FoundKind == FerruleValueKind.String);
Console.WriteLine("generated mapping passed");

static FerruleGroup Source(FerruleValue condition) =>
    new(new FerruleField[]
    {
        new("Account", new FerruleGroup(new FerruleField[]
        {
            new("Name", new FerruleScalar(FerruleValue.FromString("Ada"))),
        })),
        new("Condition", new FerruleScalar(condition)),
    });

static void Assert(bool condition)
{
    if (!condition)
    {
        throw new InvalidOperationException("generated mapping assertion failed");
    }
}

static FerruleRuntimeException Error(FerruleRuntimeError expected, Action action)
{
    try
    {
        action();
    }
    catch (FerruleRuntimeException exception)
    {
        Assert(exception.Error == expected);
        return exception;
    }

    throw new InvalidOperationException($"Expected Ferrule runtime error {expected}.");
}
"#,
    )
    .expect("harness source is written");
}

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
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule-codegen-csharp-{label}-{}-{unique}",
            std::process::id()
        ));
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
