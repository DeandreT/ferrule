use super::*;
use mapping::{IterationOutput, SequenceExpr, SequenceWindow, SortFilterOrder};

fn parent_string_row(name: &str, repeating: bool) -> SchemaNode {
    let row = SchemaNode::group(
        name,
        vec![string("Value"), string("Parent"), int("Position")],
    );
    if repeating { row.repeating() } else { row }
}

fn parent_int_row(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![int("Value"), string("Parent"), int("Position")]).repeating()
}

fn simple_string_row(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![string("Value"), int("Position")]).repeating()
}

fn simple_int_row(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![int("Value"), int("Position")]).repeating()
}

fn nested_sequence_row() -> SchemaNode {
    SchemaNode::group(
        "Nested",
        vec![
            string("Value"),
            int("Position"),
            simple_string_row("Pieces"),
        ],
    )
    .repeating()
}

fn parent_bindings(item: u32) -> Vec<Binding> {
    vec![
        Binding {
            target_field: "Value".into(),
            node: item,
        },
        Binding {
            target_field: "Parent".into(),
            node: 10,
        },
        Binding {
            target_field: "Position".into(),
            node: 50,
        },
    ]
}

fn simple_bindings(item: u32) -> Vec<Binding> {
    vec![
        Binding {
            target_field: "Value".into(),
            node: item,
        },
        Binding {
            target_field: "Position".into(),
            node: 50,
        },
    ]
}

fn generated_sequence_project() -> Project {
    let parent = SchemaNode::group(
        "Parents",
        vec![
            string("Name"),
            string("Tokens"),
            string("Unicode"),
            string("Delimiter"),
            int("Length"),
            int("From"),
            int("To"),
        ],
    )
    .repeating();
    let parent_out = SchemaNode::group(
        "ParentOut",
        vec![
            string("Name"),
            parent_string_row("Controlled", true),
            parent_string_row("Chunks", true),
            parent_int_row("DefaultRange"),
            parent_int_row("ExplicitRange"),
            parent_string_row("First", false),
            parent_string_row("Mapped", false),
            nested_sequence_row(),
        ],
    )
    .repeating();

    Project {
        source: SchemaNode::group(
            "Source",
            vec![string("ErrorInput"), bool_("TriggerError"), parent],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                parent_out,
                simple_string_row("EmptyTokens"),
                simple_string_row("EmptyChunks"),
                simple_string_row("NullTokens"),
                simple_int_row("NullRange"),
                simple_string_row("ErrorCheck"),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: None,
                    },
                ),
                (
                    11,
                    Node::SourceField {
                        path: vec!["Tokens".into()],
                        frame: None,
                    },
                ),
                (
                    12,
                    Node::SourceField {
                        path: vec!["Unicode".into()],
                        frame: None,
                    },
                ),
                (
                    13,
                    Node::SourceField {
                        path: vec!["Delimiter".into()],
                        frame: None,
                    },
                ),
                (
                    14,
                    Node::SourceField {
                        path: vec!["Length".into()],
                        frame: None,
                    },
                ),
                (
                    15,
                    Node::SourceField {
                        path: vec!["From".into()],
                        frame: None,
                    },
                ),
                (
                    16,
                    Node::SourceField {
                        path: vec!["To".into()],
                        frame: None,
                    },
                ),
                (
                    20,
                    Node::Const {
                        value: Value::String(String::new()),
                    },
                ),
                (
                    21,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (22, Node::Const { value: Value::Null }),
                (
                    23,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    24,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    25,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![23, 24],
                    },
                ),
                (
                    26,
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (
                    27,
                    Node::SourceField {
                        path: vec!["ErrorInput".into()],
                        frame: None,
                    },
                ),
                (
                    28,
                    Node::SourceField {
                        path: vec!["TriggerError".into()],
                        frame: None,
                    },
                ),
                (
                    29,
                    Node::If {
                        condition: 28,
                        then: 25,
                        else_: 26,
                    },
                ),
                (
                    40,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    41,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    42,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    43,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    44,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    45,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    46,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    47,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    50,
                    Node::Position {
                        collection: Vec::new(),
                    },
                ),
                (
                    51,
                    Node::Call {
                        function: "not_equal".into(),
                        args: vec![40, 20],
                    },
                ),
                (
                    52,
                    Node::Call {
                        function: "not_equal".into(),
                        args: vec![45, 20],
                    },
                ),
                (
                    60,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    61,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    62,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    63,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    64,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![
                Scope {
                    target_field: "ParentOut".into(),
                    iteration: ScopeIteration::Source(vec!["Parents".into()]),
                    bindings: vec![Binding {
                        target_field: "Name".into(),
                        node: 10,
                    }],
                    children: vec![
                        Scope {
                            target_field: "Controlled".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                                input: 11,
                                delimiter: 13,
                                item: 40,
                            }),
                            filter: Some(51),
                            sort_by: Some(40),
                            sort_filter_order: SortFilterOrder::FilterThenSort,
                            windows: vec![
                                SequenceWindow::SkipFirst { count: 23 },
                                SequenceWindow::First { count: 21 },
                            ],
                            bindings: parent_bindings(40),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "Chunks".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::TokenizeByLength {
                                input: 12,
                                length: 14,
                                item: 41,
                            }),
                            bindings: parent_bindings(41),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "DefaultRange".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                                from: None,
                                to: 16,
                                item: 42,
                            }),
                            bindings: parent_bindings(42),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "ExplicitRange".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                                from: Some(15),
                                to: 16,
                                item: 43,
                            }),
                            bindings: parent_bindings(43),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "First".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                                input: 11,
                                delimiter: 13,
                                item: 44,
                            }),
                            iteration_output: IterationOutput::First,
                            bindings: parent_bindings(44),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "Mapped".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                                input: 11,
                                delimiter: 13,
                                item: 45,
                            }),
                            filter: Some(52),
                            iteration_output: IterationOutput::MappedSequence,
                            bindings: parent_bindings(45),
                            ..Scope::default()
                        },
                        Scope {
                            target_field: "Nested".into(),
                            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                                input: 11,
                                delimiter: 13,
                                item: 46,
                            }),
                            bindings: simple_bindings(46),
                            children: vec![Scope {
                                target_field: "Pieces".into(),
                                iteration: ScopeIteration::Sequence(
                                    SequenceExpr::TokenizeByLength {
                                        input: 46,
                                        length: 21,
                                        item: 47,
                                    },
                                ),
                                bindings: simple_bindings(47),
                                ..Scope::default()
                            }],
                            ..Scope::default()
                        },
                    ],
                    ..Scope::default()
                },
                Scope {
                    target_field: "EmptyTokens".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                        input: 20,
                        delimiter: 26,
                        item: 60,
                    }),
                    bindings: simple_bindings(60),
                    ..Scope::default()
                },
                Scope {
                    target_field: "EmptyChunks".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::TokenizeByLength {
                        input: 20,
                        length: 21,
                        item: 61,
                    }),
                    bindings: simple_bindings(61),
                    ..Scope::default()
                },
                Scope {
                    target_field: "NullTokens".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                        input: 22,
                        delimiter: 25,
                        item: 62,
                    }),
                    bindings: simple_bindings(62),
                    ..Scope::default()
                },
                Scope {
                    target_field: "NullRange".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                        from: Some(22),
                        to: 25,
                        item: 63,
                    }),
                    bindings: simple_bindings(63),
                    ..Scope::default()
                },
                Scope {
                    target_field: "ErrorCheck".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                        input: 27,
                        delimiter: 29,
                        item: 64,
                    }),
                    bindings: simple_bindings(64),
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        },
    }
}

fn generated_sequence_source(error_precedence: bool) -> Instance {
    let parent = |name: &str,
                  tokens: &str,
                  unicode: &str,
                  delimiter: &str,
                  length: i64,
                  from: i64,
                  to: i64| {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "Tokens".into(),
                Instance::Scalar(Value::String(tokens.into())),
            ),
            (
                "Unicode".into(),
                Instance::Scalar(Value::String(unicode.into())),
            ),
            (
                "Delimiter".into(),
                Instance::Scalar(Value::String(delimiter.into())),
            ),
            ("Length".into(), Instance::Scalar(Value::Int(length))),
            ("From".into(), Instance::Scalar(Value::Int(from))),
            ("To".into(), Instance::Scalar(Value::Int(to))),
        ])
    };

    Instance::Group(vec![
        (
            "ErrorInput".into(),
            Instance::Scalar(if error_precedence {
                Value::Int(7)
            } else {
                Value::String("left,right".into())
            }),
        ),
        (
            "TriggerError".into(),
            Instance::Scalar(Value::Bool(error_precedence)),
        ),
        (
            "Parents".into(),
            Instance::Repeated(vec![
                parent("Alpha", "delta,,alpha,beta,gamma", "aé🙂z", ",", 2, 2, 4),
                parent("Beta", "one|three|two", "", "|", 2, 4, 2),
            ]),
        ),
    ])
}

fn generated_sequence_expected() -> Instance {
    fn parent_string(value: &str, parent: &str, position: i64) -> Instance {
        Instance::Group(vec![
            (
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            ),
            (
                "Parent".into(),
                Instance::Scalar(Value::String(parent.into())),
            ),
            ("Position".into(), Instance::Scalar(Value::Int(position))),
        ])
    }
    fn parent_int(value: i64, parent: &str, position: i64) -> Instance {
        Instance::Group(vec![
            ("Value".into(), Instance::Scalar(Value::Int(value))),
            (
                "Parent".into(),
                Instance::Scalar(Value::String(parent.into())),
            ),
            ("Position".into(), Instance::Scalar(Value::Int(position))),
        ])
    }
    fn simple_string(value: &str, position: i64) -> Instance {
        Instance::Group(vec![
            (
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            ),
            ("Position".into(), Instance::Scalar(Value::Int(position))),
        ])
    }
    struct ExpectedParent<'a> {
        name: &'a str,
        controlled: &'a [&'a str],
        chunks: &'a [&'a str],
        default_range: &'a [i64],
        explicit_range: &'a [i64],
        first: &'a str,
        mapped: &'a [&'a str],
        nested: &'a [(&'a str, &'a [&'a str])],
    }
    fn parent_output(expected: &ExpectedParent<'_>) -> Instance {
        let name = expected.name;
        let strings = |values: &[&str]| {
            values
                .iter()
                .enumerate()
                .map(|(index, value)| parent_string(value, name, index as i64 + 1))
                .collect::<Vec<_>>()
        };
        let ints = |values: &[i64]| {
            values
                .iter()
                .enumerate()
                .map(|(index, value)| parent_int(*value, name, index as i64 + 1))
                .collect::<Vec<_>>()
        };
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "Controlled".into(),
                Instance::Repeated(strings(expected.controlled)),
            ),
            (
                "Chunks".into(),
                Instance::Repeated(strings(expected.chunks)),
            ),
            (
                "DefaultRange".into(),
                Instance::Repeated(ints(expected.default_range)),
            ),
            (
                "ExplicitRange".into(),
                Instance::Repeated(ints(expected.explicit_range)),
            ),
            ("First".into(), parent_string(expected.first, name, 1)),
            (
                "Mapped".into(),
                Instance::MappedSequence(strings(expected.mapped)),
            ),
            (
                "Nested".into(),
                Instance::Repeated(
                    expected
                        .nested
                        .iter()
                        .enumerate()
                        .map(|(outer_index, (value, pieces))| {
                            Instance::Group(vec![
                                (
                                    "Value".into(),
                                    Instance::Scalar(Value::String((*value).into())),
                                ),
                                (
                                    "Position".into(),
                                    Instance::Scalar(Value::Int(outer_index as i64 + 1)),
                                ),
                                (
                                    "Pieces".into(),
                                    Instance::Repeated(
                                        pieces
                                            .iter()
                                            .enumerate()
                                            .map(|(inner_index, piece)| {
                                                simple_string(piece, inner_index as i64 + 1)
                                            })
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }

    Instance::Group(vec![
        (
            "ParentOut".into(),
            Instance::Repeated(vec![
                parent_output(&ExpectedParent {
                    name: "Alpha",
                    controlled: &["beta", "delta"],
                    chunks: &["aé", "🙂z"],
                    default_range: &[1, 2, 3, 4],
                    explicit_range: &[2, 3, 4],
                    first: "delta",
                    mapped: &["delta", "alpha", "beta", "gamma"],
                    nested: &[
                        ("delta", &["de", "lt", "a"]),
                        ("", &[]),
                        ("alpha", &["al", "ph", "a"]),
                        ("beta", &["be", "ta"]),
                        ("gamma", &["ga", "mm", "a"]),
                    ],
                }),
                parent_output(&ExpectedParent {
                    name: "Beta",
                    controlled: &["three", "two"],
                    chunks: &[],
                    default_range: &[1, 2],
                    explicit_range: &[],
                    first: "one",
                    mapped: &["one", "three", "two"],
                    nested: &[
                        ("one", &["on", "e"]),
                        ("three", &["th", "re", "e"]),
                        ("two", &["tw", "o"]),
                    ],
                }),
            ]),
        ),
        (
            "EmptyTokens".into(),
            Instance::Repeated(vec![simple_string("", 1)]),
        ),
        ("EmptyChunks".into(), Instance::Repeated(Vec::new())),
        ("NullTokens".into(), Instance::Repeated(Vec::new())),
        ("NullRange".into(), Instance::Repeated(Vec::new())),
        (
            "ErrorCheck".into(),
            Instance::Repeated(vec![simple_string("left", 1), simple_string("right", 2)]),
        ),
    ])
}

fn write_generated_sequence_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("generated-sequence-project.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&generated_sequence_project())?,
    )?;
    Ok(path)
}

fn write_rust_harness(output: &Path) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/generated_sequences_rust_harness.rs.txt"),
    )?;
    Ok(())
}

fn write_csharp_harness(output: &Path) -> TestResult<()> {
    let harness = output.join("Harness");
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
        include_str!("fixtures/generated_sequences_csharp_harness.cs.txt"),
    )?;
    Ok(())
}

#[test]
fn generated_sequence_scopes_match_engine_and_generated_backends() -> TestResult<()> {
    let project = generated_sequence_project();
    assert_eq!(
        engine::run(&project, &generated_sequence_source(false))?,
        generated_sequence_expected()
    );
    let engine_error = engine::run(&project, &generated_sequence_source(true))
        .expect_err("the later delimiter expression must fail before input type coercion");
    assert!(matches!(engine_error, engine::EngineError::Function(_)));
    assert_eq!(engine_error.to_string(), "division by zero");

    let directory = TempDir::new("generated_sequences")?;
    let project_path = write_generated_sequence_project(&directory.0)?;

    let rust_output = directory.0.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    write_rust_harness(&rust_output)?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust sequence scopes failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    write_csharp_harness(&csharp_output)?;
    let csharp = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .output()?;
    assert!(
        csharp.status.success(),
        "generated C# sequence scopes failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
