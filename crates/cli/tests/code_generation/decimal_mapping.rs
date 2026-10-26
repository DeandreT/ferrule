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
fn saved_decimal_literal_survives_native_and_generated_execution() -> TestResult<()> {
    let mut project = project();
    project.source = SchemaNode::group("Source", vec![]);
    project.graph.nodes = [
        (
            1,
            Node::Const {
                value: Value::decimal(ir::Decimal96::parse_exact("9007199254740993.1")?),
            },
        ),
        (
            2,
            Node::Call {
                function: "string".into(),
                args: vec![1],
            },
        ),
    ]
    .into();
    project.root.bindings[0].node = 2;
    verify_documents(
        project,
        r#"[{"id":"saved-decimal-literal","input":{},"expected":{"Total":"9007199254740993.1"}}]"#,
        "decimal-literal",
    )
}

#[test]
fn decimal_owned_collections_match_complete_documents_and_failure_order() -> TestResult<()> {
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Rows".into(), "Amount".into()],
            },
            frame: None,
        }),
    );
    project.graph.nodes.insert(
        3,
        Node::OwnedAggregate(mapping::OwnedAggregate {
            function: AggregateOp::DecimalSum,
            collection: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Rows".into()],
            },
            value: mapping::OwnedAggregateValue::Expression(2),
            arg: None,
        }),
    );
    verify_documents(
        project,
        include_str!("fixtures/decimal_mapping_cases.json"),
        "decimal-owned-documents",
    )
}

#[test]
fn decimal_missing_join_expression_fails_before_constant_or_raising_arguments() -> TestResult<()> {
    for owned in [false, true] {
        verify_documents(
            missing_join_expression_project(owned)?,
            include_str!("fixtures/decimal_missing_join_expression_cases.json"),
            if owned {
                "decimal-owned-missing-expression"
            } else {
                "decimal-missing-expression"
            },
        )?;
    }
    Ok(())
}

fn missing_join_expression_project(owned: bool) -> TestResult<Project> {
    let mut project = project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            string("Mode"),
            string("Key"),
            SchemaNode::group("Rows", vec![string("Key")]).repeating(),
        ],
    );
    let source = |parts: &[&str]| mapping::OwnedSourceRef {
        owner: mapping::SourceOwner::Primary,
        path: parts.iter().map(|part| (*part).into()).collect(),
    };
    let join = |id, arg| -> TestResult<Node> {
        let join = mapping::JoinId::new(id);
        Ok(if owned {
            Node::OwnedJoinAggregate(mapping::OwnedJoinAggregate {
                function: AggregateOp::DecimalSum,
                join,
                plan: mapping::OwnedJoinPlan::new(
                    mapping::OwnedJoinSource::new(source(&["Rows"])),
                    mapping::OwnedJoinSource::singleton(source(&["Key"])),
                    mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
                        source(&["Rows"]),
                        vec!["Key".into()],
                        vec![],
                    )),
                )?,
                expression: None,
                arg,
            })
        } else {
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
                expression: None,
                arg,
            }
        })
    };
    project.graph.nodes = [
        (
            1,
            if owned {
                Node::OwnedSourceField(mapping::OwnedSourceField {
                    source: source(&["Mode"]),
                    frame: None,
                })
            } else {
                Node::SourceField {
                    path: vec!["Mode".into()],
                    frame: None,
                }
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("constant".into()),
            },
        ),
        (
            3,
            Node::Call {
                function: "equal".into(),
                args: vec![1, 2],
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String("failing".into()),
            },
        ),
        (
            5,
            Node::Call {
                function: "equal".into(),
                args: vec![1, 4],
            },
        ),
        (
            6,
            Node::Const {
                value: Value::String("unused argument".into()),
            },
        ),
        (
            7,
            Node::Const {
                value: Value::String("argument failed".into()),
            },
        ),
        (8, Node::Raise { message: Some(7) }),
        (9, join(11, Some(6))?),
        (10, join(12, Some(8))?),
        (11, join(13, None)?),
        (
            12,
            Node::If {
                condition: 5,
                then: 10,
                else_: 11,
            },
        ),
        (
            13,
            Node::If {
                condition: 3,
                then: 9,
                else_: 12,
            },
        ),
        (
            14,
            Node::Call {
                function: "string".into(),
                args: vec![13],
            },
        ),
    ]
    .into();
    project.root.bindings[0].node = 14;
    Ok(project)
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
    verify_documents(joined.clone(), &cases["join"].to_string(), "decimal-join")?;
    let owned = |parts: &[&str]| mapping::OwnedSourceRef {
        owner: mapping::SourceOwner::Primary,
        path: parts.iter().map(|part| (*part).into()).collect(),
    };
    joined.graph.nodes.insert(
        1,
        Node::OwnedJoinField(mapping::OwnedJoinField {
            join,
            collection: owned(&["Rows"]),
            path: vec!["Amount".into()],
        }),
    );
    joined.graph.nodes.insert(
        3,
        Node::OwnedJoinAggregate(mapping::OwnedJoinAggregate {
            function: AggregateOp::DecimalSum,
            join,
            plan: mapping::OwnedJoinPlan::new(
                mapping::OwnedJoinSource::new(owned(&["Rows"])),
                mapping::OwnedJoinSource::singleton(owned(&["Key"])),
                mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
                    owned(&["Rows"]),
                    vec!["Key".into()],
                    vec![],
                )),
            )?,
            expression: Some(2),
            arg: None,
        }),
    );
    verify_documents(joined, &cases["join"].to_string(), "decimal-owned-join")
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
                | ("sum", Err(engine::EngineError::AggregateDecimal { .. }))
                | (
                    "unsupported",
                    Err(engine::EngineError::AggregateUnsupportedScalar {
                        function: AggregateOp::DecimalSum,
                        found: "null",
                    }),
                )
                | (
                    "argument",
                    Err(engine::EngineError::Function(
                        codegen_runtime::FunctionError::InvalidArgument {
                            function: "decimal_sum",
                            ..
                        },
                    )),
                )
                | ("raise", Err(engine::EngineError::MappingException { node: 8, .. })) => {}
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
    let csharp = directory.0.join("csharp");
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let owned_join = project
        .graph
        .nodes
        .values()
        .any(|node| matches!(node, Node::OwnedJoinAggregate(_)));
    let owned_collection = project
        .graph
        .nodes
        .values()
        .any(|node| matches!(node, Node::OwnedAggregate(_)));
    if owned_join || owned_collection {
        let options = codegen_rust::Options {
            package_name: "ferrule-generated-mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime_path.display().to_string(),
            ),
        };
        let (rust_files, csharp_files) = if owned_join {
            let program = codegen::lower_with_basic_owned_joins(&project)?;
            (
                codegen_rust::emit_with_basic_owned_joins(&program, &options)?,
                codegen_csharp::emit_with_basic_owned_joins(&program)?,
            )
        } else {
            let program = codegen::lower_with_basic_owned_collection_aggregates(&project)?;
            (
                codegen_rust::emit_with_basic_owned_collection_aggregates(&program, &options)?,
                codegen_csharp::emit_with_basic_owned_collection_aggregates(&program)?,
            )
        };
        for (output, files) in [(&rust, rust_files), (&csharp, csharp_files)] {
            for file in files.files() {
                let destination = output.join(file.path.as_str());
                std::fs::create_dir_all(destination.parent().unwrap())?;
                std::fs::write(destination, &file.contents)?;
            }
        }
    } else {
        generate_project(&path, &rust, GenerateTarget::Rust { runtime_path })?;
        generate_project(&path, &csharp, GenerateTarget::CSharp)?;
    }
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
    let mut rust_host = include_str!("fixtures/decimal_mapping_rust.rs.txt").to_owned();
    if owned_join || owned_collection {
        rust_host = rust_host.replace("use ferrule_generated_mapping::execute;", r#"fn execute(source: &codegen_runtime::Instance) -> Result<codegen_runtime::Instance, RuntimeError> {
    ferrule_generated_mapping::execute_with_basic_owned_sources(source, &[], None,
        codegen_runtime::OwnedSourceLimits { max_roots: 16, max_active_frames: 32,
            max_path_segments: 64, max_metadata_bytes: 16_777_216, max_walk_contexts: 512 })
        .map(|output| output.primary)
}"#);
    }
    std::fs::write(rust.join("src/main.rs"), rust_host)?;
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
    let mut csharp_host = include_str!("fixtures/decimal_mapping_csharp.cs.txt").to_owned();
    if owned_join || owned_collection {
        csharp_host = csharp_host.replace("GeneratedMapping.Execute(source)", "GeneratedMapping.ExecuteWithBasicOwnedSources(source, Array.Empty<NamedInput>(), new FerruleOwnedSourceLimits(16, 32, 64, 16_777_216, 512), null).Primary");
    }
    std::fs::write(harness.join("Program.cs"), csharp_host)?;
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
