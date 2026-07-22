use super::*;

fn lookup_project() -> Project {
    let row = SchemaNode::group(
        "Row",
        vec![
            string("Label"),
            int("IntegerNeedle"),
            SchemaNode::scalar("FloatNeedle", ScalarType::Float),
        ],
    )
    .repeating();
    let catalog = SchemaNode::group(
        "Catalog",
        vec![
            SchemaNode::group("Identity", vec![int("Code")]),
            SchemaNode::group("Payload", vec![string("Text")]),
        ],
    )
    .repeating();
    let result = SchemaNode::group(
        "Result",
        vec![
            string("Label"),
            string("IntegerMatch"),
            string("FloatMatch"),
        ],
    )
    .repeating();

    Project {
        source: SchemaNode::group("Source", vec![row, catalog]),
        target: SchemaNode::group("Target", vec![result]),
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
                (
                    10,
                    Node::SourceField {
                        frame: Some(vec!["Row".into()]),
                        path: vec!["Label".into()],
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        frame: Some(vec!["Row".into()]),
                        path: vec!["IntegerNeedle".into()],
                    },
                ),
                (
                    30,
                    Node::SourceField {
                        frame: Some(vec!["Row".into()]),
                        path: vec!["FloatNeedle".into()],
                    },
                ),
                (
                    40,
                    Node::Lookup {
                        collection: vec!["Catalog".into()],
                        key: vec!["Identity".into(), "Code".into()],
                        matches: 20,
                        value: vec!["Payload".into(), "Text".into()],
                    },
                ),
                (
                    50,
                    Node::Lookup {
                        collection: vec!["Catalog".into()],
                        key: vec!["Identity".into(), "Code".into()],
                        matches: 30,
                        value: vec!["Payload".into(), "Text".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Result".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Label".into(),
                        node: 10,
                    },
                    Binding {
                        target_field: "IntegerMatch".into(),
                        node: 40,
                    },
                    Binding {
                        target_field: "FloatMatch".into(),
                        node: 50,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn lookup_source() -> Instance {
    let row = |label: &str, integer: i64, float: f64| {
        Instance::Group(vec![
            (
                "Label".into(),
                Instance::Scalar(Value::String(label.into())),
            ),
            (
                "IntegerNeedle".into(),
                Instance::Scalar(Value::Int(integer)),
            ),
            ("FloatNeedle".into(), Instance::Scalar(Value::Float(float))),
        ])
    };
    let catalog = |key: Option<Value>, value: Option<&str>| {
        let mut fields = Vec::new();
        if let Some(key) = key {
            fields.push((
                "Identity".into(),
                Instance::Group(vec![("Code".into(), Instance::Scalar(key))]),
            ));
        }
        if let Some(value) = value {
            fields.push((
                "Payload".into(),
                Instance::Group(vec![(
                    "Text".into(),
                    Instance::Scalar(Value::String(value.into())),
                )]),
            ));
        }
        Instance::Group(fields)
    };

    Instance::Group(vec![
        (
            "Row".into(),
            Instance::Repeated(vec![
                row("numeric-tags", 1, 1.0),
                row("missing-value", 2, 2.0),
                row("after-missing-key", 3, 3.0),
                row("miss", 99, 99.0),
            ]),
        ),
        (
            "Catalog".into(),
            Instance::Repeated(vec![
                catalog(None, Some("missing-key-must-be-skipped")),
                catalog(Some(Value::Int(1)), Some("first-integer")),
                catalog(Some(Value::Int(1)), Some("second-integer")),
                catalog(Some(Value::Float(1.0)), Some("float")),
                catalog(Some(Value::Int(2)), None),
                catalog(Some(Value::Int(2)), Some("must-not-continue")),
                catalog(Some(Value::Int(3)), Some("after-missing-key")),
            ]),
        ),
    ])
}

fn lookup_expected() -> Instance {
    let row = |label: &str, integer: Value, float: Value| {
        Instance::Group(vec![
            (
                "Label".into(),
                Instance::Scalar(Value::String(label.into())),
            ),
            ("IntegerMatch".into(), Instance::Scalar(integer)),
            ("FloatMatch".into(), Instance::Scalar(float)),
        ])
    };
    Instance::Group(vec![(
        "Result".into(),
        Instance::Repeated(vec![
            row(
                "numeric-tags",
                Value::String("first-integer".into()),
                Value::String("float".into()),
            ),
            row("missing-value", Value::Null, Value::Null),
            row(
                "after-missing-key",
                Value::String("after-missing-key".into()),
                Value::Null,
            ),
            row("miss", Value::Null, Value::Null),
        ]),
    )])
}

#[test]
fn primary_source_lookups_match_engine_and_generated_backends() -> TestResult<()> {
    let project = lookup_project();
    let source = lookup_source();
    assert_eq!(engine::run(&project, &source)?, lookup_expected());

    let directory = TempDir::new("lookups")?;
    let project_path = directory.0.join("lookups.json");
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
        include_str!("fixtures/lookups_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust lookups failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/lookups_csharp_harness.cs.txt"),
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
        "generated C# lookups failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
