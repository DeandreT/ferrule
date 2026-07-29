use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

#[test]
fn generated_mapping_matches_dynamic_document_interpreter_output() {
    let project = project();
    let source = source();
    let interpreted =
        engine::run(&project, &source).expect("interpreter creates dynamic documents");
    let program = codegen::lower(&project).expect("dynamic documents lower");
    let artifacts = codegen_csharp::emit(&program).expect("dynamic document project emits");
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
        "generated dynamic documents passed"
    );

    let Instance::DocumentSet(documents) = interpreted else {
        panic!("interpreter should return a document set")
    };
    assert_eq!(
        documents
            .iter()
            .map(|document| document.path())
            .collect::<Vec<_>>(),
        ["outputs/first.xml", "outputs/second.xml"]
    );
}

fn project() -> Project {
    Project {
        source: schema("Source"),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
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
                    0,
                    Node::SourceField {
                        path: vec!["OutputPath".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path: 0,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("OutputPath", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
    )
}

fn source() -> Instance {
    Instance::DocumentSet(vec![
        member(
            "inputs/first.xml",
            "/resolved/first.xml",
            "outputs/first.xml",
            "first",
        ),
        member(
            "inputs/second.xml",
            "/resolved/second.xml",
            "outputs/second.xml",
            "second",
        ),
    ])
}

fn member(portable: &str, resolved: &str, output_path: &str, value: &str) -> DocumentMember {
    DocumentMember::new_source(
        portable,
        resolved,
        Instance::Group(vec![
            (
                "OutputPath".into(),
                Instance::Scalar(Value::String(output_path.into())),
            ),
            (
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            ),
        ]),
    )
    .expect("fixture member is valid")
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

static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleScalar Text(string value) => new(FerruleValue.FromString(value));
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleDocument Document(
    string portable,
    string resolved,
    string outputPath,
    string value) =>
    new(
        portable,
        Group(
            Field("OutputPath", Text(outputPath)),
            Field("Value", Text(value))),
        resolved);

var source = new FerruleDocumentSet(new[]
{
    Document("inputs/first.xml", "/resolved/first.xml", "outputs/first.xml", "first"),
    Document("inputs/second.xml", "/resolved/second.xml", "outputs/second.xml", "second"),
});
var output = GeneratedMapping.Execute(source);
if (output is not FerruleDocumentSet documents || documents.Documents.Count != 2)
{
    throw new Exception("expected two output documents");
}
var first = documents.Documents[0];
if (first.Path != "outputs/first.xml" ||
    first.ResolvedSourcePath is not null ||
    first.Value is not FerruleGroup firstGroup ||
    !firstGroup.TryGetField("Value", out var firstValue) ||
    firstValue is not FerruleScalar firstScalar ||
    firstScalar.Value.StringValue != "first")
{
    throw new Exception("first output document does not match");
}
var second = documents.Documents[1];
if (second.Path != "outputs/second.xml" || second.ResolvedSourcePath is not null)
{
    throw new Exception("second output document does not match");
}

try
{
    _ = FerruleDynamicDocuments.Create(7U, FerruleValue.FromInt64(3), Group());
    throw new Exception("non-string path should fail");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.DynamicTargetPath && error.Node == 7U)
{
}
try
{
    _ = FerruleDynamicDocuments.Create(8U, FerruleValue.FromString("  "), Group());
    throw new Exception("empty path should fail");
}
catch (FerruleRuntimeException error)
    when (error.Error == FerruleRuntimeError.EmptyDynamicTargetPath && error.Node == 8U)
{
}

Console.WriteLine("generated dynamic documents passed");
"#;

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_dynamic_documents_dotnet_{}_{}",
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
