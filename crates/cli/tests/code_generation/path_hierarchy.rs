use super::*;
use mapping::{PathHierarchyPlan, ScopeConstruction};

fn hierarchy_schema() -> (SchemaNode, SchemaNode) {
    (
        SchemaNode::group("FileList", vec![string("File").repeating()]),
        SchemaNode::group(
            "directory",
            vec![
                SchemaNode::group("file", vec![string("name")]).repeating(),
                SchemaNode::recursive_group("directory", "directory").repeating(),
                string("name"),
            ],
        ),
    )
}

fn hierarchy_project() -> Project {
    let (source, target) = hierarchy_schema();
    let Some(plan) = PathHierarchyPlan::new(
        vec!["File".into()],
        "/".into(),
        "directory".into(),
        "file".into(),
        "name".into(),
    ) else {
        panic!("valid path-hierarchy plan");
    };
    Project {
        source,
        target,
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
            construction: ScopeConstruction::PathHierarchy { plan },
            ..Scope::default()
        },
    }
}

fn hierarchy_source(paths: impl IntoIterator<Item = Value>) -> Instance {
    Instance::Group(vec![(
        "File".into(),
        Instance::Repeated(paths.into_iter().map(Instance::Scalar).collect()),
    )])
}

fn hierarchy_expected() -> Instance {
    Instance::Group(vec![
        (
            "file".into(),
            Instance::Repeated(vec![Instance::Group(vec![(
                "name".into(),
                Instance::Scalar(Value::String("a.txt".into())),
            )])]),
        ),
        (
            "directory".into(),
            Instance::Repeated(vec![Instance::Group(vec![
                (
                    "file".into(),
                    Instance::Repeated(vec![
                        Instance::Group(vec![(
                            "name".into(),
                            Instance::Scalar(Value::String("b.txt".into())),
                        )]),
                        Instance::Group(vec![(
                            "name".into(),
                            Instance::Scalar(Value::String("b.txt".into())),
                        )]),
                    ]),
                ),
                ("directory".into(), Instance::Repeated(Vec::new())),
                ("name".into(), Instance::Scalar(Value::String("b".into()))),
            ])]),
        ),
        (
            "name".into(),
            Instance::Scalar(Value::String("root".into())),
        ),
    ])
}

#[test]
fn path_hierarchy_matches_engine_and_generated_backends() -> TestResult<()> {
    let project = hierarchy_project();
    let source = hierarchy_source([
        Value::String("root/b/b.txt".into()),
        Value::String("root/a.txt".into()),
        Value::String("root/b/b.txt".into()),
        Value::Null,
        Value::String("top-level.txt".into()),
    ]);
    assert_eq!(engine::run(&project, &source)?, hierarchy_expected());
    assert_eq!(
        engine::run(
            &project,
            &hierarchy_source([
                Value::String("one/a.txt".into()),
                Value::String("two/b.txt".into()),
            ]),
        ),
        Err(engine::EngineError::PathHierarchyRootCount { count: 2 })
    );

    let directory = TempDir::new("path_hierarchy")?;
    let project_path = directory.0.join("path-hierarchy.json");
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
    assert!(rust_source.contains("let output = path_hierarchy("));
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/path_hierarchy_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust path hierarchy failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let csharp_source = std::fs::read_to_string(csharp_output.join("GeneratedMapping.cs"))?;
    assert!(csharp_source.contains("FerrulePathHierarchy.Build("));
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
        include_str!("fixtures/path_hierarchy_csharp_harness.cs.txt"),
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
        "generated C# path hierarchy failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
