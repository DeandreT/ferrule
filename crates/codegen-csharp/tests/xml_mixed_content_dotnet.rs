use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, Program, ScalarFunction, TargetConstruction, TargetScope,
    XmlMixedContentElement, XmlMixedContentReplacement,
};
use ir::{ScalarType, SchemaNode, Value};

#[test]
fn generated_mapping_preserves_order_and_frames_each_replaced_occurrence() {
    let stdout = run_generated(&fixture(), HARNESS);
    assert_eq!(stdout, "generated mixed content passed");
}

fn run_generated(program: &Program, harness: &str) -> String {
    let artifacts = codegen_csharp::emit(program).expect("mixed-content fixture emits");
    let directory = TempDirectory::new();
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent directory is created");
        }
        std::fs::write(path, &file.contents).expect("artifact is written");
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
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

fn fixture() -> Program {
    let content = SchemaNode::group(
        "Content",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::group("Em", vec![SchemaNode::scalar("Value", ScalarType::String)])
                .repeating(),
        ],
    );
    Program {
        source: SchemaNode::group("Source", vec![content]),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Position {
                    collection: vec!["Content".into(), "Em".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const {
                    value: Value::String(":".into()),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![1, 3, 2],
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::XmlMixedContent {
                    frame: None,
                    path: vec!["Content".into()],
                    replacements: vec![XmlMixedContentReplacement {
                        element: "Em".into(),
                        collection: vec!["Content".into(), "Em".into()],
                        expression: 4,
                    }],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Text".into(),
                expression: 5,
                target_type: ScalarType::String,
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn target_fixture() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::scalar("Em", ScalarType::String).repeating(),
                SchemaNode::scalar("Strong", ScalarType::String).repeating(),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::scalar("Styled", ScalarType::String).repeating(),
            ],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::Const {
                    value: Value::String("first".into()),
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::String("second".into()),
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::XmlMixedContent {
                elements: vec![
                    XmlMixedContentElement {
                        source: "Em".into(),
                        target: "Styled".into(),
                    },
                    XmlMixedContentElement {
                        source: "Strong".into(),
                        target: "Styled".into(),
                    },
                ],
            },
            bindings: vec![
                Binding {
                    target_field: "Styled".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: true,
                },
                Binding {
                    target_field: "Styled".into(),
                    expression: 2,
                    target_type: ScalarType::String,
                    repeating: true,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_mapping_preserves_constructed_target_mixed_content_order() {
    let stdout = run_generated(&target_fixture(), TARGET_HARNESS);
    assert_eq!(stdout, "generated target mixed content passed");
}

fn write_harness(root: &Path, harness: &str) {
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
    std::fs::write(directory.join("Program.cs"), harness).expect("harness source is written");
}

const HARNESS: &str = r##"using Ferrule.Generated;
using Ferrule.Runtime;

const string Mixed = "\u001fferrule-xml-mixed-content";
const string MixedValue = "\u001fferrule-xml-mixed-value";

FerruleGroup Text(string value) => new([
    new("NodeName", new FerruleScalar(FerruleValue.FromString(""))),
    new("#text", new FerruleScalar(FerruleValue.FromString(value))),
]);

FerruleGroup Element(string name, string sourceText, string value) => new([
    new("NodeName", new FerruleScalar(FerruleValue.FromString(name))),
    new("#text", new FerruleScalar(FerruleValue.FromString(sourceText))),
    new(MixedValue, new FerruleGroup([
        new("Value", new FerruleScalar(FerruleValue.FromString(value))),
    ])),
]);

var source = new FerruleGroup([
    new("Content", new FerruleGroup([
        new(Mixed, new FerruleRepeated([
            Text("Hello "),
            Element("Em", "old", "world"),
            Text(" and "),
            Element("Em", "old", "again"),
            Element("Strong", "!", "unused"),
        ])),
    ])),
]);
var output = (FerruleGroup)GeneratedMapping.Execute(source);
Equal("Hello world:1 and again:2!", Value(output, "Text").StringValue);

var fallback = new FerruleGroup([
    new("Content", new FerruleGroup([
        new("#text", new FerruleScalar(FerruleValue.FromString("plain"))),
    ])),
]);
output = (FerruleGroup)GeneratedMapping.Execute(fallback);
Equal("plain", Value(output, "Text").StringValue);
Console.WriteLine("generated mixed content passed");

static FerruleValue Value(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
    }
}
"##;

const TARGET_HARNESS: &str = r##"using Ferrule.Generated;
using Ferrule.Runtime;

const string Mixed = "\u001fferrule-xml-mixed-content";

FerruleGroup Content(string name, string text) => new([
    new("NodeName", new FerruleScalar(FerruleValue.FromString(name))),
    new("#text", new FerruleScalar(FerruleValue.FromString(text))),
]);

var source = new FerruleGroup([
    new(Mixed, new FerruleRepeated([
        Content("", "before "),
        Content("Em", "old"),
        Content("Strong", "old"),
        Content("Code", "drop"),
        Content("", " after"),
    ])),
]);
var output = (FerruleGroup)GeneratedMapping.Execute(source);
var ordered = ((FerruleRepeated)Field(output, Mixed)).Items;
Equal(4, ordered.Count);
Equal("", Text(ordered[0], "NodeName"));
Equal("Styled", Text(ordered[1], "NodeName"));
Equal("Styled", Text(ordered[2], "NodeName"));
Equal("", Text(ordered[3], "NodeName"));
Equal("before ", Text(ordered[0], "#text"));
Equal("first", Text(ordered[1], "#text"));
Equal("second", Text(ordered[2], "#text"));
Equal(" after", Text(ordered[3], "#text"));
Console.WriteLine("generated target mixed content passed");

static FerruleInstance Field(FerruleGroup group, string name) =>
    group.Fields.Single(field => field.Name == name).Value;

static string Text(FerruleInstance instance, string name) =>
    ((FerruleScalar)Field((FerruleGroup)instance, name)).Value.StringValue;

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
            "ferrule_codegen_csharp_xml_mixed_content_{}_{}",
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
