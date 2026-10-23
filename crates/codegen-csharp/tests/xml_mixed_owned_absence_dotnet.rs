//! Generated C# mixed-content lookups owned by an active repeated item treat
//! its absent optional group as authoritative, matching the interpreter.
#[path = "../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;
use dotnet_temp::DotnetTemp;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_MIXED_CONTENT_FIELD, XML_NODE_NAME_FIELD,
    XML_TEXT_FIELD,
};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration, XmlMixedContentReplacement};

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect::<Vec<_>>().into())
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn text(name: &str, value: &str) -> Instance {
    group([
        field(XML_NODE_NAME_FIELD, Instance::Scalar(string(name))),
        field(XML_TEXT_FIELD, Instance::Scalar(string(value))),
    ])
}

/// Mirrors the generated harness's `Source` fixture.
fn source(fail: Option<&str>) -> Instance {
    group([
        field("Fail", Instance::Scalar(fail.map_or(Value::Null, string))),
        field(
            "Rows",
            Instance::Repeated(vec![
                group([
                    field("Name", Instance::Scalar(string("first"))),
                    field(
                        "Note",
                        group([field(
                            XML_MIXED_CONTENT_FIELD,
                            Instance::Repeated(vec![
                                text("", "one "),
                                text("Em", "x"),
                                text("", " end"),
                            ]),
                        )]),
                    ),
                ]),
                group([field("Name", Instance::Scalar(string("second")))]),
            ]),
        ),
    ])
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

/// `Note` and `FramedNote` are evaluated lazily for the row named `first`;
/// root `Fail` selects one whose absent second-row lookup is also evaluated.
fn project() -> Project {
    let mut graph = Graph::default();
    for (id, value) in [(2, "first"), (4, "skipped"), (6, "[em]")] {
        graph.nodes.insert(
            id,
            Node::Const {
                value: string(value),
            },
        );
    }
    for (id, segments) in [(1, &["Rows", "Name"][..]), (5, &["Fail"][..])] {
        graph.nodes.insert(
            id,
            Node::SourceField {
                path: path(segments),
                frame: None,
            },
        );
    }
    graph.nodes.insert(
        3,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 2],
        },
    );
    let mut bindings = vec![Binding {
        target_field: "Name".into(),
        node: 1,
    }];
    for (base, target, frame, segments) in [
        (10, "Note", None, &["Rows", "Note"][..]),
        (20, "FramedNote", Some(path(&["Rows"])), &["Note"][..]),
    ] {
        graph.nodes.extend([
            (
                base,
                Node::XmlMixedContent {
                    path: path(segments),
                    frame,
                    replacements: vec![XmlMixedContentReplacement {
                        element: "Em".into(),
                        collection: Vec::new(),
                        expression: 6,
                    }],
                },
            ),
            (
                base + 1,
                Node::Const {
                    value: string(target),
                },
            ),
            (
                base + 2,
                Node::Call {
                    function: "equal".into(),
                    args: vec![5, base + 1],
                },
            ),
            (
                base + 3,
                Node::If {
                    condition: base + 2,
                    then: base,
                    else_: 4,
                },
            ),
            (
                base + 4,
                Node::If {
                    condition: 3,
                    then: base,
                    else_: base + 3,
                },
            ),
        ]);
        bindings.push(Binding {
            target_field: target.into(),
            node: base + 4,
        });
    }
    let note = SchemaNode::group(
        "Note",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Em", ScalarType::String).repeating(),
        ],
    );
    let out = ["Name", "Note", "FramedNote"]
        .into_iter()
        .map(|name| SchemaNode::scalar(name, ScalarType::String))
        .collect();
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Fail", ScalarType::String),
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::scalar("Name", ScalarType::String), note],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", vec![SchemaNode::group("Out", out).repeating()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Out".into(),
                iteration: ScopeIteration::Source(path(&["Rows"])),
                bindings,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_mixed_content_keeps_owned_absence_like_the_interpreter() {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let row = |values: [&str; 3]| {
        group(
            ["Name", "Note", "FramedNote"]
                .into_iter()
                .zip(values)
                .map(|(name, value)| field(name, Instance::Scalar(string(value)))),
        )
    };
    assert_eq!(
        engine::run(&project, &source(None)).expect("guarded rows run"),
        group([field(
            "Out",
            Instance::Repeated(vec![
                row(["first", "one [em] end", "one [em] end"]),
                row(["second", "skipped", "skipped"]),
            ]),
        )])
    );
    // Framed mixed-content failures keep the interpreter's frame-relative display.
    for (fail, display) in [("Note", "Rows/Note"), ("FramedNote", "Note")] {
        let result = engine::run(&project, &source(Some(fail)));
        assert!(
            matches!(&result, Err(engine::EngineError::MissingSourceField(path)) if path == display),
            "{fail}: {result:?}"
        );
    }

    let program = codegen::lower(&project).expect("mixed-content project lowers");
    let artifacts = codegen_csharp::emit(&program).expect("mixed-content project emits");
    let directory = TempDirectory::new();
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent directory is created");
        }
        std::fs::write(path, &file.contents).expect("artifact is written");
    }
    let harness = directory.path().join("Harness");
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
    std::fs::write(harness.join("Program.cs"), HARNESS).expect("harness source is written");

    let build = Command::new("dotnet")
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .dotnet_temp(directory.path())
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
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .dotnet_temp(directory.path())
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
        "generated owned mixed content passed"
    );
}

const HARNESS: &str = r##"using Ferrule.Generated;
using Ferrule.Runtime;

const string Mixed = "\u001fferrule-xml-mixed-content";

FerruleGroup Text(string name, string value) => new([
    new("NodeName", new FerruleScalar(FerruleValue.FromString(name))),
    new("#text", new FerruleScalar(FerruleValue.FromString(value))),
]);

FerruleGroup Source(string? fail) => new([
    new("Fail", new FerruleScalar(fail is null ? FerruleValue.Null : FerruleValue.FromString(fail))),
    new("Rows", new FerruleRepeated([
        new FerruleGroup([
            new("Name", new FerruleScalar(FerruleValue.FromString("first"))),
            new("Note", new FerruleGroup([
                new(Mixed, new FerruleRepeated([Text("", "one "), Text("Em", "x"), Text("", " end")])),
            ])),
        ]),
        new FerruleGroup([new("Name", new FerruleScalar(FerruleValue.FromString("second")))]),
    ])),
]);

var rows = ((FerruleRepeated)Field((FerruleGroup)GeneratedMapping.Execute(Source(null)), "Out")).Items;
string[][] expected = [
    ["first", "one [em] end", "one [em] end"],
    ["second", "skipped", "skipped"],
];
string[] names = ["Name", "Note", "FramedNote"];
Equal(expected.Length, rows.Count);
for (var index = 0; index < rows.Count; index++)
{
    for (var column = 0; column < names.Length; column++)
    {
        Equal(expected[index][column], ((FerruleScalar)Field((FerruleGroup)rows[index], names[column])).Value.StringValue);
    }
}
foreach (var fail in new[] { "Note", "FramedNote" })
{
    try
    {
        _ = GeneratedMapping.Execute(Source(fail));
        throw new InvalidOperationException($"{fail} borrowed another row's mixed content.");
    }
    catch (FerruleRuntimeException error) when (
        error.Error == FerruleRuntimeError.MissingSourceField &&
        error.Message == "Structured source 'Rows/Note' does not exist in the active scope context.")
    {
    }
}
Console.WriteLine("generated owned mixed content passed");

static FerruleInstance Field(FerruleGroup group, string name) =>
    group.Fields.Single(field => field.Name == name).Value;

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
    }
}
"##;

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
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_codegen_csharp_xml_mixed_owned_absence_{}_{}",
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
