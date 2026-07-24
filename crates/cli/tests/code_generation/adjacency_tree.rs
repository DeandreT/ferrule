use super::*;
use mapping::{AdjacencyTreePlan, ScopeConstruction};

const ROOT: u32 = 7;

fn adjacency_project() -> Project {
    let Some(plan) = AdjacencyTreePlan::new(
        vec!["Rows".into()],
        vec!["Key".into()],
        vec!["Parent".into()],
        "name".into(),
        "children".into(),
        Some(ROOT),
    ) else {
        panic!("valid adjacency-tree plan");
    };
    Project {
        source: SchemaNode::group(
            "Types",
            vec![
                string("Root"),
                SchemaNode::group("Rows", vec![string("Key"), string("Parent")]).repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Type",
            vec![
                string("name"),
                SchemaNode::recursive_group("children", "Type").repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                ROOT,
                Node::SourceField {
                    path: vec!["Root".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            construction: ScopeConstruction::AdjacencyTree { plan },
            ..Scope::default()
        },
    }
}

fn adjacency_source(root: Value, rows: &[(&str, Option<&str>)]) -> Instance {
    Instance::Group(vec![
        ("Root".into(), Instance::Scalar(root)),
        (
            "Rows".into(),
            Instance::Repeated(
                rows.iter()
                    .map(|(key, parent)| {
                        Instance::Group(vec![
                            ("Key".into(), Instance::Scalar(Value::String((*key).into()))),
                            (
                                "Parent".into(),
                                Instance::Scalar(
                                    parent
                                        .map(|parent| Value::String(parent.into()))
                                        .unwrap_or(Value::Null),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn adjacency_expected() -> Instance {
    Instance::Group(vec![
        (
            "name".into(),
            Instance::Scalar(Value::String("Root".into())),
        ),
        (
            "children".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    (
                        "name".into(),
                        Instance::Scalar(Value::String("Beta".into())),
                    ),
                    (
                        "children".into(),
                        Instance::Repeated(vec![Instance::Group(vec![
                            (
                                "name".into(),
                                Instance::Scalar(Value::String("Leaf".into())),
                            ),
                            ("children".into(), Instance::Repeated(Vec::new())),
                        ])]),
                    ),
                ]),
                Instance::Group(vec![
                    (
                        "name".into(),
                        Instance::Scalar(Value::String("Alpha".into())),
                    ),
                    ("children".into(), Instance::Repeated(Vec::new())),
                ]),
            ]),
        ),
    ])
}

#[test]
fn adjacency_tree_matches_engine_and_generated_backends() -> TestResult<()> {
    let project = adjacency_project();
    let source = adjacency_source(
        Value::Null,
        &[
            ("Root", None),
            ("Beta", Some("Root")),
            ("Alpha", Some("Root")),
            ("Leaf", Some("Beta")),
            ("Unreachable", Some("Unreachable")),
        ],
    );
    assert_eq!(engine::run(&project, &source)?, adjacency_expected());
    assert_eq!(
        engine::run(
            &project,
            &adjacency_source(Value::String("Loop".into()), &[("Loop", Some("Loop"))]),
        ),
        Err(engine::EngineError::AdjacencyCycle("Loop".into()))
    );
    assert_eq!(
        engine::run(
            &project,
            &adjacency_source(Value::Int(1), &[("Duplicate", None), ("Duplicate", None)],),
        ),
        Err(engine::EngineError::DuplicateAdjacencyKey(
            "Duplicate".into()
        ))
    );
    assert_eq!(
        engine::run(
            &project,
            &adjacency_source(Value::Int(1), &[("Root", None)]),
        ),
        Err(engine::EngineError::InvalidAdjacencyRoot { found: "int" })
    );

    let directory = TempDir::new("adjacency_tree")?;
    let project_path = directory.0.join("adjacency-tree.json");
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
    assert!(rust_source.contains("let output = adjacency_tree("));
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/adjacency_tree_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust adjacency tree failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let csharp_source = std::fs::read_to_string(csharp_output.join("GeneratedMapping.cs"))?;
    assert!(csharp_source.contains("FerruleAdjacencyTree.Build("));
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
        include_str!("fixtures/adjacency_tree_csharp_harness.cs.txt"),
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
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "generated C# adjacency tree failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
