use super::*;
use mapping::AggregateOp;

fn project() -> Project {
    let mut amount = string("Amount");
    amount.nullable = true;
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Rows", vec![amount]).repeating()],
        ),
        target: SchemaNode::group("Target", vec![string("Total")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::SourceField {
                        path: vec!["Amount".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "decimal_from_nullable_text".into(),
                        args: vec![1],
                    },
                ),
                (
                    3,
                    Node::Aggregate {
                        function: AggregateOp::DecimalSum,
                        collection: vec!["Rows".into()],
                        value: vec![],
                        expression: Some(2),
                        arg: None,
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "string".into(),
                        args: vec![3],
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Total".into(),
                node: 4,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn complete_decimal_documents_match_interpreter_and_generated_backends() -> TestResult<()> {
    let project = project();
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/decimal_mapping_cases.json"))?;
    for case in cases.as_array().unwrap() {
        let input = format_json::from_str(&case["input"].to_string(), &project.source)?;
        let result = engine::run(&project, &input);
        if let Some(kind) = case.get("error").and_then(serde_json::Value::as_str) {
            match (kind, result) {
                (
                    "conversion",
                    Err(engine::EngineError::Function(
                        codegen_runtime::FunctionError::DecimalConversion { .. },
                    )),
                )
                | ("sum", Err(engine::EngineError::AggregateDecimal { .. })) => {}
                (_, actual) => panic!("{}: unexpected failure result {actual:?}", case["id"]),
            }
        } else {
            assert_eq!(
                format_json::to_value(&project.target, &result?)?,
                case["expected"],
                "{}",
                case["id"]
            );
        }
    }

    let directory = TempDir::new("decimal-documents")?;
    let path = directory.0.join("mapping.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let rust = directory.0.join("rust");
    generate_project(
        &path,
        &rust,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    let csharp = directory.0.join("csharp");
    generate_project(&path, &csharp, GenerateTarget::CSharp)?;
    for output in [&rust, &csharp] {
        std::fs::write(
            output.join("cases.json"),
            include_str!("fixtures/decimal_mapping_cases.json"),
        )?;
        std::fs::write(
            output.join("source-schema.json"),
            serde_json::to_string(&project.source)?,
        )?;
        std::fs::write(
            output.join("target-schema.json"),
            serde_json::to_string(&project.target)?,
        )?;
    }
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("fixtures/decimal_mapping_rust.rs.txt"),
    )?;
    let manifest = rust.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)?;
    std::fs::write(
        &manifest,
        text.replacen(
            "\n[workspace]",
            "\nserde_json = \"=1.0.150\"\n\n[workspace]",
            1,
        ),
    )?;
    let result = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(&rust)
        .env("CARGO_BUILD_BUILD_DIR", directory.0.join("cargo-target"))
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        result.status.success(),
        "Rust decimal documents: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let harness = csharp.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/decimal_mapping_csharp.cs.txt"),
    )?;
    let result = dotnet_command(&csharp)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp)
        .isolated_output()?;
    assert!(
        result.status.success(),
        "C# decimal documents: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
