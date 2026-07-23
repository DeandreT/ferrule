use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::ProgramValidationError;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration, SequenceWindow,
};

fn segment(collection: &str, name: u32, branch: u32, position: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec![collection.into()]),
        iteration_output: IterationOutput::Repeated,
        bindings: vec![
            Binding {
                target_field: "Name".into(),
                node: name,
            },
            Binding {
                target_field: "Branch".into(),
                node: branch,
            },
            Binding {
                target_field: "Position".into(),
                node: position,
            },
        ],
        children: vec![Scope {
            target_field: "Details".into(),
            bindings: vec![Binding {
                target_field: "Label".into(),
                node: branch,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

fn project(output: IterationOutput) -> Project {
    let mut domestic = segment("Domestic", 1, 3, 6);
    domestic.sort_by = Some(5);
    domestic.sort_descending = true;
    domestic.windows = vec![SequenceWindow::First { count: 8 }];
    domestic.iteration_output = output;
    let mut international = segment("International", 2, 4, 7);
    international.iteration_output = output;
    let address = SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Branch", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            ),
        ],
    );
    let address = if output == IterationOutput::Repeated {
        address.repeating()
    } else {
        address
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Domestic",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Int),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "International",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", vec![address]),
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
                        frame: Some(vec!["Domestic".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        frame: Some(vec!["International".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("domestic".into()),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("international".into()),
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        frame: Some(vec!["Domestic".into()]),
                        path: vec!["Rank".into()],
                    },
                ),
                (
                    6,
                    Node::Position {
                        collection: vec!["Domestic".into()],
                    },
                ),
                (
                    7,
                    Node::Position {
                        collection: vec!["International".into()],
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Address".into(),
                iteration: ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                    domestic,
                    vec![international],
                )),
                iteration_output: output,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    fn row(name: &str, rank: i64) -> Instance {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ("Rank".into(), Instance::Scalar(Value::Int(rank))),
        ])
    }
    Instance::Group(vec![
        (
            "Domestic".into(),
            Instance::Repeated(vec![row("North", 1), row("South", 3), row("West", 2)]),
        ),
        (
            "International".into(),
            Instance::Repeated(vec![row("East", 8), row("Central", 4)]),
        ),
    ])
}

fn text<'a>(group: &'a Instance, field: &str) -> &'a str {
    let Some(Value::String(value)) = group.field(field).and_then(Instance::as_scalar) else {
        panic!("fixture field `{field}` is text");
    };
    value
}

fn integer(group: &Instance, field: &str) -> i64 {
    let Some(Value::Int(value)) = group.field(field).and_then(Instance::as_scalar) else {
        panic!("fixture field `{field}` is an integer");
    };
    *value
}

fn signature(output: &Instance) -> String {
    let Some(rows) = output.field("Address").and_then(Instance::as_repeated) else {
        panic!("engine output contains repeated Address");
    };
    rows.iter()
        .map(|row| {
            let Some(details) = row.field("Details") else {
                panic!("Address contains Details");
            };
            format!(
                "{}|{}|{}|{}",
                text(row, "Name"),
                text(row, "Branch"),
                integer(row, "Position"),
                text(details, "Label")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn generated_scope_sequences_match_engine_order_controls_and_nested_content() {
    let project = project(IterationOutput::Repeated);
    let expected = engine::run(&project, &source())
        .map(|output| signature(&output))
        .expect("engine executes scope sequence");
    let program = codegen::lower(&project).expect("scope sequence lowers");
    let artifacts = codegen_csharp::emit(&program).expect("scope sequence emits");
    let directory = TempDirectory::new("scope-sequence");
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent exists");
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
        .env("EXPECTED_OUTPUT", expected)
        .current_dir(directory.path())
        .output()
        .expect("generated harness starts");
    assert_command_succeeded("generated harness", &run);
}

#[test]
fn emits_mapped_scope_sequences_and_rejects_invalid_wrappers_atomically() {
    let mapped =
        codegen::lower(&project(IterationOutput::MappedSequence)).expect("mapped sequence lowers");
    let artifacts = codegen_csharp::emit(&mapped).expect("mapped sequence emits");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated mapping is UTF-8");
    assert!(source.contains("new global::Ferrule.Runtime.FerruleMappedSequence(outputs)"));

    let mut invalid =
        codegen::lower(&project(IterationOutput::Repeated)).expect("repeated sequence lowers");
    invalid.root.children[0].bindings.push(codegen::Binding {
        target_field: "Name".into(),
        expression: 1,
        target_type: ScalarType::String,
        repeating: false,
    });
    assert!(matches!(
        codegen_csharp::emit(&invalid),
        Err(codegen_csharp::EmitError::ProgramValidation(
            ProgramValidationError::InvalidScopeSequenceWrapper { target_path }
        )) if target_path == ["Address"]
    ));
}

fn write_harness(root: &Path) {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory).expect("harness directory exists");
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
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
    )
    .expect("harness project is written");
    std::fs::write(directory.join("Program.cs"), HARNESS).expect("harness source is written");
}

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = Group(
    Field("Domestic", Repeated(
        Row("North", 1),
        Row("South", 3),
        Row("West", 2))),
    Field("International", Repeated(
        Row("East", 8),
        Row("Central", 4))));
var output = (FerruleGroup)GeneratedMapping.Execute(source);
var rows = (FerruleRepeated)output.Fields.Single(field => field.Name == "Address").Value;
var signature = string.Join("\n", rows.Items.Cast<FerruleGroup>().Select(row =>
{
    var details = (FerruleGroup)row.Fields.Single(field => field.Name == "Details").Value;
    return $"{Text(row, "Name")}|{Text(row, "Branch")}|{Integer(row, "Position")}|{Text(details, "Label")}";
}));
Equal(Environment.GetEnvironmentVariable("EXPECTED_OUTPUT"), signature);

static FerruleGroup Row(string name, long rank) => Group(
    Field("Name", Scalar(FerruleValue.FromString(name))),
    Field("Rank", Scalar(FerruleValue.FromInt64(rank))));

static string? Text(FerruleGroup group, string name) => Value(group, name).StringValue;
static long Integer(FerruleGroup group, string name) => Value(group, name).Int64Value;
static FerruleValue Value(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;
static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
        throw new InvalidOperationException($"Expected '{expected}', got '{actual}'.");
}
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
