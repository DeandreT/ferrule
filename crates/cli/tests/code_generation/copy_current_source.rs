use super::*;
use mapping::ScopeConstruction;

fn copy_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            int("Id"),
            string("Missing"),
            string("Nil").nillable(),
            SchemaNode::group("Empty", vec![string("Note")]),
            SchemaNode::group("Items", vec![string("Name")]).repeating(),
        ],
    )
}

fn copy_project() -> Project {
    Project {
        source: copy_schema("Source"),
        target: copy_schema("Target"),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

fn copy_source() -> Instance {
    Instance::Group(vec![
        ("Id".into(), Instance::Scalar(Value::Int(7))),
        ("Missing".into(), Instance::Scalar(Value::Null)),
        ("Nil".into(), Instance::Scalar(Value::xml_nil())),
        ("Empty".into(), Instance::Group(Vec::new())),
        (
            "Items".into(),
            Instance::MappedSequence(vec![
                Instance::Group(vec![(
                    "Name".into(),
                    Instance::Scalar(Value::String("first".into())),
                )]),
                Instance::Group(vec![(
                    "Name".into(),
                    Instance::Scalar(Value::String("second".into())),
                )]),
            ]),
        ),
    ])
}

#[test]
fn copy_current_source_matches_engine_and_generated_backends() -> TestResult<()> {
    let project = copy_project();
    let source = copy_source();
    assert_eq!(engine::run(&project, &source)?, source);
    assert_eq!(
        engine::run(&project, &Instance::Scalar(Value::String("wrong".into()))),
        Err(engine::EngineError::CopyCurrentSourceRequiresGroup { found: "scalar" })
    );

    let directory = TempDir::new("copy_current_source")?;
    let project_path = directory.0.join("copy-project.json");
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
    assert!(rust_source.contains("let output = context.copy_current_group()?;"));
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/copy_current_source_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust copy-current-source failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let csharp_source = std::fs::read_to_string(csharp_output.join("GeneratedMapping.cs"))?;
    assert!(csharp_source.contains("return context.CopyCurrentGroup();"));
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
        include_str!("fixtures/copy_current_source_csharp_harness.cs.txt"),
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
        "generated C# copy-current-source failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
