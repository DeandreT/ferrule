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
        user_functions: Default::default(),
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
        .then(|| Instance::Group((Vec::new()).into()))
        .into_iter()
        .collect();
    let bad = matches!(case, FailureCase::NonBoolean)
        .then(|| {
            Instance::Group(
                (vec![(
                    "Value".into(),
                    Instance::Scalar(Value::String("not-bool".into())),
                )])
                .into(),
            )
        })
        .into_iter()
        .collect();
    Instance::Group(
        (vec![
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
        .into(),
    )
}

fn row(code: &str, valid: bool) -> Instance {
    Instance::Group(
        (vec![
            ("Code".into(), Instance::Scalar(Value::String(code.into()))),
            ("Valid".into(), Instance::Scalar(Value::Bool(valid))),
        ])
        .into(),
    )
}

fn rules(case: FailureCase) -> Instance {
    let flag = |code: &str, reject| {
        Instance::Group(
            (vec![
                ("Code".into(), Instance::Scalar(Value::String(code.into()))),
                ("Reject".into(), Instance::Scalar(Value::Bool(reject))),
            ])
            .into(),
        )
    };
    Instance::Group(
        (vec![(
            "Flag".into(),
            Instance::Repeated(vec![
                flag("allowed", false),
                flag("blocked", matches!(case, FailureCase::Named)),
            ]),
        )])
        .into(),
    )
}

fn sources(case: FailureCase) -> Vec<(String, Instance)> {
    vec![("rules".into(), rules(case))]
}

fn expected_output() -> Instance {
    Instance::Group(
        (vec![(
            "Name".into(),
            Instance::Scalar(Value::String("mapped".into())),
        )])
        .into(),
    )
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
    let mut rust_command = Command::new("cargo");
    rust_command
        .args(["run", "--quiet"])
        .current_dir(&rust_output);
    let rust = host_policy::recorded_output(&mut rust_command, &directory.0, "rust-host")?;
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
    let mut csharp_command = dotnet_command(&csharp_output);
    csharp_command
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output);
    let csharp = host_policy::recorded_output(&mut csharp_command, &directory.0, "csharp-host")?;
    assert!(
        csharp.status.success(),
        "generated C# failure rules failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum TokenizerCapKind {
    FixedLength,
    Literal,
}

impl TokenizerCapKind {
    fn name(self) -> &'static str {
        match self {
            Self::FixedLength => "fixed",
            Self::Literal => "literal",
        }
    }

    fn case_enabled(self, name: &str) -> bool {
        !matches!(
            (self, name),
            (
                Self::Literal,
                "empty-skips-message" | "empty-reaches-target"
            ) | (Self::FixedLength, "empty-selects-empty-item")
        )
    }

    fn input(self, large: bool) -> String {
        match (self, large) {
            (Self::FixedLength, true) => "x".repeat(2_000_000) + "🙂",
            (Self::Literal, true) => ",".repeat(1_000_000) + "🙂",
            (Self::FixedLength, false) => "e\u{301}🙂z!".into(),
            (Self::Literal, false) => "e\u{301},🙂,tail".into(),
        }
    }
}

// These controls reach later errors with small inputs; the large cases must
// instead fail while materializing the pre-target generated sequence.
const TOKENIZER_CAP_CASES: [(&str, bool, bool, bool); 10] = [
    ("cap-before-mapping-failure", false, false, true),
    ("cap-before-message", false, true, true),
    ("selected-before-target", false, false, true),
    ("selected-message-error", false, true, true),
    ("null-skips-argument", true, true, false),
    ("null-reaches-target", true, true, true),
    ("empty-selects-empty-item", false, false, true),
    ("empty-evaluates-argument", true, true, false),
    ("empty-skips-message", false, true, false),
    ("empty-reaches-target", false, true, true),
];

fn tokenizer_cap_project(kind: TokenizerCapKind) -> Project {
    let source_field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: None,
    };
    let sequence = match kind {
        TokenizerCapKind::FixedLength => SequenceExpr::TokenizeByLength {
            input: 1,
            length: 5,
            item: 10,
        },
        TokenizerCapKind::Literal => SequenceExpr::Tokenize {
            input: 1,
            delimiter: 5,
            item: 10,
        },
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Text"),
                bool_("FailArgument"),
                bool_("FailMessage"),
                bool_("FailTarget"),
            ],
        ),
        target: SchemaNode::group("Target", vec![string("Output")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: Default::default(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Sequence { sequence },
            selection: FailureSelection::All,
            message: Some(12),
        }],
        graph: Graph {
            nodes: BTreeMap::from([
                (1, source_field("Text")),
                (2, source_field("FailArgument")),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    5,
                    Node::If {
                        condition: 2,
                        then: 6,
                        else_: 7,
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![3, 4],
                    },
                ),
                (
                    7,
                    Node::Const {
                        value: match kind {
                            TokenizerCapKind::FixedLength => Value::Int(2),
                            TokenizerCapKind::Literal => Value::String(",".into()),
                        },
                    },
                ),
                (
                    10,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (11, source_field("FailMessage")),
                (
                    12,
                    Node::If {
                        condition: 11,
                        then: 6,
                        else_: 10,
                    },
                ),
                (20, source_field("FailTarget")),
                (
                    21,
                    Node::If {
                        condition: 20,
                        then: 6,
                        else_: 22,
                    },
                ),
                (
                    22,
                    Node::Const {
                        value: Value::String("ok".into()),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Output".into(),
                node: 21,
            }],
            ..Scope::default()
        },
    }
}

struct TokenizerCapDirectory {
    path: PathBuf,
    complete: bool,
}

impl TokenizerCapDirectory {
    fn new(kind: TokenizerCapKind) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time follows the epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-codegen-tokenizer-cap-{}-{}-{nonce}-{}",
            kind.name(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            complete: false,
        })
    }
}

impl Drop for TokenizerCapDirectory {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "retained generated tokenizer-cap evidence: {}",
                self.path.display()
            );
        }
    }
}

fn recorded_tokenizer_cap_command(
    command: &mut Command,
    directory: &Path,
    name: &str,
) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = match command.isolated_output() {
        Ok(output) => output,
        Err(error) => {
            std::fs::write(
                directory.join(format!("{name}-spawn-error.txt")),
                format!("{error:?}\n"),
            )?;
            return Err(error);
        }
    };
    std::fs::write(directory.join(format!("{name}-stdout.txt")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.txt")), &output.stderr)?;
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt as _;
        output.status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec(&serde_json::json!({
            "success": output.status.success(), "code": output.status.code(), "signal": signal,
        }))?,
    )?;
    Ok(output)
}

fn tokenizer_cap_native_oracles(
    project: &Project,
    kind: TokenizerCapKind,
    directory: &Path,
) -> TestResult<()> {
    use std::error::Error as _;
    use std::io::Write as _;

    std::fs::create_dir(directory)?;
    for (name, fail_argument, fail_message, fail_target) in TOKENIZER_CAP_CASES {
        if !kind.case_enabled(name) {
            continue;
        }
        let text = if name.starts_with("null-") {
            Value::Null
        } else if name.starts_with("empty-") {
            Value::String(String::new())
        } else {
            Value::String(kind.input(name.starts_with("cap-")))
        };
        let input_kind = match &text {
            Value::Null => "Null",
            Value::String(_) => "String",
            _ => unreachable!("fixed fixture domains"),
        };
        let input_text = match &text {
            Value::String(value) => value.as_bytes(),
            _ => &[],
        };
        std::fs::write(directory.join(format!("{name}-input.utf8")), input_text)?;
        std::fs::write(
            directory.join(format!("{name}-input-fields.txt")),
            format!(
                "kind={input_kind}\nFailArgument={fail_argument}\nFailMessage={fail_message}\nFailTarget={fail_target}\n"
            ),
        )?;
        let source = codegen_runtime::group([
            codegen_runtime::field("Text", Instance::Scalar(text)),
            codegen_runtime::field("FailArgument", Instance::Scalar(Value::Bool(fail_argument))),
            codegen_runtime::field("FailMessage", Instance::Scalar(Value::Bool(fail_message))),
            codegen_runtime::field("FailTarget", Instance::Scalar(Value::Bool(fail_target))),
        ]);
        serde_json::to_writer(
            std::fs::File::create(directory.join(format!("{name}-input.json")))?,
            &source,
        )?;
        let result = engine::run(project, &source);
        let mut original = std::fs::File::create(directory.join(format!("{name}-outcome.txt")))?;
        writeln!(original, "{result:#?}")?;
        match &result {
            Ok(output) => serde_json::to_writer(
                std::fs::File::create(directory.join(format!("{name}-output.json")))?,
                output,
            )?,
            Err(error) => {
                writeln!(original, "Display: {error}")?;
                let mut cause = error.source();
                while let Some(error) = cause {
                    writeln!(original, "Cause Debug: {error:?}\nCause Display: {error}")?;
                    cause = error.source();
                }
            }
        }
        original.flush()?;
        let expected = if name.starts_with("cap-") {
            Err(engine::EngineError::GeneratedSequenceTooLarge {
                requested: 1_000_001,
                max: 1_000_000,
            })
        } else if matches!(name, "selected-before-target" | "empty-selects-empty-item") {
            Err(engine::EngineError::MappingFailure {
                rule: 1,
                message: Some(
                    if name == "selected-before-target" {
                        "e\u{301}"
                    } else {
                        ""
                    }
                    .into(),
                ),
            })
        } else if matches!(name, "null-skips-argument" | "empty-skips-message") {
            Ok(codegen_runtime::group([codegen_runtime::field(
                "Output",
                Instance::Scalar(Value::String("ok".into())),
            )]))
        } else {
            Err(engine::EngineError::Function(
                codegen_runtime::FunctionError::DivideByZero,
            ))
        };
        assert_eq!(
            result,
            expected,
            "native {}/{name}; complete originals retained",
            kind.name()
        );
        // The generated hosts compare their typed observations to this actual,
        // independently asserted native outcome rather than a second expected table.
        let semantic = match &result {
            Err(engine::EngineError::GeneratedSequenceTooLarge { requested, max }) => {
                format!("GeneratedSequenceTooLarge\nrequested={requested}\nmaximum={max}\n")
            }
            Err(engine::EngineError::MappingFailure {
                rule,
                message: Some(message),
            }) => format!("MappingFailure\nrule={rule}\nmessage={message}\n"),
            Err(engine::EngineError::Function(codegen_runtime::FunctionError::DivideByZero)) => {
                "Function.DivideByZero\n".into()
            }
            Ok(_) => "Success\nOutput=ok\n".into(),
            _ => unreachable!("independently asserted native outcome"),
        };
        std::fs::write(directory.join(format!("{name}-oracle.txt")), semantic)?;
    }
    Ok(())
}

fn generated_tokenizer_cap_matches_native(kind: TokenizerCapKind) -> TestResult<()> {
    let mut directory = TokenizerCapDirectory::new(kind)?;
    let project = tokenizer_cap_project(kind);
    let project_path = directory.path.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let issues = engine::validate(&project);
    std::fs::write(
        directory.path.join("validation.txt"),
        format!("{issues:#?}\n"),
    )?;
    assert!(
        issues.is_empty(),
        "whole fixture must validate before execution"
    );
    let native = directory.path.join("native");
    tokenizer_cap_native_oracles(&project, kind, &native)?;

    let rust_output = directory.path.join("rust");
    let generated = generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    );
    std::fs::write(
        directory.path.join("rust-generation.txt"),
        format!("{generated:#?}\n"),
    )?;
    generated?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/fixed_length_cap_rust_harness.rs.txt"),
    )?;
    let host_target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(target) => {
            let target = PathBuf::from(target);
            if target.is_absolute() {
                target
            } else {
                std::env::current_dir()?.join(target)
            }
        }
        None => directory.path.join("cargo-target"),
    };
    let mut rust_command = Command::new("cargo");
    rust_command
        .args(["run", "--quiet", "--jobs", "1", "--", kind.name()])
        .arg(&native)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", host_target)
        .env("CARGO_INCREMENTAL", "0");
    let rust = recorded_tokenizer_cap_command(&mut rust_command, &rust_output, "host")?;
    assert!(
        rust.status.success(),
        "generated Rust tokenizer cap failed; originals at {}",
        directory.path.display()
    );
    assert!(
        String::from_utf8_lossy(&rust.stdout).contains("generated tokenizer cap checks passed")
    );

    let csharp_output = directory.path.join("csharp");
    let generated = generate_project(&project_path, &csharp_output, GenerateTarget::CSharp);
    std::fs::write(
        directory.path.join("csharp-generation.txt"),
        format!("{generated:#?}\n"),
    )?;
    generated?;
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
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/fixed_length_cap_csharp_harness.cs.txt"),
    )?;
    let mut csharp_command = dotnet_command(&csharp_output);
    csharp_command
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--",
            kind.name(),
        ])
        .arg(&native)
        .current_dir(&csharp_output);
    let csharp = recorded_tokenizer_cap_command(&mut csharp_command, &csharp_output, "host")?;
    assert!(
        csharp.status.success(),
        "generated C# tokenizer cap failed; originals at {}",
        directory.path.display()
    );
    assert!(
        String::from_utf8_lossy(&csharp.stdout).contains("generated tokenizer cap checks passed")
    );
    for output in [&rust_output, &csharp_output] {
        for (name, ..) in TOKENIZER_CAP_CASES {
            if !kind.case_enabled(name) {
                continue;
            }
            for suffix in ["input.utf8", "input-fields.txt", "oracle.txt"] {
                let filename = format!("{name}-{suffix}");
                assert_eq!(
                    std::fs::read(output.join("evidence").join(&filename))?,
                    std::fs::read(native.join(&filename))?,
                    "complete generated {} differs for {filename}",
                    output.display()
                );
            }
        }
    }
    directory.complete = true;
    Ok(())
}

#[test]
fn fixed_length_pre_target_cap_and_null_laziness_match_generated_backends() -> TestResult<()> {
    generated_tokenizer_cap_matches_native(TokenizerCapKind::FixedLength)
}

#[test]
fn literal_pre_target_cap_and_null_laziness_match_generated_backends() -> TestResult<()> {
    generated_tokenizer_cap_matches_native(TokenizerCapKind::Literal)
}
