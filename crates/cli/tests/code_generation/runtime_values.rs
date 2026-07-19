use super::*;

const ACTIVE: &str = "/maps/library.ferrule.json";
const MAIN: &str = "/maps/main.ferrule.json";
const CURRENT: &str = "2026-07-19T11:22:33.45-07:00";
const SKIPPED: &str = "missing-current-skipped";

fn runtime_project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![bool_("UseCurrent")]),
        target: SchemaNode::group(
            "Target",
            ["Active", "Main", "CurrentOne", "CurrentTwo", "Lazy"]
                .into_iter()
                .map(string)
                .collect(),
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
                    Node::SourceField {
                        path: vec!["UseCurrent".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::MappingFilePath,
                    },
                ),
                (
                    3,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::MainMappingFilePath,
                    },
                ),
                (
                    4,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::CurrentDateTime,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String(SKIPPED.into()),
                    },
                ),
                (
                    6,
                    Node::If {
                        condition: 1,
                        then: 4,
                        else_: 5,
                    },
                ),
                (
                    7,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::String("lazy".into()),
                    },
                ),
                (
                    9,
                    Node::If {
                        condition: 7,
                        then: 8,
                        else_: 4,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: [
                ("Active", 2),
                ("Main", 3),
                ("CurrentOne", 6),
                ("CurrentTwo", 6),
                ("Lazy", 9),
            ]
            .into_iter()
            .map(|(target_field, node)| Binding {
                target_field: target_field.into(),
                node,
            })
            .collect(),
            ..Scope::default()
        },
    }
}

fn source(use_current: bool) -> Instance {
    Instance::Group(vec![(
        "UseCurrent".into(),
        Instance::Scalar(Value::Bool(use_current)),
    )])
}

fn expected(current: &str) -> Instance {
    Instance::Group(vec![
        (
            "Active".into(),
            Instance::Scalar(Value::String(ACTIVE.into())),
        ),
        ("Main".into(), Instance::Scalar(Value::String(MAIN.into()))),
        (
            "CurrentOne".into(),
            Instance::Scalar(Value::String(current.into())),
        ),
        (
            "CurrentTwo".into(),
            Instance::Scalar(Value::String(current.into())),
        ),
        (
            "Lazy".into(),
            Instance::Scalar(Value::String("lazy".into())),
        ),
    ])
}

#[test]
fn runtime_values_match_engine_and_generated_backends() -> TestResult<()> {
    let project = runtime_project();
    let full =
        engine::ExecutionContext::with_main_mapping_file_path(Path::new(ACTIVE), Path::new(MAIN))
            .with_current_datetime(CURRENT);
    let paths =
        engine::ExecutionContext::with_main_mapping_file_path(Path::new(ACTIVE), Path::new(MAIN));
    assert_eq!(
        engine::run_with_context(&project, &source(true), &full)?,
        expected(CURRENT)
    );
    assert_eq!(
        engine::run_with_context(&project, &source(false), &paths)?,
        expected(SKIPPED)
    );
    assert_eq!(
        engine::run(&project, &source(true)),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::MappingFilePath,
        ))
    );
    assert_eq!(
        engine::run_with_context(&project, &source(true), &paths),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime,
        ))
    );

    let directory = TempDir::new("runtime_values")?;
    let project_path = directory.0.join("runtime-values.json");
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
        rust_output.join("src/main.rs"),
        include_str!("fixtures/runtime_values_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust runtime values failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/runtime_values_csharp_harness.cs.txt"),
    )?;
    let csharp = Command::new("dotnet")
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
        "generated C# runtime values failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
