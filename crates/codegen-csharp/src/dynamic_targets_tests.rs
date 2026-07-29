use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DynamicBinding, DynamicChild, Graph, Node, Project, Scope, ScopeIteration};

use crate::emit;

#[test]
fn generated_dotnet_mapping_matches_ordered_dynamic_target_semantics() {
    let project = project();
    let interpreted = engine::run(&project, &source()).expect("interpreter constructs open object");
    let program = codegen::lower(&project).expect("computed target properties lower");
    let artifacts = emit(&program).expect("dynamic target project emits");
    let directory = TempDir::new("dynamic_targets_csharp");
    write_artifacts(directory.path(), &artifacts);
    let harness = directory.path().join("Harness");
    fs::create_dir_all(&harness).expect("harness directory is created");
    fs::write(harness.join("Harness.csproj"), HARNESS_PROJECT).expect("project is written");
    fs::write(harness.join("Program.cs"), HARNESS).expect("harness is written");

    let run = Command::new("dotnet")
        .args(["run", "--project", "Harness.csproj", "--nologo"])
        .current_dir(&harness)
        .env("DOTNET_CLI_HOME", directory.path().join(".dotnet-home"))
        .env("NUGET_PACKAGES", directory.path().join(".nuget"))
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .output()
        .expect("generated .NET project starts");
    assert!(
        run.status.success(),
        "generated .NET project failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("generated dynamic targets passed"),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
    assert_eq!(
        interpreted,
        Instance::Group(vec![
            (
                "Engineering".into(),
                Instance::Repeated(vec![
                    target_person("Ada", "Manager", 1),
                    target_person("Linus", "Engineer", 2),
                ]),
            ),
            (
                "Sales".into(),
                Instance::Repeated(vec![target_person("Grace", "Director", 3)]),
            ),
        ])
    );
}

fn project() -> Project {
    let person_target = SchemaNode::group(
        "person",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Details", ScalarType::String),
        ],
    )
    .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::Int))
    .unwrap();
    let target = SchemaNode::group(
        "root",
        vec![SchemaNode::scalar("Reserved", ScalarType::String)],
    )
    .with_dynamic_fields(person_target.repeating())
    .unwrap();
    Project {
        source: SchemaNode::group(
            "Department",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Person",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("Title", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Float),
                    ],
                )
                .repeating(),
            ],
        )
        .repeating(),
        target,
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
                        path: vec!["Name".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["First".into()],
                        frame: None,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Title".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String("Rank".into()),
                    },
                ),
                (
                    6,
                    Node::SourceField {
                        path: vec!["Rank".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            dynamic_children: vec![DynamicChild {
                key: 0,
                scope: Scope {
                    iteration: ScopeIteration::Source(vec!["Person".into()]),
                    bindings: vec![
                        Binding {
                            target_field: "Name".into(),
                            node: 2,
                        },
                        Binding {
                            target_field: "Details".into(),
                            node: 4,
                        },
                    ],
                    dynamic_bindings: vec![DynamicBinding { key: 5, value: 6 }],
                    ..Scope::default()
                },
            }],
            merge_dynamic_fields: true,
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    Instance::Repeated(vec![
        department(
            "Engineering",
            &[("Ada", "Manager", 1.0), ("Linus", "Engineer", 2.0)],
        ),
        department("Sales", &[("Grace", "Director", 3.0)]),
    ])
}

fn department(name: &str, people: &[(&str, &str, f64)]) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(name.into()))),
        (
            "Person".into(),
            Instance::Repeated(
                people
                    .iter()
                    .map(|(first, title, rank)| source_person(first, title, *rank))
                    .collect(),
            ),
        ),
    ])
}

fn source_person(first: &str, title: &str, rank: f64) -> Instance {
    Instance::Group(vec![
        (
            "First".into(),
            Instance::Scalar(Value::String(first.into())),
        ),
        (
            "Title".into(),
            Instance::Scalar(Value::String(title.into())),
        ),
        ("Rank".into(), Instance::Scalar(Value::Float(rank))),
    ])
}

fn target_person(first: &str, title: &str, rank: i64) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(first.into()))),
        (
            "Details".into(),
            Instance::Scalar(Value::String(title.into())),
        ),
        ("Rank".into(), Instance::Scalar(Value::Int(rank))),
    ])
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("artifact parent is created");
        }
        fs::write(path, &file.contents).expect("artifact is written");
    }
}

const HARNESS_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
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
"#;

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

static FerruleInstance Person(string first, string title, double rank) =>
    new FerruleGroup(new FerruleField[]
    {
        new("First", new FerruleScalar(FerruleValue.FromString(first))),
        new("Title", new FerruleScalar(FerruleValue.FromString(title))),
        new("Rank", new FerruleScalar(FerruleValue.FromDouble(rank))),
    });

static FerruleInstance Department(
    FerruleValue name,
    params (string First, string Title, double Rank)[] people) =>
    new FerruleGroup(new FerruleField[]
    {
        new("Name", new FerruleScalar(name)),
        new("Person", new FerruleRepeated(
            people.Select(person => Person(person.First, person.Title, person.Rank)))),
    });

var source = new FerruleRepeated(new FerruleInstance[]
{
    Department(
        FerruleValue.FromString("Engineering"),
        ("Ada", "Manager", 1.0),
        ("Linus", "Engineer", 2.0)),
    Department(FerruleValue.FromString("Sales"), ("Grace", "Director", 3.0)),
});
var output = (FerruleGroup)GeneratedMapping.Execute(source);
if (string.Join(",", output.Fields.Select(field => field.Name)) != "Engineering,Sales")
{
    throw new InvalidOperationException("computed property order changed");
}
var engineering = (FerruleRepeated)output.Fields[0].Value;
var firstPerson = (FerruleGroup)engineering.Items[0];
var rank = (FerruleScalar)firstPerson.Fields.Single(field => field.Name == "Rank").Value;
if (rank.Value != FerruleValue.FromInt64(1))
{
    throw new InvalidOperationException("computed property target adaptation changed");
}

static FerruleRuntimeException Failure(FerruleInstance source)
{
    try
    {
        _ = GeneratedMapping.Execute(source);
        throw new InvalidOperationException("mapping unexpectedly succeeded");
    }
    catch (FerruleRuntimeException error)
    {
        return error;
    }
}

var duplicate = Failure(new FerruleRepeated(new FerruleInstance[]
{
    Department(FerruleValue.FromString("Same")),
    Department(FerruleValue.FromString("Same")),
}));
if (duplicate.Error != FerruleRuntimeError.DuplicateDynamicProperty ||
    duplicate.Detail != "Same")
{
    throw new InvalidOperationException("duplicate property failure changed");
}

var nonString = Failure(new FerruleRepeated(new FerruleInstance[]
{
    Department(FerruleValue.FromInt64(1)),
}));
if (nonString.Error != FerruleRuntimeError.DynamicPropertyName ||
    nonString.Node != 0U ||
    nonString.FoundKind != FerruleValueKind.Int64)
{
    throw new InvalidOperationException("property-name type failure changed");
}

var fixedCollision = Failure(new FerruleRepeated(new FerruleInstance[]
{
    Department(FerruleValue.FromString("Reserved")),
}));
if (fixedCollision.Error != FerruleRuntimeError.DuplicateDynamicProperty ||
    fixedCollision.Detail != "Reserved")
{
    throw new InvalidOperationException("fixed property collision changed");
}

Console.WriteLine("generated dynamic targets passed");
"#;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let path =
            std::env::temp_dir().join(format!("ferrule_{tag}_{}_{}", std::process::id(), nonce));
        fs::create_dir_all(&path).expect("temporary directory is created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
