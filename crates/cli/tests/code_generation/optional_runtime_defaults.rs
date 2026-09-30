use super::*;

fn optional_project() -> Project {
    let inner = FunctionId::new(1);
    let outer = FunctionId::new(2);
    let definition = |name: &str, nodes, output| UserFunction {
        library: "tests".into(),
        name: name.into(),
        description: None,
        parameters: Vec::new(),
        output_name: "result".into(),
        output_type: ScalarType::String,
        body: Graph { nodes },
        output,
    };
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Target", vec![int("Direct"), string("Nested")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::from([
            (
                inner,
                definition(
                    "inner",
                    BTreeMap::from([
                        (
                            1,
                            Node::Const {
                                value: Value::String("inner-default".into()),
                            },
                        ),
                        (
                            2,
                            Node::RuntimeParameterDefault {
                                name: "nested".into(),
                                ty: ScalarType::String,
                                default: 1,
                                preview: Some("preview-inner".into()),
                            },
                        ),
                    ]),
                    2,
                ),
            ),
            (
                outer,
                definition(
                    "outer",
                    BTreeMap::from([(
                        3,
                        Node::UserFunctionCall {
                            function: inner,
                            args: Vec::new(),
                        },
                    )]),
                    3,
                ),
            ),
        ]),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::If {
                        condition: 6,
                        then: 7,
                        else_: 4,
                    },
                ),
                (
                    2,
                    Node::RuntimeParameterDefault {
                        name: "control".into(),
                        ty: ScalarType::Int,
                        default: 1,
                        preview: Some("not-an-integer".into()),
                    },
                ),
                (
                    3,
                    Node::UserFunctionCall {
                        function: outer,
                        args: Vec::new(),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("7".into()),
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
                (
                    6,
                    Node::RuntimeParameterDefault {
                        name: "explode_default".into(),
                        ty: ScalarType::Bool,
                        default: 5,
                        preview: Some("true".into()),
                    },
                ),
                (
                    7,
                    Node::RuntimeParameter {
                        name: "missing_default".into(),
                        ty: ScalarType::Int,
                        preview: Some("99".into()),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Direct".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Nested".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    }
}

#[test]
fn optional_runtime_defaults_match_interpreter_in_generated_rust_and_csharp() -> TestResult<()> {
    let project = optional_project();
    assert!(engine::validate(&project).is_empty());
    let source = Instance::Group(Vec::new());
    let fallback = Instance::Group(vec![
        ("Direct".into(), Instance::Scalar(Value::Int(7))),
        (
            "Nested".into(),
            Instance::Scalar(Value::String("inner-default".into())),
        ),
    ]);
    assert_eq!(engine::run(&project, &source)?, fallback);
    let mut supplied = engine::RuntimeParameters::new();
    supplied.insert("control", Value::String(" 42 ".into()))?;
    supplied.insert("nested", Value::String("host".into()))?;
    let context =
        engine::ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&supplied);
    assert_eq!(
        engine::run_with_context(&project, &source, &context)?,
        Instance::Group(vec![
            ("Direct".into(), Instance::Scalar(Value::Int(42))),
            (
                "Nested".into(),
                Instance::Scalar(Value::String("host".into()))
            ),
        ])
    );
    let mut lazy = engine::RuntimeParameters::new();
    lazy.insert("explode_default", Value::Bool(true))?;
    lazy.insert("control", Value::Int(9))?;
    let context =
        engine::ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&lazy);
    assert_eq!(
        engine::run_with_context(&project, &source, &context)?.field("Direct"),
        Some(&Instance::Scalar(Value::Int(9)))
    );
    let mut failing = engine::RuntimeParameters::new();
    failing.insert("explode_default", Value::Bool(true))?;
    let context =
        engine::ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&failing);
    assert_eq!(
        engine::run_with_context(&project, &source, &context),
        Err(engine::EngineError::MissingRuntimeParameter {
            node: 7,
            name: "missing_default".into(),
        })
    );

    let directory = TempDir::new("optional_runtime_defaults")?;
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;

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
        include_str!("fixtures/optional_runtime_defaults_rust.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .env("RUSTFLAGS", "-Dwarnings")
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust optional inputs failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/optional_runtime_defaults_csharp.cs.txt"),
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
        "generated C# optional inputs failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
