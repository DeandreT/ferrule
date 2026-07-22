use super::*;

fn value_map_project() -> Project {
    let fields = [
        "Duplicate",
        "Default",
        "NoDefault",
        "FloatString",
        "Int",
        "Float",
        "Bool",
        "Failed",
        "Null",
        "XmlNil",
    ];
    let mut nodes = BTreeMap::new();
    let mut bindings = Vec::new();
    let cases = [
        (
            Value::String("same".into()),
            None,
            vec![
                (Value::String("same".into()), Value::String("first".into())),
                (Value::String("same".into()), Value::String("second".into())),
            ],
            Some(Value::String("unused".into())),
        ),
        (
            Value::String("missing".into()),
            None,
            vec![(Value::String("known".into()), Value::String("known".into()))],
            Some(Value::String("fallback".into())),
        ),
        (Value::String("missing".into()), None, Vec::new(), None),
        (
            Value::Float(1e20),
            Some(ScalarType::String),
            vec![(
                Value::String("100000000000000000000".into()),
                Value::String("float-string".into()),
            )],
            None,
        ),
        (
            Value::String(" 1 ".into()),
            Some(ScalarType::Int),
            vec![(Value::Int(1), Value::String("int".into()))],
            None,
        ),
        (
            Value::String(" 1 ".into()),
            Some(ScalarType::Float),
            vec![(Value::Float(1.0), Value::String("float".into()))],
            None,
        ),
        (
            Value::String(" 1 ".into()),
            Some(ScalarType::Bool),
            vec![(Value::Bool(true), Value::String("bool".into()))],
            None,
        ),
        (
            Value::String("1.0".into()),
            Some(ScalarType::Int),
            vec![(
                Value::String("1.0".into()),
                Value::String("retained".into()),
            )],
            None,
        ),
        (
            Value::Null,
            Some(ScalarType::Bool),
            vec![(Value::Null, Value::String("null".into()))],
            None,
        ),
        (
            Value::xml_nil(),
            Some(ScalarType::Float),
            vec![(Value::xml_nil(), Value::String("xml-nil".into()))],
            None,
        ),
    ];
    for (index, (input, input_type, table, default)) in cases.into_iter().enumerate() {
        let input_id = index as u32 * 2 + 1;
        let map_id = input_id + 1;
        nodes.insert(input_id, Node::Const { value: input });
        nodes.insert(
            map_id,
            Node::ValueMap {
                input: input_id,
                input_type,
                table,
                default,
            },
        );
        bindings.push(Binding {
            target_field: fields[index].into(),
            node: map_id,
        });
    }
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Target", fields.into_iter().map(string).collect()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            bindings,
            ..Scope::default()
        },
    }
}

fn expected_output() -> Instance {
    Instance::Group(vec![
        (
            "Duplicate".into(),
            Instance::Scalar(Value::String("first".into())),
        ),
        (
            "Default".into(),
            Instance::Scalar(Value::String("fallback".into())),
        ),
        ("NoDefault".into(), Instance::Scalar(Value::Null)),
        (
            "FloatString".into(),
            Instance::Scalar(Value::String("float-string".into())),
        ),
        ("Int".into(), Instance::Scalar(Value::String("int".into()))),
        (
            "Float".into(),
            Instance::Scalar(Value::String("float".into())),
        ),
        (
            "Bool".into(),
            Instance::Scalar(Value::String("bool".into())),
        ),
        (
            "Failed".into(),
            Instance::Scalar(Value::String("retained".into())),
        ),
        (
            "Null".into(),
            Instance::Scalar(Value::String("null".into())),
        ),
        (
            "XmlNil".into(),
            Instance::Scalar(Value::String("xml-nil".into())),
        ),
    ])
}

#[test]
fn value_maps_match_engine_and_generated_backends() -> TestResult<()> {
    let project = value_map_project();
    let source = Instance::Group(Vec::new());
    assert_eq!(engine::run(&project, &source)?, expected_output());

    let directory = TempDir::new("value_maps")?;
    let project_path = directory.0.join("value-maps.json");
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
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/value_maps_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust value maps failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/value_maps_csharp_harness.cs.txt"),
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
        "generated C# value maps failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
