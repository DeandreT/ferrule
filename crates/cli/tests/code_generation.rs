use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{GenerateOutcome, GenerateTarget, generate_project};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

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
            files_written: 8,
        }
    );
    assert_eq!(repeated.files_written, outcome.files_written);
    assert_eq!(
        manifest,
        vec![
            "Ferrule.Generated.csproj",
            "GeneratedMapping.cs",
            "GeneratedTargetBuilder.cs",
            "Runtime/FerruleFunctions.cs",
            "Runtime/FerruleInstance.cs",
            "Runtime/FerruleRuntimeException.cs",
            "Runtime/FerruleValue.cs",
            "Runtime/ScalarPathResolver.cs",
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
            function: "concat".into(),
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
    assert!(error.to_string().contains("concat"));
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
