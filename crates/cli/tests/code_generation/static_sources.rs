use super::*;

fn static_source_project() -> Project {
    let catalog_row =
        SchemaNode::group("CatalogRow", vec![string("Sku"), string("Label")]).repeating();
    let label_row = SchemaNode::group("LabelRow", vec![string("Text")]).repeating();
    let result =
        SchemaNode::group("Result", vec![string("Sku"), string("CatalogLabel")]).repeating();
    let reference_label = SchemaNode::group("ReferenceLabel", vec![string("Text")]).repeating();

    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Order", vec![string("Sku")]).repeating()],
        ),
        target: SchemaNode::group("Target", vec![int("CatalogCount"), result, reference_label]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            mapping::NamedSource {
                name: "catalog".into(),
                path: "ignored/catalog.json".into(),
                schema: SchemaNode::group("CatalogDocument", vec![catalog_row]),
                options: Default::default(),
                dynamic_path: None,
            },
            mapping::NamedSource {
                name: "labels".into(),
                path: "ignored/labels.json".into(),
                schema: SchemaNode::group("LabelsDocument", vec![label_row]),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: vec![mapping::NamedTarget {
            name: "audit".into(),
            path: None,
            schema: SchemaNode::group("Audit", vec![string("FirstCatalogLabel")]),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "FirstCatalogLabel".into(),
                    node: 50,
                }],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    Node::SourceField {
                        path: vec!["Sku".into()],
                        frame: Some(vec!["Order".into()]),
                    },
                ),
                (
                    20,
                    Node::Lookup {
                        collection: vec!["catalog".into(), "CatalogRow".into()],
                        key: vec!["Sku".into()],
                        matches: 10,
                        value: vec!["Label".into()],
                    },
                ),
                (
                    30,
                    Node::Aggregate {
                        function: mapping::AggregateOp::Count,
                        collection: vec!["catalog".into(), "CatalogRow".into()],
                        value: vec!["Sku".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    40,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: Some(vec!["labels".into(), "LabelRow".into()]),
                    },
                ),
                (
                    50,
                    Node::SourceField {
                        path: vec!["catalog".into(), "CatalogRow".into(), "Label".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "CatalogCount".into(),
                node: 30,
            }],
            children: vec![
                Scope {
                    target_field: "Result".into(),
                    iteration: ScopeIteration::Source(vec!["Order".into()]),
                    bindings: vec![
                        Binding {
                            target_field: "Sku".into(),
                            node: 10,
                        },
                        Binding {
                            target_field: "CatalogLabel".into(),
                            node: 20,
                        },
                    ],
                    ..Scope::default()
                },
                Scope {
                    target_field: "ReferenceLabel".into(),
                    iteration: ScopeIteration::Source(vec!["labels".into(), "LabelRow".into()]),
                    bindings: vec![Binding {
                        target_field: "Text".into(),
                        node: 40,
                    }],
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        },
    }
}

fn primary_source() -> Instance {
    Instance::Group(vec![(
        "Order".into(),
        Instance::Repeated(vec![order("A"), order("missing"), order("B")]),
    )])
}

fn order(sku: &str) -> Instance {
    Instance::Group(vec![(
        "Sku".into(),
        Instance::Scalar(Value::String(sku.into())),
    )])
}

fn catalog_source() -> Instance {
    Instance::Group(vec![(
        "CatalogRow".into(),
        Instance::Repeated(vec![catalog_row("A", "Alpha"), catalog_row("B", "Beta")]),
    )])
}

fn catalog_row(sku: &str, label: &str) -> Instance {
    Instance::Group(vec![
        ("Sku".into(), Instance::Scalar(Value::String(sku.into()))),
        (
            "Label".into(),
            Instance::Scalar(Value::String(label.into())),
        ),
    ])
}

fn labels_source() -> Instance {
    Instance::Group(vec![(
        "LabelRow".into(),
        Instance::Repeated(vec![label_row("first"), label_row("second")]),
    )])
}

fn label_row(text: &str) -> Instance {
    Instance::Group(vec![(
        "Text".into(),
        Instance::Scalar(Value::String(text.into())),
    )])
}

fn expected_primary() -> Instance {
    let result = |sku: &str, label: Value| {
        Instance::Group(vec![
            ("Sku".into(), Instance::Scalar(Value::String(sku.into()))),
            ("CatalogLabel".into(), Instance::Scalar(label)),
        ])
    };
    Instance::Group(vec![
        ("CatalogCount".into(), Instance::Scalar(Value::Int(2))),
        (
            "Result".into(),
            Instance::Repeated(vec![
                result("A", Value::String("Alpha".into())),
                result("missing", Value::Null),
                result("B", Value::String("Beta".into())),
            ]),
        ),
        (
            "ReferenceLabel".into(),
            Instance::Repeated(vec![label_row("first"), label_row("second")]),
        ),
    ])
}

fn expected_audit() -> Instance {
    Instance::Group(vec![(
        "FirstCatalogLabel".into(),
        Instance::Scalar(Value::String("Alpha".into())),
    )])
}

fn engine_sources() -> Vec<(String, Instance)> {
    vec![
        ("catalog".into(), catalog_source()),
        ("labels".into(), labels_source()),
    ]
}

#[test]
fn static_sources_match_engine_and_generated_backends() -> TestResult<()> {
    let project = static_source_project();
    assert!(engine::validate(&project).is_empty());
    let execution = engine::ExecutionContext::new(Path::new("static-sources.ferrule"));
    let expected = engine::run_outputs_with_sources_and_context(
        &project,
        &primary_source(),
        engine_sources(),
        &execution,
    )?;
    assert_eq!(expected.primary, expected_primary());
    assert_eq!(expected.extras.len(), 1);
    assert_eq!(expected.extras[0].name, "audit");
    assert_eq!(expected.extras[0].instance, expected_audit());

    let directory = TempDir::new("static_sources")?;
    let project_path = directory.0.join("static-sources.json");
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
        include_str!("fixtures/static_sources_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust static sources failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/static_sources_csharp_harness.cs.txt"),
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
        "generated C# static sources failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
