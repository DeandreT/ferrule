use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{Expression, ProgramValidationError};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration};

fn item_schema() -> SchemaNode {
    let mut child = SchemaNode::recursive_group("Child", "Item");
    child.nillable = true;
    SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("id", ScalarType::String).attribute(),
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Code", ScalarType::String)],
            ),
            SchemaNode::scalar("Optional", ScalarType::String),
            SchemaNode::scalar("Nil", ScalarType::String).nillable(),
            SchemaNode::scalar("Tag", ScalarType::String).repeating(),
            child,
        ],
    )
}

fn project() -> Project {
    let item = item_schema();
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Rows", vec![item.clone()]).repeating()],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Pretty", ScalarType::String),
                        SchemaNode::scalar("Compact", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
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
                    1,
                    Node::XmlSerialize {
                        path: vec!["Item".into()],
                        frame: Some(vec!["Rows".into()]),
                        schema: item.clone(),
                        declaration: true,
                        indent: true,
                        namespace: Some("urn:ferrule:test".into()),
                    },
                ),
                (
                    2,
                    Node::XmlSerialize {
                        path: vec!["Item".into()],
                        frame: Some(vec!["Rows".into()]),
                        schema: item,
                        declaration: false,
                        indent: false,
                        namespace: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                construction: ScopeConstruction::Constructed,
                bindings: vec![
                    Binding {
                        target_field: "Pretty".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "Compact".into(),
                        node: 2,
                    },
                ],
                ..Scope::default()
            }],
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

fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn source() -> Instance {
    group([field(
        "Rows",
        repeated([group([field(
            "Item",
            group([
                field("id", scalar(string("A&\"1\n"))),
                field("Name", scalar(string("Alpha & \"Beta\""))),
                field("Details", group([field("Code", scalar(string("D<1")))])),
                field("Optional", scalar(Value::Null)),
                field("Nil", scalar(Value::xml_nil())),
                field(
                    "Tag",
                    repeated([scalar(string("one")), scalar(string("two"))]),
                ),
                field("Child", group([field("Name", scalar(string("Nested")))])),
            ]),
        )])]),
    )])
}

fn expected(output: &Instance, field_name: &str) -> String {
    output
        .field("Row")
        .and_then(Instance::as_repeated)
        .and_then(|rows| rows.first())
        .and_then(|row| row.field(field_name))
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("engine output contains {field_name}"))
}

#[test]
fn generated_xml_serializer_matches_engine_output_and_typed_failures() {
    let project = project();
    let input = source();
    let engine_output = engine::run(&project, &input).expect("interpreter serializes XML");
    let pretty = expected(&engine_output, "Pretty");
    let compact = expected(&engine_output, "Compact");
    assert!(pretty.contains("\n    <Code>D&lt;1</Code>"), "{pretty}");
    assert!(
        pretty.contains("\n  <Tag>one</Tag>\n  <Tag>two</Tag>"),
        "{pretty}"
    );

    let program = codegen::lower(&project).expect("XML project lowers");
    let artifacts = codegen_csharp::emit(&program).expect("XML project emits");
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated mapping is UTF-8");
    assert!(generated.contains("context.ResolveXmlInstance("));
    assert!(generated.contains("FerruleXml.Serialize("));

    let directory = TempDirectory::new("xml-serialize");
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
        .env("EXPECTED_PRETTY", pretty)
        .env("EXPECTED_COMPACT", compact)
        .current_dir(directory.path())
        .output()
        .expect("generated harness starts");
    assert_command_succeeded("generated harness", &run);
}

#[test]
fn rejects_malformed_xml_program_before_artifact_creation() {
    let mut program = codegen::lower(&project()).expect("fixture lowers");
    let Expression::XmlSerialize { namespace, .. } = &mut program.expressions[0].expression else {
        panic!("fixture contains XML serialization");
    };
    *namespace = Some(String::new());

    assert!(matches!(
        codegen_csharp::emit(&program),
        Err(codegen_csharp::EmitError::ProgramValidation(
            ProgramValidationError::EmptyXmlSerializeNamespace { node: 1 }
        ))
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
    std::fs::write(
        directory.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = Group(Field("Rows", Repeated(Group(Field("Item", Group(
    Field("id", Scalar(Text("A&\"1\n"))),
    Field("Name", Scalar(Text("Alpha & \"Beta\""))),
    Field("Details", Group(Field("Code", Scalar(Text("D<1"))))),
    Field("Optional", Scalar(FerruleValue.Null)),
    Field("Nil", Scalar(FerruleValue.XmlNil)),
    Field("Tag", Repeated(Scalar(Text("one")), Scalar(Text("two")))),
    Field("Child", Group(Field("Name", Scalar(Text("Nested")))))))))));
var output = (FerruleGroup)GeneratedMapping.Execute(source);
var row = (FerruleGroup)((FerruleRepeated)output.Fields.Single(field => field.Name == "Row").Value).Items[0];
Equal(Environment.GetEnvironmentVariable("EXPECTED_PRETTY"), TextValue(row, "Pretty"));
Equal(Environment.GetEnvironmentVariable("EXPECTED_COMPACT"), TextValue(row, "Compact"));

var missing = Group(Field("Rows", Repeated(Group())));
var error = Error(() => GeneratedMapping.Execute(missing));
Equal(FerruleRuntimeError.MissingSourceField, error.Error);

var malformed = Group(Field("Rows", Repeated(Group(Field(
    "Item",
    Group(Field("Details", Scalar(FerruleValue.Null))))))));
var xmlError = Error(() => GeneratedMapping.Execute(malformed));
Equal(FerruleRuntimeError.XmlSerialization, xmlError.Error);
Equal((uint?)1U, xmlError.Node);

var nested = Group(Field("Rows", Repeated(Group(
    Field("Name", Scalar(Text("outer"))),
    Field("Rows", Repeated(
        Group(Field("Name", Scalar(Text("inner-first")))),
        Group(Field("Name", Scalar(Text("inner-second"))))))))));
var outerRows = ScopeContext.FromSource(nested).IterateSource("Rows");
var innerRows = outerRows[0].IterateSource("Rows");
var pinned = (FerruleGroup)innerRows[1].ResolveXmlInstance(
    new[] { "Rows" },
    Array.Empty<string>());
Equal("inner-second", TextValue(pinned, "Name"));

static string TextValue(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value.StringValue;

static FerruleRuntimeException Error(Action action)
{
    try { action(); }
    catch (FerruleRuntimeException exception) { return exception; }
    throw new InvalidOperationException("Expected a Ferrule runtime error.");
}

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
        throw new InvalidOperationException($"Expected '{expected}', got '{actual}'.");
}

static FerruleValue Text(string value) => FerruleValue.FromString(value);
static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);
"#,
    )
    .expect("harness source is written");
}

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
