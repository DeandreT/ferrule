use super::*;
use mapping::AggregateOp;

fn project() -> Project {
    let mut text = SchemaNode::scalar_union(
        "Text",
        ir::ScalarTypeSet::new([
            ScalarType::String,
            ScalarType::Int,
            ScalarType::Float,
            ScalarType::Bool,
        ])
        .unwrap(),
    );
    text.nullable = true;
    let aggregate = |function| Node::Aggregate {
        function,
        collection: vec!["Rows".into()],
        value: vec!["Text".into()],
        expression: None,
        arg: None,
    };
    let mut project = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", vec![text]).repeating(),
                string("Mode"),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                string("Last"),
                string("Distinct"),
                bool_("Present"),
                bool_("DistinctPresent"),
                string("ComputedLast"),
                string("ComputedDistinct"),
            ],
        ),
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
                (1, aggregate(AggregateOp::LastPresentText)),
                (2, aggregate(AggregateOp::LastDistinctText)),
                (
                    3,
                    Node::Call {
                        function: "string".into(),
                        args: vec![1],
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "string".into(),
                        args: vec![2],
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![1],
                    },
                ),
                (
                    6,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
                (
                    7,
                    Node::Aggregate {
                        function: AggregateOp::LastPresentText,
                        collection: vec!["Rows".into()],
                        value: vec![],
                        expression: Some(6),
                        arg: None,
                    },
                ),
                (
                    8,
                    Node::Aggregate {
                        function: AggregateOp::LastDistinctText,
                        collection: vec!["Rows".into()],
                        value: vec![],
                        expression: Some(6),
                        arg: None,
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "string".into(),
                        args: vec![7],
                    },
                ),
                (
                    10,
                    Node::Call {
                        function: "string".into(),
                        args: vec![8],
                    },
                ),
                (
                    11,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![2],
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: [
                ("Last", 3),
                ("Distinct", 4),
                ("Present", 5),
                ("DistinctPresent", 11),
                ("ComputedLast", 9),
                ("ComputedDistinct", 10),
            ]
            .into_iter()
            .map(|(field, node)| Binding {
                target_field: field.into(),
                node,
            })
            .collect(),
            ..Scope::default()
        },
    };
    // Select the first binding's probe without rebuilding the generated host.
    // Every branch is emitted; only the selected operation may fail at runtime.
    project
        .graph
        .nodes
        .insert(12, Node::Const { value: Value::Null });
    project.graph.nodes.insert(
        13,
        Node::SourceField {
            path: vec!["Mode".into()],
            frame: None,
        },
    );
    let mut selected = 1;
    for (base, mode, function, argument) in [
        (20, "distinct", AggregateOp::LastDistinctText, false),
        (30, "present-arg", AggregateOp::LastPresentText, true),
        (40, "distinct-arg", AggregateOp::LastDistinctText, true),
    ] {
        project.graph.nodes.extend([
            (
                base,
                Node::Const {
                    value: Value::String(mode.into()),
                },
            ),
            (
                base + 1,
                Node::Call {
                    function: "equal".into(),
                    args: vec![13, base],
                },
            ),
            (
                base + 2,
                Node::Aggregate {
                    function,
                    collection: vec!["Rows".into()],
                    value: vec!["Text".into()],
                    expression: None,
                    arg: argument.then_some(12),
                },
            ),
            (
                base + 3,
                Node::If {
                    condition: base + 1,
                    then: base + 2,
                    else_: selected,
                },
            ),
        ]);
        selected = base + 3;
    }
    project.graph.nodes.insert(
        3,
        Node::Call {
            function: "string".into(),
            args: vec![selected],
        },
    );
    project
}

#[test]
fn ordered_text_documents_match_interpreter_and_generated_backends() -> TestResult<()> {
    let authored = project();
    let encoded = mapping::project_file::encode_pretty(&authored)?;
    let project = mapping::project_file::decode_str(&encoded)?;
    assert_eq!(
        serde_json::to_value(&project)?,
        serde_json::to_value(&authored)?,
        "saved/reopened selectors must preserve the complete project"
    );
    assert_eq!(mapping::project_file::encode_pretty(&project)?, encoded);
    let directory = TempDir::new("ordered-text-selection")?;
    let path = directory.0.join("mapping.json");
    std::fs::write(&path, encoded)?;
    let validation = engine::validate(&project);
    std::fs::write(
        directory.0.join("validation.txt"),
        format!("{validation:#?}"),
    )?;
    assert!(validation.is_empty(), "{validation:?}");
    let fixture = include_str!("fixtures/ordered_text_selection_cases.json");
    let cases: serde_json::Value = serde_json::from_str(fixture)?;
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let input = format_json::from_str(&case["input"].to_string(), &project.source)?;
        let result = engine::run(&project, &input);
        let observation = match &result {
            Ok(output) => {
                serde_json::json!({"id":case["id"],"output":format_json::to_value(&project.target,output)?})
            }
            Err(error) => serde_json::json!({"id":case["id"],"error":format!("{error:?}")}),
        };
        std::fs::write(
            directory.0.join(format!("native-{index:02}.json")),
            serde_json::to_vec_pretty(&observation)?,
        )?;
        if case.get("error").is_some() {
            let name = case["function"].as_str().unwrap_or("last_present_text");
            let expected_kind = case["found"].as_str().unwrap_or("json null");
            let operation = if name == "last_distinct_text" {
                AggregateOp::LastDistinctText
            } else {
                AggregateOp::LastPresentText
            };
            assert!(
                match &result {
                    Err(engine::EngineError::AggregateUnsupportedScalar { function, found }) =>
                        case["error"] == "unsupported"
                            && *function == operation
                            && *found == expected_kind,
                    Err(engine::EngineError::Function(
                        codegen_runtime::FunctionError::InvalidArgument { function, .. },
                    )) => case["error"] == "argument" && *function == name,
                    _ => false,
                },
                "{}: {result:?}",
                case["id"]
            );
        } else {
            let actual = format_json::to_value(&project.target, &result?)?;
            let mut expected = case["expected"].clone();
            expected["DistinctPresent"] = expected["Present"].clone();
            expected["ComputedLast"] = expected["Last"].clone();
            expected["ComputedDistinct"] = expected["Distinct"].clone();
            assert_eq!(actual, expected, "{}", case["id"]);
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
    for target in [&rust, &csharp] {
        std::fs::write(target.join("cases.json"), fixture)?;
        std::fs::write(
            target.join("source-schema.json"),
            serde_json::to_string(&project.source)?,
        )?;
        std::fs::write(
            target.join("target-schema.json"),
            serde_json::to_string(&project.target)?,
        )?;
    }
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("fixtures/ordered_text_selection_rust.rs.txt"),
    )?;
    let manifest = rust.join("Cargo.toml");
    let body = std::fs::read_to_string(&manifest)?;
    std::fs::write(
        &manifest,
        body.replacen(
            "\n[workspace]",
            "\nserde_json = \"=1.0.150\"\n\n[workspace]",
            1,
        ),
    )?;
    let result = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(&rust)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .env("CARGO_BUILD_BUILD_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    std::fs::write(directory.0.join("rust.stdout.txt"), &result.stdout)?;
    std::fs::write(directory.0.join("rust.stderr.txt"), &result.stderr)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let harness = csharp.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/ordered_text_selection_csharp.cs.txt"),
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
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
