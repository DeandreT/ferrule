use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{GenerateOutcome, GenerateTarget, generate_project};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

#[path = "code_generation/aggregates.rs"]
mod aggregates;
#[path = "code_generation/collection_find.rs"]
mod collection_find;
#[path = "code_generation/copy_current_source.rs"]
mod copy_current_source;
#[path = "code_generation/extra_targets.rs"]
mod extra_targets;
#[path = "code_generation/failure_rules.rs"]
mod failure_rules;
#[path = "code_generation/generated_sequences.rs"]
mod generated_sequences;
#[path = "code_generation/grouping.rs"]
mod grouping;
#[path = "code_generation/iteration_controls.rs"]
mod iteration_controls;
#[path = "code_generation/iteration_metadata.rs"]
mod iteration_metadata;
#[path = "code_generation/joins.rs"]
mod joins;
#[path = "code_generation/lookups.rs"]
mod lookups;
#[path = "code_generation/recursive_sequences.rs"]
mod recursive_sequences;
#[path = "code_generation/runtime_values.rs"]
mod runtime_values;
#[path = "code_generation/scalar_algorithms.rs"]
mod scalar_algorithms;
#[path = "code_generation/scalar_functions.rs"]
mod scalar_functions;
#[path = "code_generation/sequence_reducers.rs"]
mod sequence_reducers;
#[path = "code_generation/static_sources.rs"]
mod static_sources;
#[path = "code_generation/value_maps.rs"]
mod value_maps;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
type ArtifactFiles = Vec<(String, Vec<u8>)>;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_codegen_{name}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

fn bool_(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Bool)
}

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Name"),
                int("Score"),
                bool_("Enabled"),
                string("Danger"),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                string("Copied"),
                string("Fixed"),
                int("Adjusted"),
                string("Bucket"),
                bool_("Enabled"),
                string("Lazy"),
                SchemaNode::group("Details", vec![string("NestedCopied")]),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    Node::Const {
                        value: Value::String("fixed".into()),
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: None,
                    },
                ),
                (
                    30,
                    Node::SourceField {
                        path: vec!["Score".into()],
                        frame: None,
                    },
                ),
                (
                    40,
                    Node::Const {
                        value: Value::Int(5),
                    },
                ),
                (
                    50,
                    Node::Call {
                        function: "add".into(),
                        args: vec![30, 40],
                    },
                ),
                (
                    60,
                    Node::Const {
                        value: Value::Int(10),
                    },
                ),
                (
                    70,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![50, 60],
                    },
                ),
                (
                    80,
                    Node::Const {
                        value: Value::String("large".into()),
                    },
                ),
                (
                    90,
                    Node::Const {
                        value: Value::String("small".into()),
                    },
                ),
                (
                    100,
                    Node::If {
                        condition: 70,
                        then: 80,
                        else_: 90,
                    },
                ),
                (
                    110,
                    Node::SourceField {
                        path: vec!["Enabled".into()],
                        frame: None,
                    },
                ),
                (
                    120,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
                (
                    130,
                    Node::Call {
                        function: "or".into(),
                        args: vec![110, 120],
                    },
                ),
                (
                    140,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    150,
                    Node::SourceField {
                        path: vec!["Danger".into()],
                        frame: None,
                    },
                ),
                (
                    160,
                    Node::If {
                        condition: 140,
                        then: 10,
                        else_: 150,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Copied".into(),
                    node: 20,
                },
                Binding {
                    target_field: "Fixed".into(),
                    node: 10,
                },
                Binding {
                    target_field: "Adjusted".into(),
                    node: 50,
                },
                Binding {
                    target_field: "Bucket".into(),
                    node: 100,
                },
                Binding {
                    target_field: "Enabled".into(),
                    node: 130,
                },
                Binding {
                    target_field: "Lazy".into(),
                    node: 160,
                },
            ],
            children: vec![Scope {
                target_field: "Details".into(),
                bindings: vec![Binding {
                    target_field: "NestedCopied".into(),
                    node: 20,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn write_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&project())?)?;
    Ok(path)
}

fn write_nested_iteration_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("nested-iteration-project.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&nested_iteration_project())?,
    )?;
    Ok(path)
}

fn source_instance() -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String("Ada".into()))),
        ("Score".into(), Instance::Scalar(Value::Int(8))),
        ("Enabled".into(), Instance::Scalar(Value::Bool(true))),
    ])
}

fn expected_instance() -> Instance {
    Instance::Group(vec![
        (
            "Copied".into(),
            Instance::Scalar(Value::String("Ada".into())),
        ),
        (
            "Fixed".into(),
            Instance::Scalar(Value::String("fixed".into())),
        ),
        ("Adjusted".into(), Instance::Scalar(Value::Int(13))),
        (
            "Bucket".into(),
            Instance::Scalar(Value::String("large".into())),
        ),
        ("Enabled".into(), Instance::Scalar(Value::Bool(true))),
        (
            "Lazy".into(),
            Instance::Scalar(Value::String("fixed".into())),
        ),
        (
            "Details".into(),
            Instance::Group(vec![(
                "NestedCopied".into(),
                Instance::Scalar(Value::String("Ada".into())),
            )]),
        ),
    ])
}

fn nested_iteration_project() -> Project {
    let source_line = SchemaNode::group("Lines", vec![string("Sku"), int("Quantity")]).repeating();
    let source_order =
        SchemaNode::group("Orders", vec![string("OrderId"), source_line]).repeating();
    let target_line = SchemaNode::group(
        "Lines",
        vec![
            string("Sku"),
            string("OrderId"),
            string("Batch"),
            string("DefaultLabel"),
            int("Adjusted"),
        ],
    )
    .repeating();
    let target_order =
        SchemaNode::group("Orders", vec![string("OrderId"), target_line]).repeating();

    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Batch"),
                int("Bonus"),
                SchemaNode::group("Defaults", vec![string("Label")]).repeating(),
                source_order,
            ],
        ),
        target: SchemaNode::group("Target", vec![target_order]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    Node::SourceField {
                        path: vec!["OrderId".into()],
                        frame: None,
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        path: vec!["Sku".into()],
                        frame: None,
                    },
                ),
                (
                    30,
                    Node::SourceField {
                        path: vec!["Batch".into()],
                        frame: None,
                    },
                ),
                (
                    40,
                    Node::SourceField {
                        path: vec!["Defaults".into(), "Label".into()],
                        frame: None,
                    },
                ),
                (
                    50,
                    Node::SourceField {
                        path: vec!["Quantity".into()],
                        frame: None,
                    },
                ),
                (
                    60,
                    Node::SourceField {
                        path: vec!["Bonus".into()],
                        frame: None,
                    },
                ),
                (
                    70,
                    Node::Call {
                        function: "add".into(),
                        args: vec![50, 60],
                    },
                ),
                (
                    80,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    90,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![50, 80],
                    },
                ),
                (
                    100,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    110,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    120,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![100, 110],
                    },
                ),
                (
                    130,
                    Node::If {
                        condition: 90,
                        then: 70,
                        else_: 120,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Orders".into(),
                iteration: ScopeIteration::Source(vec!["Orders".into()]),
                bindings: vec![Binding {
                    target_field: "OrderId".into(),
                    node: 10,
                }],
                children: vec![Scope {
                    target_field: "Lines".into(),
                    iteration: ScopeIteration::Source(vec!["Lines".into()]),
                    bindings: vec![
                        Binding {
                            target_field: "Sku".into(),
                            node: 20,
                        },
                        Binding {
                            target_field: "OrderId".into(),
                            node: 10,
                        },
                        Binding {
                            target_field: "Batch".into(),
                            node: 30,
                        },
                        Binding {
                            target_field: "DefaultLabel".into(),
                            node: 40,
                        },
                        Binding {
                            target_field: "Adjusted".into(),
                            node: 130,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn nested_source_instance() -> Instance {
    let line = |sku: &str, quantity: i64| {
        Instance::Group(vec![
            ("Sku".into(), Instance::Scalar(Value::String(sku.into()))),
            ("Quantity".into(), Instance::Scalar(Value::Int(quantity))),
        ])
    };
    let order = |id: &str, lines: Vec<Instance>| {
        Instance::Group(vec![
            ("OrderId".into(), Instance::Scalar(Value::String(id.into()))),
            ("Lines".into(), Instance::Repeated(lines)),
        ])
    };
    let default = |label: &str| {
        Instance::Group(vec![(
            "Label".into(),
            Instance::Scalar(Value::String(label.into())),
        )])
    };

    Instance::Group(vec![
        (
            "Batch".into(),
            Instance::Scalar(Value::String("run-42".into())),
        ),
        ("Bonus".into(), Instance::Scalar(Value::Int(2))),
        (
            "Defaults".into(),
            Instance::Repeated(vec![default("primary"), default("ignored")]),
        ),
        (
            "Orders".into(),
            Instance::Repeated(vec![
                order("A", vec![line("red", 3), line("blue", 1)]),
                order("B", vec![line("green", 2)]),
            ]),
        ),
    ])
}

fn nested_expected_instance() -> Instance {
    let line = |sku: &str, order_id: &str, adjusted: i64| {
        Instance::Group(vec![
            ("Sku".into(), Instance::Scalar(Value::String(sku.into()))),
            (
                "OrderId".into(),
                Instance::Scalar(Value::String(order_id.into())),
            ),
            (
                "Batch".into(),
                Instance::Scalar(Value::String("run-42".into())),
            ),
            (
                "DefaultLabel".into(),
                Instance::Scalar(Value::String("primary".into())),
            ),
            ("Adjusted".into(), Instance::Scalar(Value::Int(adjusted))),
        ])
    };
    let order = |id: &str, lines: Vec<Instance>| {
        Instance::Group(vec![
            ("OrderId".into(), Instance::Scalar(Value::String(id.into()))),
            ("Lines".into(), Instance::Repeated(lines)),
        ])
    };

    Instance::Group(vec![(
        "Orders".into(),
        Instance::Repeated(vec![
            order("A", vec![line("red", "A", 5), line("blue", "A", 3)]),
            order("B", vec![line("green", "B", 4)]),
        ]),
    )])
}

fn artifact_files(root: &Path) -> TestResult<ArtifactFiles> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<(String, Vec<u8>)>) -> TestResult<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files)?;
            } else {
                let relative = path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push((relative, std::fs::read(path)?));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

#[test]
fn csharp_generation_has_a_deterministic_manifest() -> TestResult<()> {
    let directory = TempDir::new("csharp_manifest")?;
    let project_path = write_project(&directory.0)?;
    let first = directory.0.join("first");
    let second = directory.0.join("second");

    let outcome = generate_project(&project_path, &first, GenerateTarget::CSharp)?;
    let repeated = generate_project(&project_path, &second, GenerateTarget::CSharp)?;
    let first_files = artifact_files(&first)?;
    let second_files = artifact_files(&second)?;
    let manifest = first_files
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        outcome,
        GenerateOutcome {
            output_directory: first,
            files_written: 22,
        }
    );
    assert_eq!(repeated.files_written, outcome.files_written);
    assert_eq!(
        manifest,
        vec![
            "Ferrule.Generated.csproj",
            "GeneratedMapping.cs",
            "GeneratedTargetBuilder.cs",
            "Runtime/FerruleAggregates.cs",
            "Runtime/FerruleExecutionContext.cs",
            "Runtime/FerruleFailures.cs",
            "Runtime/FerruleFunctions.DateTime.cs",
            "Runtime/FerruleFunctions.DateTimePictures.cs",
            "Runtime/FerruleFunctions.FormatNumber.cs",
            "Runtime/FerruleFunctions.Numeric.cs",
            "Runtime/FerruleFunctions.Strings.cs",
            "Runtime/FerruleFunctions.cs",
            "Runtime/FerruleGrouping.cs",
            "Runtime/FerruleInstance.cs",
            "Runtime/FerruleJoins.cs",
            "Runtime/FerruleRuntimeException.cs",
            "Runtime/FerruleSequences.cs",
            "Runtime/FerruleValue.cs",
            "Runtime/FerruleValueMaps.cs",
            "Runtime/ScalarPathResolver.cs",
            "Runtime/ScopeContext.CollectionFind.cs",
            "Runtime/ScopeContext.cs",
        ]
    );
    assert_eq!(first_files, second_files);
    Ok(())
}

#[test]
fn csharp_generation_preserves_an_existing_destination() -> TestResult<()> {
    let directory = TempDir::new("csharp_existing")?;
    let project_path = write_project(&directory.0)?;
    let output = directory.0.join("generated");
    std::fs::create_dir(&output)?;
    std::fs::write(output.join("keep.txt"), "do not replace")?;
    let before = artifact_files(&output)?;

    let error = generate_project(&project_path, &output, GenerateTarget::CSharp)
        .expect_err("an existing generation destination must be rejected");

    assert!(
        error.to_string().contains("already exists"),
        "unexpected error: {error:#}"
    );
    assert_eq!(artifact_files(&output)?, before);
    Ok(())
}

#[cfg(unix)]
#[test]
fn rust_generation_rejects_a_non_utf8_runtime_path() -> TestResult<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = TempDir::new("rust_non_utf8_runtime")?;
    let project_path = write_project(&directory.0)?;
    let runtime = directory
        .0
        .join(OsString::from_vec(b"runtime-\xff".to_vec()));
    std::fs::create_dir(&runtime)?;
    let output = directory.0.join("generated");

    let error = generate_project(
        &project_path,
        &output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )
    .expect_err("a non-UTF-8 Cargo dependency path must be rejected");

    assert!(error.to_string().contains("must be valid UTF-8"));
    assert!(!output.exists());
    Ok(())
}

#[test]
fn unsupported_mapping_creates_no_output_directory() -> TestResult<()> {
    let directory = TempDir::new("unsupported")?;
    let mut unsupported = project();
    unsupported.graph.nodes.insert(
        30,
        Node::Call {
            function: "edifact_to_datetime".into(),
            args: vec![10, 20],
        },
    );
    unsupported.root.bindings[0].node = 30;
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&unsupported)?)?;
    let output = directory.0.join("generated");

    let error = generate_project(&project_path, &output, GenerateTarget::CSharp)
        .expect_err("unsupported nodes must fail capability analysis");

    assert!(error.to_string().contains("graph node 30"));
    assert!(error.to_string().contains("edifact_to_datetime"));
    assert!(!output.exists());
    assert!(
        std::fs::read_dir(&directory.0)?
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .contains("ferrule-stage"))
    );
    Ok(())
}

#[test]
fn generated_csharp_project_matches_the_engine() -> TestResult<()> {
    let directory = TempDir::new("csharp_execute")?;
    let mapping = project();
    assert_eq!(
        engine::run(&mapping, &source_instance())?,
        expected_instance()
    );
    let project_path = write_project(&directory.0)?;
    let output = directory.0.join("generated");
    generate_project(&project_path, &output, GenerateTarget::CSharp)?;

    let harness = output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = new FerruleGroup(new FerruleField[]
{
    new("Name", new FerruleScalar(FerruleValue.FromString("Ada"))),
    new("Score", new FerruleScalar(FerruleValue.FromInt64(8))),
    new("Enabled", new FerruleScalar(FerruleValue.FromBoolean(true))),
});
var output = (FerruleGroup)GeneratedMapping.Execute(source);
Assert(output.Fields.Select(field => field.Name).SequenceEqual(
    new[] { "Copied", "Fixed", "Adjusted", "Bucket", "Enabled", "Lazy", "Details" }));
Assert(((FerruleScalar)output.Fields[0].Value).Value == FerruleValue.FromString("Ada"));
Assert(((FerruleScalar)output.Fields[1].Value).Value == FerruleValue.FromString("fixed"));
Assert(((FerruleScalar)output.Fields[2].Value).Value == FerruleValue.FromInt64(13));
Assert(((FerruleScalar)output.Fields[3].Value).Value == FerruleValue.FromString("large"));
Assert(((FerruleScalar)output.Fields[4].Value).Value == FerruleValue.FromBoolean(true));
Assert(((FerruleScalar)output.Fields[5].Value).Value == FerruleValue.FromString("fixed"));
var details = (FerruleGroup)output.Fields[6].Value;
Assert(details.Fields.Count == 1 && details.Fields[0].Name == "NestedCopied");
Assert(((FerruleScalar)details.Fields[0].Value).Value == FerruleValue.FromString("Ada"));

static void Assert(bool condition)
{
    if (!condition)
    {
        throw new InvalidOperationException("generated C# output differs from the engine");
    }
}
"#,
    )?;
    let command = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&output)
        .output()?;
    assert!(
        command.status.success(),
        "generated C# project failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}

#[test]
fn generated_rust_project_executes_the_mapping() -> TestResult<()> {
    let directory = TempDir::new("rust_execute")?;
    let mapping = project();
    assert_eq!(
        engine::run(&mapping, &source_instance())?,
        expected_instance()
    );
    let project_path = write_project(&directory.0)?;
    let output = directory.0.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");

    let outcome = generate_project(
        &project_path,
        &output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;

    assert_eq!(
        outcome,
        GenerateOutcome {
            output_directory: output.clone(),
            files_written: 2,
        }
    );
    assert_eq!(
        artifact_files(&output)?
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>(),
        vec!["Cargo.toml", "src/lib.rs"]
    );

    std::fs::write(
        output.join("src/main.rs"),
        r#"use codegen_runtime::{Value, field, group, scalar};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = group([
        field("Name", scalar(Value::String("Ada".into()))),
        field("Score", scalar(Value::Int(8))),
        field("Enabled", scalar(Value::Bool(true))),
    ]);
    let actual = execute(&source)?;
    let expected = group([
        field("Copied", scalar(Value::String("Ada".into()))),
        field("Fixed", scalar(Value::String("fixed".into()))),
        field("Adjusted", scalar(Value::Int(13))),
        field("Bucket", scalar(Value::String("large".into()))),
        field("Enabled", scalar(Value::Bool(true))),
        field("Lazy", scalar(Value::String("fixed".into()))),
        field(
            "Details",
            group([field(
                "NestedCopied",
                scalar(Value::String("Ada".into())),
            )]),
        ),
    ]);
    assert_eq!(actual, expected);
    Ok(())
}
"#,
    )?;

    let command = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        command.status.success(),
        "generated Rust project failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}

#[test]
fn generated_csharp_nested_source_iteration_matches_engine() -> TestResult<()> {
    let directory = TempDir::new("csharp_nested_iteration")?;
    let mapping = nested_iteration_project();
    assert_eq!(
        engine::run(&mapping, &nested_source_instance())?,
        nested_expected_instance()
    );
    let project_path = write_nested_iteration_project(&directory.0)?;
    let output = directory.0.join("generated");
    generate_project(&project_path, &output, GenerateTarget::CSharp)?;

    let harness = output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = new FerruleGroup(new FerruleField[]
{
    new("Batch", Scalar(FerruleValue.FromString("run-42"))),
    new("Bonus", Scalar(FerruleValue.FromInt64(2))),
    new("Defaults", new FerruleRepeated(new FerruleInstance[]
    {
        Default("primary"),
        Default("ignored"),
    })),
    new("Orders", new FerruleRepeated(new FerruleInstance[]
    {
        SourceOrder("A", SourceLine("red", 3), SourceLine("blue", 1)),
        SourceOrder("B", SourceLine("green", 2)),
    })),
});

var output = (FerruleGroup)GeneratedMapping.Execute(source);
Assert(output.Fields.Select(field => field.Name).SequenceEqual(new[] { "Orders" }));
var orders = (FerruleRepeated)output.Fields[0].Value;
Assert(orders.Items.Count == 2);
AssertOrder((FerruleGroup)orders.Items[0], "A", 2);
AssertOrder((FerruleGroup)orders.Items[1], "B", 1);

var firstLines = (FerruleRepeated)((FerruleGroup)orders.Items[0]).Fields[1].Value;
AssertLine((FerruleGroup)firstLines.Items[0], "red", "A", 5);
AssertLine((FerruleGroup)firstLines.Items[1], "blue", "A", 3);
var secondLines = (FerruleRepeated)((FerruleGroup)orders.Items[1]).Fields[1].Value;
AssertLine((FerruleGroup)secondLines.Items[0], "green", "B", 4);

static FerruleScalar Scalar(FerruleValue value) => new(value);

static FerruleGroup Default(string label) =>
    new(new FerruleField[] { new("Label", Scalar(FerruleValue.FromString(label))) });

static FerruleGroup SourceLine(string sku, long quantity) =>
    new(new FerruleField[]
    {
        new("Sku", Scalar(FerruleValue.FromString(sku))),
        new("Quantity", Scalar(FerruleValue.FromInt64(quantity))),
    });

static FerruleGroup SourceOrder(string id, params FerruleGroup[] lines) =>
    new(new FerruleField[]
    {
        new("OrderId", Scalar(FerruleValue.FromString(id))),
        new("Lines", new FerruleRepeated(lines)),
    });

static void AssertOrder(FerruleGroup order, string id, int lineCount)
{
    Assert(order.Fields.Select(field => field.Name).SequenceEqual(new[] { "OrderId", "Lines" }));
    Assert(Value(order, 0) == FerruleValue.FromString(id));
    Assert(((FerruleRepeated)order.Fields[1].Value).Items.Count == lineCount);
}

static void AssertLine(FerruleGroup line, string sku, string orderId, long adjusted)
{
    Assert(line.Fields.Select(field => field.Name).SequenceEqual(
        new[] { "Sku", "OrderId", "Batch", "DefaultLabel", "Adjusted" }));
    Assert(Value(line, 0) == FerruleValue.FromString(sku));
    Assert(Value(line, 1) == FerruleValue.FromString(orderId));
    Assert(Value(line, 2) == FerruleValue.FromString("run-42"));
    Assert(Value(line, 3) == FerruleValue.FromString("primary"));
    Assert(Value(line, 4) == FerruleValue.FromInt64(adjusted));
}

static FerruleValue Value(FerruleGroup group, int field) =>
    ((FerruleScalar)group.Fields[field].Value).Value;

static void Assert(bool condition)
{
    if (!condition)
    {
        throw new InvalidOperationException("generated C# nested iteration differs from the engine");
    }
}
"#,
    )?;
    let command = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&output)
        .output()?;
    assert!(
        command.status.success(),
        "generated C# nested iteration failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}

#[test]
fn generated_rust_nested_source_iteration_matches_engine() -> TestResult<()> {
    let directory = TempDir::new("rust_nested_iteration")?;
    let mapping = nested_iteration_project();
    assert_eq!(
        engine::run(&mapping, &nested_source_instance())?,
        nested_expected_instance()
    );
    let project_path = write_nested_iteration_project(&directory.0)?;
    let output = directory.0.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;

    std::fs::write(
        output.join("src/main.rs"),
        r#"use codegen_runtime::{Instance, Value, field, group, repeated, scalar};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = group([
        field("Batch", scalar(Value::String("run-42".into()))),
        field("Bonus", scalar(Value::Int(2))),
        field("Defaults", repeated([default("primary"), default("ignored")])),
        field(
            "Orders",
            repeated([
                source_order("A", [source_line("red", 3), source_line("blue", 1)]),
                source_order("B", [source_line("green", 2)]),
            ]),
        ),
    ]);
    let actual = execute(&source)?;
    let expected = group([field(
        "Orders",
        repeated([
            target_order(
                "A",
                [target_line("red", "A", 5), target_line("blue", "A", 3)],
            ),
            target_order("B", [target_line("green", "B", 4)]),
        ]),
    )]);
    assert_eq!(actual, expected);
    Ok(())
}

fn default(label: &str) -> Instance {
    group([field("Label", scalar(Value::String(label.into())))])
}

fn source_line(sku: &str, quantity: i64) -> Instance {
    group([
        field("Sku", scalar(Value::String(sku.into()))),
        field("Quantity", scalar(Value::Int(quantity))),
    ])
}

fn source_order(id: &str, lines: impl IntoIterator<Item = Instance>) -> Instance {
    group([
        field("OrderId", scalar(Value::String(id.into()))),
        field("Lines", repeated(lines)),
    ])
}

fn target_line(sku: &str, order_id: &str, adjusted: i64) -> Instance {
    group([
        field("Sku", scalar(Value::String(sku.into()))),
        field("OrderId", scalar(Value::String(order_id.into()))),
        field("Batch", scalar(Value::String("run-42".into()))),
        field("DefaultLabel", scalar(Value::String("primary".into()))),
        field("Adjusted", scalar(Value::Int(adjusted))),
    ])
}

fn target_order(id: &str, lines: impl IntoIterator<Item = Instance>) -> Instance {
    group([
        field("OrderId", scalar(Value::String(id.into()))),
        field("Lines", repeated(lines)),
    ])
}
"#,
    )?;

    let command = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        command.status.success(),
        "generated Rust nested iteration failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}
