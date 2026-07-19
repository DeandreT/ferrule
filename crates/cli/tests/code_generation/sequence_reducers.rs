use super::*;
use mapping::SequenceExpr;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ErrorMode {
    None,
    NonBool,
    EmptyIndex,
    ExistsSequence,
    ItemAtSequence,
}

fn reducer_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Words"),
                int("Index"),
                bool_("FailNonBool"),
                bool_("FailEmptyIndex"),
                bool_("FailExistsSequence"),
                bool_("FailItemAtSequence"),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                bool_("ExistsTrue"),
                bool_("ExistsFalse"),
                bool_("ExistsPosition"),
                bool_("ExistsShortCircuit"),
                bool_("NullSkipsPredicate"),
                bool_("EmptySkipsPredicate"),
                int("ItemAtOne"),
                string("ItemAtOutOfRange"),
                string("ItemAtParentIndex"),
                bool_("NonBoolProbe"),
                string("EmptyIndexProbe"),
                bool_("ExistsSequenceProbe"),
                string("ItemAtSequenceProbe"),
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
                    1,
                    Node::SourceField {
                        path: vec!["Words".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Index".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (4, Node::Const { value: Value::Null }),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![5, 6],
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    10,
                    Node::Const {
                        value: Value::String("beta".into()),
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
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    13,
                    Node::Const {
                        value: Value::Int(5),
                    },
                ),
                (
                    14,
                    Node::Const {
                        value: Value::Int(8),
                    },
                ),
                (
                    15,
                    Node::Const {
                        value: Value::Int(42),
                    },
                ),
                (
                    16,
                    Node::Const {
                        value: Value::String("safe".into()),
                    },
                ),
                (
                    17,
                    Node::Const {
                        value: Value::String("zeta".into()),
                    },
                ),
                (
                    18,
                    Node::Const {
                        value: Value::String("hit".into()),
                    },
                ),
                (
                    19,
                    Node::Const {
                        value: Value::String("hit,bad".into()),
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        path: vec!["FailNonBool".into()],
                        frame: None,
                    },
                ),
                (
                    21,
                    Node::SourceField {
                        path: vec!["FailEmptyIndex".into()],
                        frame: None,
                    },
                ),
                (
                    22,
                    Node::SourceField {
                        path: vec!["FailExistsSequence".into()],
                        frame: None,
                    },
                ),
                (
                    23,
                    Node::SourceField {
                        path: vec!["FailItemAtSequence".into()],
                        frame: None,
                    },
                ),
                (
                    24,
                    Node::Const {
                        value: Value::String("not bool".into()),
                    },
                ),
                (
                    100,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    101,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![100, 10],
                    },
                ),
                (
                    102,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Tokenize {
                            input: 1,
                            delimiter: 3,
                            item: 100,
                        },
                        predicate: 101,
                    },
                ),
                (
                    110,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    111,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![110, 17],
                    },
                ),
                (
                    112,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Tokenize {
                            input: 1,
                            delimiter: 3,
                            item: 110,
                        },
                        predicate: 111,
                    },
                ),
                (
                    120,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    121,
                    Node::Position {
                        collection: Vec::new(),
                    },
                ),
                (
                    122,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![121, 12],
                    },
                ),
                (
                    123,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 11,
                            item: 120,
                        },
                        predicate: 122,
                    },
                ),
                (
                    130,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    131,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![130, 18],
                    },
                ),
                (
                    132,
                    Node::If {
                        condition: 131,
                        then: 8,
                        else_: 7,
                    },
                ),
                (
                    133,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Tokenize {
                            input: 19,
                            delimiter: 3,
                            item: 130,
                        },
                        predicate: 132,
                    },
                ),
                (
                    140,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    141,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Tokenize {
                            input: 4,
                            delimiter: 3,
                            item: 140,
                        },
                        predicate: 7,
                    },
                ),
                (
                    150,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    151,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Generate {
                            from: Some(12),
                            to: 5,
                            item: 150,
                        },
                        predicate: 7,
                    },
                ),
                (
                    200,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    201,
                    Node::SequenceItemAt {
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 13,
                            item: 200,
                        },
                        index: 11,
                    },
                ),
                (
                    210,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    211,
                    Node::SequenceItemAt {
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 13,
                            item: 210,
                        },
                        index: 14,
                    },
                ),
                (
                    220,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    221,
                    Node::SequenceItemAt {
                        sequence: SequenceExpr::Tokenize {
                            input: 1,
                            delimiter: 3,
                            item: 220,
                        },
                        index: 2,
                    },
                ),
                (
                    300,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    302,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 5,
                            item: 300,
                        },
                        predicate: 24,
                    },
                ),
                (
                    303,
                    Node::If {
                        condition: 20,
                        then: 302,
                        else_: 8,
                    },
                ),
                (
                    310,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    311,
                    Node::SequenceItemAt {
                        sequence: SequenceExpr::Generate {
                            from: Some(12),
                            to: 5,
                            item: 310,
                        },
                        index: 7,
                    },
                ),
                (
                    312,
                    Node::If {
                        condition: 21,
                        then: 311,
                        else_: 16,
                    },
                ),
                (
                    320,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    321,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Tokenize {
                            input: 15,
                            delimiter: 3,
                            item: 320,
                        },
                        predicate: 7,
                    },
                ),
                (
                    322,
                    Node::If {
                        condition: 22,
                        then: 321,
                        else_: 8,
                    },
                ),
                (
                    330,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    331,
                    Node::SequenceItemAt {
                        sequence: SequenceExpr::Tokenize {
                            input: 15,
                            delimiter: 3,
                            item: 330,
                        },
                        index: 7,
                    },
                ),
                (
                    332,
                    Node::If {
                        condition: 23,
                        then: 331,
                        else_: 16,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: [
                ("ExistsTrue", 102),
                ("ExistsFalse", 112),
                ("ExistsPosition", 123),
                ("ExistsShortCircuit", 133),
                ("NullSkipsPredicate", 141),
                ("EmptySkipsPredicate", 151),
                ("ItemAtOne", 201),
                ("ItemAtOutOfRange", 211),
                ("ItemAtParentIndex", 221),
                ("NonBoolProbe", 303),
                ("EmptyIndexProbe", 312),
                ("ExistsSequenceProbe", 322),
                ("ItemAtSequenceProbe", 332),
            ]
            .into_iter()
            .map(|(target_field, node)| Binding {
                target_field: target_field.into(),
                node,
            })
            .collect(),
            ..Scope::default()
        },
    }
}

fn reducer_source(mode: ErrorMode) -> Instance {
    Instance::Group(vec![
        (
            "Words".into(),
            Instance::Scalar(Value::String("alpha,beta,gamma".into())),
        ),
        ("Index".into(), Instance::Scalar(Value::Int(2))),
        (
            "FailNonBool".into(),
            Instance::Scalar(Value::Bool(mode == ErrorMode::NonBool)),
        ),
        (
            "FailEmptyIndex".into(),
            Instance::Scalar(Value::Bool(mode == ErrorMode::EmptyIndex)),
        ),
        (
            "FailExistsSequence".into(),
            Instance::Scalar(Value::Bool(mode == ErrorMode::ExistsSequence)),
        ),
        (
            "FailItemAtSequence".into(),
            Instance::Scalar(Value::Bool(mode == ErrorMode::ItemAtSequence)),
        ),
    ])
}

fn reducer_expected() -> Instance {
    Instance::Group(vec![
        ("ExistsTrue".into(), Instance::Scalar(Value::Bool(true))),
        ("ExistsFalse".into(), Instance::Scalar(Value::Bool(false))),
        ("ExistsPosition".into(), Instance::Scalar(Value::Bool(true))),
        (
            "ExistsShortCircuit".into(),
            Instance::Scalar(Value::Bool(true)),
        ),
        (
            "NullSkipsPredicate".into(),
            Instance::Scalar(Value::Bool(false)),
        ),
        (
            "EmptySkipsPredicate".into(),
            Instance::Scalar(Value::Bool(false)),
        ),
        ("ItemAtOne".into(), Instance::Scalar(Value::Int(3))),
        ("ItemAtOutOfRange".into(), Instance::Scalar(Value::Null)),
        (
            "ItemAtParentIndex".into(),
            Instance::Scalar(Value::String("beta".into())),
        ),
        ("NonBoolProbe".into(), Instance::Scalar(Value::Bool(true))),
        (
            "EmptyIndexProbe".into(),
            Instance::Scalar(Value::String("safe".into())),
        ),
        (
            "ExistsSequenceProbe".into(),
            Instance::Scalar(Value::Bool(true)),
        ),
        (
            "ItemAtSequenceProbe".into(),
            Instance::Scalar(Value::String("safe".into())),
        ),
    ])
}

fn write_reducer_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("sequence-reducers-project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&reducer_project())?)?;
    Ok(path)
}

fn write_rust_harness(output: &Path) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/sequence_reducers_rust_harness.rs.txt"),
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
        include_str!("fixtures/sequence_reducers_csharp_harness.cs.txt"),
    )?;
    Ok(())
}

#[test]
fn sequence_reducers_match_engine_and_generated_backends() -> TestResult<()> {
    let project = reducer_project();
    assert_eq!(
        engine::run(&project, &reducer_source(ErrorMode::None))?,
        reducer_expected()
    );
    assert_eq!(
        engine::run(&project, &reducer_source(ErrorMode::NonBool)),
        Err(engine::EngineError::NotABool {
            node: 24,
            found: "string",
        })
    );
    let empty_index = engine::run(&project, &reducer_source(ErrorMode::EmptyIndex))
        .expect_err("an empty sequence must still evaluate its item-at index");
    assert!(matches!(empty_index, engine::EngineError::Function(_)));
    assert_eq!(empty_index.to_string(), "division by zero");
    for mode in [ErrorMode::ExistsSequence, ErrorMode::ItemAtSequence] {
        let sequence = engine::run(&project, &reducer_source(mode))
            .expect_err("sequence generation must fail before predicate or index evaluation");
        assert!(matches!(sequence, engine::EngineError::Function(_)));
        assert_eq!(
            sequence.to_string(),
            "`tokenize` cannot accept a int argument"
        );
    }

    let directory = TempDir::new("sequence_reducers")?;
    let project_path = write_reducer_project(&directory.0)?;

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
        "generated Rust sequence reducers failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    write_csharp_harness(&csharp_output)?;
    let csharp = Command::new("dotnet")
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
        "generated C# sequence reducers failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
