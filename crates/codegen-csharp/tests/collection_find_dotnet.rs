use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarFunction, TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

#[test]
fn generated_collection_find_preserves_runtime_semantics() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("collection-find fixture emits");
    let directory = TempDirectory::new("collection-find-dotnet");
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
        "generated collection-find passed"
    );
}

fn fixture() -> Program {
    let department = SchemaNode::group(
        "Departments",
        vec![
            SchemaNode::scalar("Office", ScalarType::String),
            SchemaNode::group(
                "People",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Select", ScalarType::Bool),
                    SchemaNode::scalar("Divisor", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    let catalog_row = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Pick", ScalarType::Bool),
            SchemaNode::scalar("Code", ScalarType::String),
        ],
    )
    .repeating();
    Program {
        source: SchemaNode::group("Source", vec![department]),
        extra_sources: vec![NamedSourceProgram {
            name: "catalog".into(),
            source: SchemaNode::group("Catalog", vec![catalog_row]),
        }],
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Details", ScalarType::String),
                SchemaNode::scalar("Catalog", ScalarType::String),
            ],
        ),
        expressions: vec![
            source_field(1, &["Office"], &["Departments"]),
            source_field(2, &["Name"], &["Departments", "People"]),
            source_field(3, &["Select"], &["Departments", "People"]),
            ExpressionNode {
                id: 4,
                expression: Expression::Position {
                    collection: vec!["Departments".into()],
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Position {
                    collection: vec!["Departments".into(), "People".into()],
                },
            },
            constant(6, Value::String(":".into())),
            source_field(7, &["Divisor"], &["Departments", "People"]),
            constant(8, Value::Int(10)),
            ExpressionNode {
                id: 9,
                expression: Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![8, 7],
                },
            },
            constant(10, Value::String("/".into())),
            ExpressionNode {
                id: 11,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![4, 10, 5, 6, 1, 6, 2, 6, 9],
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::CollectionFind {
                    collection: vec!["Departments".into(), "People".into()],
                    predicate: 3,
                    value: 11,
                },
            },
            source_field(20, &["Pick"], &["catalog", "Rows"]),
            source_field(21, &["Code"], &["catalog", "Rows"]),
            ExpressionNode {
                id: 22,
                expression: Expression::CollectionFind {
                    collection: vec!["catalog".into(), "Rows".into()],
                    predicate: 20,
                    value: 21,
                },
            },
        ],
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![
                Binding {
                    target_field: "Details".into(),
                    expression: 12,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "Catalog".into(),
                    expression: 22,
                    target_type: ScalarType::String,
                    repeating: false,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn source_field(id: u32, path: &[&str], frame: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: Some(frame.iter().map(|segment| (*segment).into()).collect()),
            path: path.iter().map(|segment| (*segment).into()).collect(),
        },
    }
}

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
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

var catalog = Group(Field("Rows", Repeated(
    CatalogRow(false, "A"),
    CatalogRow(true, "B"),
    CatalogRow(true, "C"))));
var inputs = new[] { new NamedInput("catalog", catalog) };

var output = (FerruleGroup)GeneratedMapping.ExecuteWithSources(Source(), inputs);
Equal(Text("2/1:HQ:Grace:5"), ScalarValue(output, "Details"));
Equal(Text("B"), ScalarValue(output, "Catalog"));

var noMatch = (FerruleGroup)GeneratedMapping.ExecuteWithSources(Source(noMatch: true), inputs);
Equal(FerruleValue.Null, ScalarValue(noMatch, "Details"));

var documents = new FerruleDocumentSet(new[]
{
    new FerruleDocument("first.xml", Source(name: "First")),
    new FerruleDocument("second.xml", Source(name: "Second")),
});
var documentOutput = (FerruleGroup)GeneratedMapping.ExecuteWithSources(documents, inputs);
Equal(Text("2/1:HQ:First:5"), ScalarValue(documentOutput, "Details"));

RuntimeError(
    FerruleRuntimeError.MissingSourceField,
    () => GeneratedMapping.ExecuteWithSources(Group(), inputs));
RuntimeError(
    FerruleRuntimeError.NotABool,
    () => GeneratedMapping.ExecuteWithSources(Source(nonBoolean: true), inputs),
    node: 3);

Console.WriteLine("generated collection-find passed");

static FerruleGroup Source(
    bool noMatch = false,
    bool nonBoolean = false,
    string name = "Grace") =>
    Group(Field("Departments", Repeated(
        Department("Remote",
            Person("Skipped null", FerruleValue.Null, 0),
            Person("Skipped nil", FerruleValue.XmlNil, 0)),
        Department("HQ",
            Person(name, nonBoolean ? Text("not bool") : Bool(!noMatch), 2),
            Person("Must stay lazy", Bool(!noMatch), 0)))));

static FerruleGroup Department(string office, params FerruleInstance[] people) =>
    Group(
        Field("Office", Scalar(Text(office))),
        Field("People", Repeated(people)));

static FerruleGroup Person(string name, FerruleValue select, long divisor) =>
    Group(
        Field("Name", Scalar(Text(name))),
        Field("Select", Scalar(select)),
        Field("Divisor", Scalar(FerruleValue.FromInt64(divisor))));

static FerruleGroup CatalogRow(bool pick, string code) =>
    Group(
        Field("Pick", Scalar(Bool(pick))),
        Field("Code", Scalar(Text(code))));

static FerruleValue ScalarValue(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;

static void RuntimeError(FerruleRuntimeError expected, Action action, uint? node = null)
{
    try
    {
        action();
    }
    catch (FerruleRuntimeException exception)
    {
        Equal(expected, exception.Error);
        Equal(node, exception.Node);
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
static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);
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
