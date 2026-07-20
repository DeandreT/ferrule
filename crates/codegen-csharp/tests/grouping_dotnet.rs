use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    AggregateFunction, AggregateValue, Binding, Expression, ExpressionNode, GeneratedSequence,
    GroupingPlan, IterationPlan, Program, TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

#[test]
fn generated_sequence_grouping_preserves_members_aggregates_and_positions() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("grouping fixture emits");
    let directory = TempDirectory::new("grouping-dotnet");
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
        "generated grouping passed"
    );
}

#[test]
fn emits_adjacent_and_ending_grouping_calls() {
    for (grouping, expected) in [
        (
            GroupingPlan::AdjacentBy { key: 3 },
            "GroupAdjacentBy(candidates_1",
        ),
        (
            GroupingPlan::EndingWith { predicate: 3 },
            "GroupEndingWith(candidates_1",
        ),
    ] {
        let mut program = fixture();
        program.root.children[0].iteration = program.root.children[0]
            .iteration
            .take()
            .map(|iteration| iteration.with_grouping(grouping));
        let artifacts = codegen_csharp::emit(&program).expect("grouping fixture emits");
        let generated = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .and_then(|file| std::str::from_utf8(&file.contents).ok())
            .expect("generated mapping is UTF-8");
        assert!(generated.contains(expected), "missing {expected}");
    }
}

fn fixture() -> Program {
    let member = SchemaNode::group(
        "Member",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .repeating();
    let group = SchemaNode::group(
        "Group",
        vec![
            SchemaNode::scalar("First", ScalarType::String),
            SchemaNode::scalar("Joined", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
            member,
        ],
    )
    .repeating();
    Program {
        source: SchemaNode::group("Source", Vec::new()),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", vec![group]),
        expressions: vec![
            constant(1, Value::String("a,a,b,a".into())),
            constant(2, Value::String(",".into())),
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Aggregate {
                    function: AggregateFunction::Join,
                    collection: Vec::new(),
                    value: AggregateValue::Path(Vec::new()),
                    arg: Some(2),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Position {
                    collection: Vec::new(),
                },
            },
        ],
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: Vec::new(),
            children: vec![TargetScope {
                target_field: "Group".into(),
                repeating: true,
                iteration: Some(
                    IterationPlan::generated(GeneratedSequence::Tokenize {
                        input: 1,
                        delimiter: 2,
                        item: 3,
                    })
                    .with_grouping(GroupingPlan::By { key: 3 }),
                ),
                construction: Default::default(),
                bindings: vec![
                    binding("First", 3, ScalarType::String),
                    binding("Joined", 4, ScalarType::String),
                    binding("Position", 5, ScalarType::Int),
                ],
                children: vec![TargetScope {
                    target_field: "Member".into(),
                    repeating: true,
                    iteration: Some(IterationPlan::source(Vec::new())),
                    construction: Default::default(),
                    bindings: vec![binding("Value", 3, ScalarType::String)],
                    children: Vec::new(),
                }],
            }],
        },
        extra_targets: Vec::new(),
    }
}

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
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

var output = (FerruleGroup)GeneratedMapping.Execute(new FerruleGroup(Array.Empty<FerruleField>()));
var groups = (FerruleRepeated)output.Fields.Single(field => field.Name == "Group").Value;
Equal(2, groups.Items.Count);
Check((FerruleGroup)groups.Items[0], "a", "a,a,a", 1, "a,a,a");
Check((FerruleGroup)groups.Items[1], "b", "b", 2, "b");
Console.WriteLine("generated grouping passed");

static void Check(
    FerruleGroup group,
    string first,
    string joined,
    long position,
    string members)
{
    Equal(first, Value(group, "First").StringValue);
    Equal(joined, Value(group, "Joined").StringValue);
    Equal(position, Value(group, "Position").Int64Value);
    var items = (FerruleRepeated)group.Fields.Single(field => field.Name == "Member").Value;
    Equal(
        members,
        string.Join(',', items.Items.Cast<FerruleGroup>().Select(item =>
            Value(item, "Value").StringValue)));
}

static FerruleValue Value(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
    }
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
