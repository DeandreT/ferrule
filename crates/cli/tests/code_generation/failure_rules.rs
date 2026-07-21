use super::*;
use mapping::{FailureIteration, FailureRule, FailureSelection, SequenceExpr};

fn failure_project() -> Project {
    let row = SchemaNode::group("Row", vec![string("Code"), bool_("Valid")]).repeating();
    let trigger = SchemaNode::group("EmptyTrigger", Vec::new()).repeating();
    let bad = SchemaNode::group("BadTrigger", vec![string("Value")]).repeating();
    let flag = SchemaNode::group("Flag", vec![string("Code"), bool_("Reject")]).repeating();
    Project {
        source: SchemaNode::group(
            "Source",
            vec![string("Name"), bool_("FailGenerated"), row, trigger, bad],
        ),
        target: SchemaNode::group("Target", vec![string("Name")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "rules".into(),
            path: "ignored/rules.json".into(),
            schema: SchemaNode::group("RulesDocument", vec![flag]),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![mapping::NamedTarget {
            name: "audit".into(),
            path: None,
            schema: SchemaNode::group("Audit", vec![string("Name")]),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Name".into(),
                    node: 1,
                }],
                ..Scope::default()
            },
        }],
        failure_rules: vec![
            FailureRule {
                iteration: FailureIteration::Source {
                    collection: vec!["Row".into()],
                },
                selection: FailureSelection::WhenFalse { predicate: 2 },
                message: Some(5),
            },
            FailureRule {
                iteration: FailureIteration::Sequence {
                    sequence: SequenceExpr::Generate {
                        from: Some(10),
                        to: 11,
                        item: 12,
                    },
                },
                selection: FailureSelection::WhenTrue { predicate: 16 },
                message: Some(12),
            },
            FailureRule {
                iteration: FailureIteration::Source {
                    collection: vec!["rules".into(), "Flag".into()],
                },
                selection: FailureSelection::WhenTrue { predicate: 20 },
                message: Some(21),
            },
            FailureRule {
                iteration: FailureIteration::Source {
                    collection: vec!["EmptyTrigger".into()],
                },
                selection: FailureSelection::All,
                message: Some(30),
            },
            FailureRule {
                iteration: FailureIteration::Source {
                    collection: vec!["BadTrigger".into()],
                },
                selection: FailureSelection::WhenTrue { predicate: 31 },
                message: None,
            },
        ],
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Valid".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("invalid:".into()),
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Code".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![3, 4],
                    },
                ),
                (
                    10,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    12,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    13,
                    Node::SourceField {
                        path: vec!["FailGenerated".into()],
                        frame: None,
                    },
                ),
                (
                    14,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    15,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![12, 14],
                    },
                ),
                (
                    16,
                    Node::Call {
                        function: "and".into(),
                        args: vec![13, 15],
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        path: vec!["Reject".into()],
                        frame: Some(vec!["rules".into(), "Flag".into()]),
                    },
                ),
                (
                    21,
                    Node::SourceField {
                        path: vec!["Code".into()],
                        frame: Some(vec!["rules".into(), "Flag".into()]),
                    },
                ),
                (30, Node::Const { value: Value::Null }),
                (
                    31,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["BadTrigger".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Name".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

#[derive(Clone, Copy)]
enum FailureCase {
    None,
    Source,
    Generated,
    Named,
    EmptyMessage,
    NonBoolean,
}

fn primary(case: FailureCase) -> Instance {
    let rows = match case {
        FailureCase::Source => vec![row("A", true), row("B", false), row("C", false)],
        _ => vec![row("A", true), row("B", true)],
    };
    let empty = matches!(case, FailureCase::EmptyMessage)
        .then(|| Instance::Group(Vec::new()))
        .into_iter()
        .collect();
    let bad = matches!(case, FailureCase::NonBoolean)
        .then(|| {
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String("not-bool".into())),
            )])
        })
        .into_iter()
        .collect();
    Instance::Group(vec![
        (
            "Name".into(),
            Instance::Scalar(Value::String("mapped".into())),
        ),
        (
            "FailGenerated".into(),
            Instance::Scalar(Value::Bool(matches!(case, FailureCase::Generated))),
        ),
        ("Row".into(), Instance::Repeated(rows)),
        ("EmptyTrigger".into(), Instance::Repeated(empty)),
        ("BadTrigger".into(), Instance::Repeated(bad)),
    ])
}

fn row(code: &str, valid: bool) -> Instance {
    Instance::Group(vec![
        ("Code".into(), Instance::Scalar(Value::String(code.into()))),
        ("Valid".into(), Instance::Scalar(Value::Bool(valid))),
    ])
}

fn rules(case: FailureCase) -> Instance {
    let flag = |code: &str, reject| {
        Instance::Group(vec![
            ("Code".into(), Instance::Scalar(Value::String(code.into()))),
            ("Reject".into(), Instance::Scalar(Value::Bool(reject))),
        ])
    };
    Instance::Group(vec![(
        "Flag".into(),
        Instance::Repeated(vec![
            flag("allowed", false),
            flag("blocked", matches!(case, FailureCase::Named)),
        ]),
    )])
}

fn sources(case: FailureCase) -> Vec<(String, Instance)> {
    vec![("rules".into(), rules(case))]
}

fn expected_output() -> Instance {
    Instance::Group(vec![(
        "Name".into(),
        Instance::Scalar(Value::String("mapped".into())),
    )])
}

#[test]
fn failure_rules_match_engine_and_generated_backends() -> TestResult<()> {
    let project = failure_project();
    assert!(engine::validate(&project).is_empty());
    let execution = engine::ExecutionContext::new(Path::new("failure-rules.ferrule"));
    let output = engine::run_outputs_with_sources_and_context(
        &project,
        &primary(FailureCase::None),
        sources(FailureCase::None),
        &execution,
    )?;
    assert_eq!(output.primary, expected_output());
    assert_eq!(output.extras[0].instance, expected_output());

    for (case, rule, message) in [
        (FailureCase::Source, 1, Some("invalid:B")),
        (FailureCase::Generated, 2, Some("2")),
        (FailureCase::Named, 3, Some("blocked")),
        (FailureCase::EmptyMessage, 4, Some("")),
    ] {
        let error = engine::run_outputs_with_sources_and_context(
            &project,
            &primary(case),
            sources(case),
            &execution,
        )
        .expect_err("selected rule must fail before outputs");
        assert_eq!(
            error,
            engine::EngineError::MappingFailure {
                rule,
                message: message.map(str::to_string),
            }
        );
    }
    let non_boolean = engine::run_outputs_with_sources_and_context(
        &project,
        &primary(FailureCase::NonBoolean),
        sources(FailureCase::NonBoolean),
        &execution,
    )
    .expect_err("non-boolean rule predicates must retain their typed error");
    assert_eq!(
        non_boolean,
        engine::EngineError::NotABool {
            node: 31,
            found: "string",
        }
    );

    let directory = TempDir::new("failure_rules")?;
    let project_path = directory.0.join("failure-rules.json");
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
        include_str!("fixtures/failure_rules_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust failure rules failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/failure_rules_csharp_harness.cs.txt"),
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
        "generated C# failure rules failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
