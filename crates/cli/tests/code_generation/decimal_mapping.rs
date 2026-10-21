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
    verify_documents(
        project(),
        include_str!("fixtures/decimal_mapping_cases.json"),
        "decimal-documents",
    )
}

#[test]
fn decimal_sequence_and_join_projection_failures_keep_source_order() -> TestResult<()> {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/decimal_projection_cases.json"))?;
    let mut sequence = project();
    sequence.source = SchemaNode::group("Source", vec![string("Numbers")]);
    sequence.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    sequence.graph.nodes.insert(
        5,
        Node::SourceField {
            path: vec!["Numbers".into()],
            frame: None,
        },
    );
    sequence.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String("|".into()),
        },
    );
    sequence.graph.nodes.insert(
        3,
        Node::SequenceAggregate {
            function: AggregateOp::DecimalSum,
            sequence: mapping::SequenceExpr::Tokenize {
                input: 5,
                delimiter: 6,
                item: 1,
            },
            predicate: None,
            expression: Some(2),
            arg: None,
        },
    );
    verify_documents(sequence, &cases["sequence"].to_string(), "decimal-sequence")?;

    let mut joined = project();
    let mut amount = string("Amount");
    amount.nullable = true;
    joined.source = SchemaNode::group(
        "Source",
        vec![
            string("Key"),
            SchemaNode::group("Rows", vec![string("Key"), amount]).repeating(),
        ],
    );
    let join = mapping::JoinId::new(7);
    joined.graph.nodes.insert(
        1,
        Node::JoinField {
            join,
            collection: vec!["Rows".into()],
            path: vec!["Amount".into()],
        },
    );
    joined.graph.nodes.insert(
        3,
        Node::JoinAggregate {
            function: AggregateOp::DecimalSum,
            join,
            plan: mapping::JoinPlan::new(
                mapping::JoinSource::new(vec!["Rows".into()]),
                mapping::JoinSource::singleton(vec!["Key".into()]),
                mapping::JoinConditions::new(mapping::JoinKey::new(
                    vec!["Rows".into()],
                    vec!["Key".into()],
                    vec![],
                )),
            )?,
            expression: Some(2),
            arg: None,
        },
    );
    verify_documents(joined, &cases["join"].to_string(), "decimal-join")
}

fn verify_documents(project: Project, cases_json: &str, label: &str) -> TestResult<()> {
    let directory = TempDir::new(label)?;
    let path = directory.0.join("mapping.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let validation = engine::validate(&project);
    std::fs::write(
        directory.0.join("validation.txt"),
        format!("{validation:#?}\n"),
    )?;
    assert!(validation.is_empty(), "{label}: {validation:?}");
    let cases: serde_json::Value = serde_json::from_str(cases_json)?;
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let input = format_json::from_str(&case["input"].to_string(), &project.source)?;
        let result = engine::run(&project, &input);
        let observation = match &result {
            Ok(value) => {
                serde_json::json!({"id":case["id"], "output":format_json::to_value(&project.target, value)?})
            }
            Err(error) => serde_json::json!({"id":case["id"], "error":format!("{error:?}")}),
        };
        std::fs::write(
            directory.0.join(format!("native-{index:02}.json")),
            serde_json::to_vec_pretty(&observation)?,
        )?;
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
        std::fs::write(output.join("cases.json"), cases_json)?;
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
    std::fs::write(directory.0.join("rust.stdout.txt"), &result.stdout)?;
    std::fs::write(directory.0.join("rust.stderr.txt"), &result.stderr)?;
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
    std::fs::write(directory.0.join("csharp.stdout.txt"), &result.stdout)?;
    std::fs::write(directory.0.join("csharp.stderr.txt"), &result.stderr)?;
    assert!(
        result.status.success(),
        "C# decimal documents: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
