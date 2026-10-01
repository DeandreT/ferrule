use super::*;

fn disconnected_project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![bool_("CheckDirect")]),
        target: SchemaNode::group("Target", vec![bool_("Present"), string("Label")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (0, Node::Unconnected),
                (
                    1,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![0],
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("unselected".into()),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![3, 4],
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::String("fallback".into()),
                    },
                ),
                (
                    7,
                    Node::If {
                        condition: 1,
                        then: 5,
                        else_: 6,
                    },
                ),
                (
                    8,
                    Node::If {
                        condition: 0,
                        then: 2,
                        else_: 6,
                    },
                ),
                (
                    9,
                    Node::SourceField {
                        path: vec!["CheckDirect".into()],
                        frame: None,
                    },
                ),
                (
                    10,
                    Node::If {
                        condition: 9,
                        then: 8,
                        else_: 7,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Present".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Label".into(),
                    node: 10,
                },
            ],
            ..Scope::default()
        },
    }
}

fn source(check_direct: bool) -> Instance {
    Instance::Group(vec![(
        "CheckDirect".into(),
        Instance::Scalar(Value::Bool(check_direct)),
    )])
}

fn expected() -> Instance {
    Instance::Group(vec![
        ("Present".into(), Instance::Scalar(Value::Bool(false))),
        (
            "Label".into(),
            Instance::Scalar(Value::String("fallback".into())),
        ),
    ])
}

#[test]
fn saved_disconnected_input_matches_interpreter_in_generated_rust_and_csharp() -> TestResult<()> {
    let directory = TempDir::new("saved_disconnected_input")?;
    let project_path = directory.0.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&disconnected_project())?,
    )?;
    let project = mapping::project_file::decode_bytes(&std::fs::read(&project_path)?)?;
    assert!(matches!(
        project.graph.nodes.get(&0),
        Some(Node::Unconnected)
    ));
    assert!(engine::validate(&project).is_empty());
    assert_eq!(engine::run(&project, &source(false))?, expected());
    assert_eq!(
        engine::run(&project, &source(true)),
        Err(engine::EngineError::NotABool {
            node: 0,
            found: "null"
        }),
    );

    let rust_output = directory.0.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        rust_output.join("target-schema.json"),
        serde_json::to_vec(&project.target)?,
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/disconnected_inputs_rust.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .env("RUSTFLAGS", "-Dwarnings")
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust saved disconnected input failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
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
        include_str!("fixtures/disconnected_inputs_csharp.cs.txt"),
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
        "generated C# saved disconnected input failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
