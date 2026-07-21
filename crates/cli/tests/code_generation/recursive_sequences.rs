use super::*;
use mapping::{ScopeConstruction, SequenceExpr};

fn sequence(item: u32) -> SequenceExpr {
    SequenceExpr::RecursiveCollect {
        collection: Vec::new(),
        children: vec!["directory".into()],
        descent_value: vec!["name".into()],
        values: vec!["file".into()],
        value: vec!["name".into()],
        prefix: 1,
        separator: 2,
        item,
    }
}

fn recursive_project() -> Project {
    Project {
        source: SchemaNode::group(
            "directory",
            vec![
                string("name"),
                SchemaNode::group("file", vec![string("name")]).repeating(),
                SchemaNode::recursive_group("directory", "directory").repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Files",
            vec![string("File").repeating(), string("Picked"), bool_("Found")],
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
                    1,
                    Node::Const {
                        value: Value::String(String::new()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("\\".into()),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    6,
                    Node::SequenceItemAt {
                        sequence: sequence(4),
                        index: 5,
                    },
                ),
                (
                    7,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::String("\\root\\child\\nested.txt".into()),
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![7, 8],
                    },
                ),
                (
                    10,
                    Node::SequenceExists {
                        sequence: sequence(7),
                        predicate: 9,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Picked".into(),
                    node: 6,
                },
                Binding {
                    target_field: "Found".into(),
                    node: 10,
                },
            ],
            children: vec![Scope {
                target_field: "File".into(),
                iteration: ScopeIteration::Sequence(sequence(3)),
                construction: ScopeConstruction::Scalar { value: 3 },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn directory(name: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    Instance::Group(vec![
        ("name".into(), Instance::Scalar(Value::String(name.into()))),
        (
            "file".into(),
            Instance::Repeated(
                files
                    .iter()
                    .map(|file| {
                        Instance::Group(vec![(
                            "name".into(),
                            Instance::Scalar(Value::String((*file).into())),
                        )])
                    })
                    .collect(),
            ),
        ),
        ("directory".into(), Instance::Repeated(children)),
    ])
}

fn recursive_source() -> Instance {
    directory(
        "root",
        &["top.txt", "second.txt"],
        vec![directory("child", &["nested.txt"], Vec::new())],
    )
}

fn recursive_expected() -> Instance {
    Instance::Group(vec![
        (
            "Picked".into(),
            Instance::Scalar(Value::String("\\root\\second.txt".into())),
        ),
        ("Found".into(), Instance::Scalar(Value::Bool(true))),
        (
            "File".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("\\root\\top.txt".into())),
                Instance::Scalar(Value::String("\\root\\second.txt".into())),
                Instance::Scalar(Value::String("\\root\\child\\nested.txt".into())),
            ]),
        ),
    ])
}

fn recursive_deep_source() -> Instance {
    let mut source = directory("leaf", &[], Vec::new());
    for depth in 0..256 {
        source = directory(&format!("level-{depth}"), &[], vec![source]);
    }
    source
}

#[test]
fn recursive_sequences_match_engine_and_generated_backends() -> TestResult<()> {
    let project = recursive_project();
    assert_eq!(
        engine::run(&project, &recursive_source())?,
        recursive_expected()
    );
    assert_eq!(
        engine::run(&project, &recursive_deep_source()),
        Err(engine::EngineError::RecursiveSequenceDepth { limit: 256 })
    );

    let directory = TempDir::new("recursive_sequences")?;
    let project_path = directory.0.join("recursive-project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

    let rust_output = directory.0.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    let rust_source = std::fs::read_to_string(rust_output.join("src/lib.rs"))?;
    let rust_prefix = rust_source
        .find("recursive_sequence_parameter(expression_1(context)?)")
        .ok_or("generated Rust recursive prefix")?;
    let rust_separator = rust_source
        .find("recursive_sequence_parameter(expression_2(context)?)")
        .ok_or("generated Rust recursive separator")?;
    let rust_collect = rust_source
        .find("recursive_collect(")
        .ok_or("generated Rust recursive collector")?;
    assert!(rust_prefix < rust_separator && rust_separator < rust_collect);
    assert!(rust_source.contains("let output = scalar(expression_3(&item_context)?);"));
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/recursive_sequences_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust recursive sequences failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let csharp_source = std::fs::read_to_string(csharp_output.join("GeneratedMapping.cs"))?;
    let csharp_prefix = csharp_source
        .find("RecursiveCollectArgumentText(Node_1(context))")
        .ok_or("generated C# recursive prefix")?;
    let csharp_separator = csharp_source
        .find("RecursiveCollectArgumentText(Node_2(context))")
        .ok_or("generated C# recursive separator")?;
    let csharp_collect = csharp_source
        .find("FerruleSequences.RecursiveCollect(")
        .ok_or("generated C# recursive collector")?;
    assert!(csharp_prefix < csharp_separator && csharp_separator < csharp_collect);
    assert!(
        csharp_source
            .contains("return new global::Ferrule.Runtime.FerruleScalar(Node_3(context));")
    );
    let harness = csharp_output.join("Harness");
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
        include_str!("fixtures/recursive_sequences_csharp_harness.cs.txt"),
    )?;
    let csharp = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .output()?;
    assert!(
        csharp.status.success(),
        "generated C# recursive sequences failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
