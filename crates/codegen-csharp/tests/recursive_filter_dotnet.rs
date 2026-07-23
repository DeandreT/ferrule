use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Graph, Node, Project, RecursiveFilterPlan, Scope, ScopeConstruction};

fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("suffix", ScalarType::String),
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group(
                "file",
                vec![
                    SchemaNode::scalar("name", ScalarType::String),
                    SchemaNode::scalar("expected", ScalarType::Int),
                ],
            )
            .repeating(),
            SchemaNode::recursive_group("directory", "Directory").repeating(),
        ],
    )
}

fn project() -> Project {
    let Some(plan) = RecursiveFilterPlan::new("directory".into(), "file".into(), 7) else {
        panic!("valid recursive-filter plan");
    };
    let schema = directory_schema();
    Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["name".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["suffix".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "contains".into(),
                        args: vec![1, 2],
                    },
                ),
                (
                    4,
                    Node::Position {
                        collection: vec!["file".into()],
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        path: vec!["expected".into()],
                        frame: None,
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![4, 5],
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "and".into(),
                        args: vec![3, 6],
                    },
                ),
            ]),
        },
        root: Scope {
            construction: ScopeConstruction::RecursiveFilter { plan },
            ..Scope::default()
        },
    }
}

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect())
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

fn file(name: &str, expected: i64) -> Instance {
    group([
        field("name", scalar(Value::String(name.into()))),
        field("expected", scalar(Value::Int(expected))),
    ])
}

fn directory(name: &str, files: Vec<Instance>, children: Vec<Instance>) -> Instance {
    group([
        field("name", scalar(Value::String(name.into()))),
        field("file", repeated(files)),
        field("directory", repeated(children)),
    ])
}

fn source() -> Instance {
    group([
        field("suffix", scalar(Value::String(".keep".into()))),
        field("name", scalar(Value::String("root".into()))),
        field(
            "file",
            repeated([file("drop.txt", 1), file("root.keep", 2)]),
        ),
        field(
            "directory",
            repeated([directory(
                "nested",
                vec![file("nested.keep", 1), file("drop.md", 2)],
                Vec::new(),
            )]),
        ),
    ])
}

fn expected() -> Instance {
    group([
        field("suffix", scalar(Value::String(".keep".into()))),
        field("name", scalar(Value::String("root".into()))),
        field("file", repeated([file("root.keep", 2)])),
        field(
            "directory",
            repeated([directory(
                "nested",
                vec![file("nested.keep", 1)],
                Vec::new(),
            )]),
        ),
    ])
}

#[test]
fn generated_mapping_matches_engine_recursive_filter_and_typed_errors() {
    let project = project();
    assert_eq!(engine::run(&project, &source()), Ok(expected()));
    let program = codegen::lower(&project)
        .unwrap_or_else(|error| panic!("recursive-filter project lowers: {error}"));
    let stdout = run_generated(&program, HARNESS);
    assert_eq!(stdout, "generated recursive filter passed");
}

fn run_generated(program: &codegen::Program, harness: &str) -> String {
    let artifacts = codegen_csharp::emit(program)
        .unwrap_or_else(|error| panic!("recursive-filter fixture emits: {error}"));
    let directory = TempDirectory::new();
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create artifact parent: {error}"));
        }
        std::fs::write(path, &file.contents)
            .unwrap_or_else(|error| panic!("write generated artifact: {error}"));
    }
    write_harness(directory.path(), harness);

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
        .unwrap_or_else(|error| panic!("dotnet build starts: {error}"));
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
        .unwrap_or_else(|error| panic!("generated harness starts: {error}"));
    assert_command_succeeded("generated harness", &run);
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

fn write_harness(root: &Path, harness: &str) {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("create harness directory: {error}"));
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
    .unwrap_or_else(|error| panic!("write harness project: {error}"));
    std::fs::write(directory.join("Program.cs"), harness)
        .unwrap_or_else(|error| panic!("write harness source: {error}"));
}

fn assert_command_succeeded(name: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{name} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csharp-recursive-filter-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
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

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

FerruleGroup File(string name, long expected) => new([
    new("name", new FerruleScalar(FerruleValue.FromString(name))),
    new("expected", new FerruleScalar(FerruleValue.FromInt64(expected))),
]);

FerruleGroup Directory(
    string name,
    IEnumerable<FerruleInstance> files,
    IEnumerable<FerruleInstance> children) => new([
        new("name", new FerruleScalar(FerruleValue.FromString(name))),
        new("file", new FerruleRepeated(files)),
        new("directory", new FerruleRepeated(children)),
    ]);

var source = new FerruleGroup([
    new("suffix", new FerruleScalar(FerruleValue.FromString(".keep"))),
    new("name", new FerruleScalar(FerruleValue.FromString("root"))),
    new("file", new FerruleRepeated([
        File("drop.txt", 1),
        File("root.keep", 2),
    ])),
    new("directory", new FerruleRepeated([
        Directory(
            "nested",
            [File("nested.keep", 1), File("drop.md", 2)],
            Array.Empty<FerruleInstance>()),
    ])),
]);
var output = (FerruleGroup)GeneratedMapping.Execute(source);
Equal(1, Repeated(output, "file").Items.Count);
Equal("root.keep", StringField((FerruleGroup)Repeated(output, "file").Items[0], "name"));
var nested = (FerruleGroup)Repeated(output, "directory").Items[0];
Equal(1, Repeated(nested, "file").Items.Count);
Equal("nested.keep", StringField((FerruleGroup)Repeated(nested, "file").Items[0], "name"));

var shape = Throws(() => GeneratedMapping.Execute(
    new FerruleScalar(FerruleValue.FromString("not a group"))));
Equal(FerruleRuntimeError.RecursiveFilterRequiresGroup, shape.Error);
Equal("scalar", shape.FoundInstance);

var malformed = new FerruleGroup([
    new("file", new FerruleScalar(FerruleValue.FromString("not repeated"))),
]);
var collection = Throws(() => GeneratedMapping.Execute(malformed));
Equal(FerruleRuntimeError.RecursiveFilterRequiresCollection, collection.Error);
Equal("file", collection.SourceField);
Equal("scalar", collection.FoundInstance);

FerruleInstance deep = Directory(
    "leaf",
    Array.Empty<FerruleInstance>(),
    Array.Empty<FerruleInstance>());
for (var index = 0; index < 255; index++)
{
    deep = Directory(
        $"level-{index}",
        Array.Empty<FerruleInstance>(),
        [deep]);
}
GeneratedMapping.Execute(deep);
deep = Directory(
    "overflow",
    Array.Empty<FerruleInstance>(),
    [deep]);
var depth = Throws(() => GeneratedMapping.Execute(deep));
Equal(FerruleRuntimeError.RecursiveFilterDepth, depth.Error);
Equal(256, depth.MaximumDepth);

Console.WriteLine("generated recursive filter passed");

static FerruleRepeated Repeated(FerruleGroup group, string name) =>
    (FerruleRepeated)group.Fields.Single(field => field.Name == name).Value;

static string StringField(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value.StringValue;

static FerruleRuntimeException Throws(Action action)
{
    try
    {
        action();
    }
    catch (FerruleRuntimeException error)
    {
        return error;
    }
    throw new InvalidOperationException("Expected FerruleRuntimeException.");
}

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
    }
}
"#;
