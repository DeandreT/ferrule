use super::*;
use mapping::{AggregateOp, SequenceWindow, SortFilterOrder};

fn grouping_output(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("First"),
            string("Joined"),
            int("Position"),
            SchemaNode::group("Member", vec![string("Label")]).repeating(),
        ],
    )
    .repeating()
}

fn grouping_project() -> Project {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            string("Category"),
            string("Label"),
            bool_("Keep"),
            bool_("Start"),
            int("Priority"),
        ],
    )
    .repeating();
    let member = || Scope {
        target_field: "Member".into(),
        iteration: ScopeIteration::Source(Vec::new()),
        bindings: vec![Binding {
            target_field: "Label".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    let bindings = || {
        vec![
            Binding {
                target_field: "First".into(),
                node: 1,
            },
            Binding {
                target_field: "Joined".into(),
                node: 6,
            },
            Binding {
                target_field: "Position".into(),
                node: 7,
            },
        ]
    };

    Project {
        source: SchemaNode::group("Source", vec![string("BlockSize"), rows]),
        target: SchemaNode::group(
            "Target",
            vec![
                grouping_output("By"),
                grouping_output("Starting"),
                grouping_output("Block"),
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
                    0,
                    Node::SourceField {
                        path: vec!["Category".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Label".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Keep".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Start".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Priority".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (
                    6,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Rows".into()],
                        value: vec!["Label".into()],
                        expression: None,
                        arg: Some(5),
                    },
                ),
                (
                    7,
                    Node::Position {
                        collection: vec!["Rows".into()],
                    },
                ),
                (
                    8,
                    Node::SourceField {
                        path: vec!["BlockSize".into()],
                        frame: None,
                    },
                ),
                (
                    9,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    10,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![
                Scope {
                    target_field: "By".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into()]),
                    filter: Some(2),
                    group_by: Some(0),
                    sort_by: Some(4),
                    sort_filter_order: SortFilterOrder::FilterThenSort,
                    windows: vec![SequenceWindow::First { count: 9 }],
                    bindings: bindings(),
                    children: vec![member()],
                    ..Scope::default()
                },
                Scope {
                    target_field: "Starting".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into()]),
                    filter: Some(2),
                    group_starting_with: Some(3),
                    windows: vec![
                        SequenceWindow::SkipFirst { count: 10 },
                        SequenceWindow::First { count: 9 },
                    ],
                    bindings: bindings(),
                    children: vec![member()],
                    ..Scope::default()
                },
                Scope {
                    target_field: "Block".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into()]),
                    filter: Some(2),
                    group_into_blocks: Some(8),
                    windows: vec![SequenceWindow::Last { count: 9 }],
                    bindings: bindings(),
                    children: vec![member()],
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        },
    }
}

fn source_row(category: &str, label: &str, keep: bool, start: bool, priority: i64) -> Instance {
    Instance::Group(vec![
        (
            "Category".into(),
            Instance::Scalar(Value::String(category.into())),
        ),
        (
            "Label".into(),
            Instance::Scalar(Value::String(label.into())),
        ),
        ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
        ("Start".into(), Instance::Scalar(Value::Bool(start))),
        ("Priority".into(), Instance::Scalar(Value::Int(priority))),
    ])
}

fn grouping_source(block_size: Value) -> Instance {
    Instance::Group(vec![
        ("BlockSize".into(), Instance::Scalar(block_size)),
        (
            "Rows".into(),
            Instance::Repeated(vec![
                source_row("A", "alpha", true, false, 4),
                source_row("B", "bravo", false, true, 7),
                source_row("A", "amber", true, false, 2),
                source_row("B", "beta", true, true, 5),
                source_row("A", "apex", true, true, 1),
                source_row("B", "birch", true, false, 3),
                source_row("C", "cedar", true, true, 6),
            ]),
        ),
    ])
}

fn output_group(first: &str, joined: &str, position: i64, members: &[&str]) -> Instance {
    Instance::Group(vec![
        (
            "First".into(),
            Instance::Scalar(Value::String(first.into())),
        ),
        (
            "Joined".into(),
            Instance::Scalar(Value::String(joined.into())),
        ),
        ("Position".into(), Instance::Scalar(Value::Int(position))),
        (
            "Member".into(),
            Instance::Repeated(
                members
                    .iter()
                    .map(|label| {
                        Instance::Group(vec![(
                            "Label".into(),
                            Instance::Scalar(Value::String((*label).into())),
                        )])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn grouping_expected() -> Instance {
    Instance::Group(vec![
        (
            "By".into(),
            Instance::Repeated(vec![
                output_group("apex", "apex,amber,alpha", 1, &["apex", "amber", "alpha"]),
                output_group("birch", "birch,beta", 2, &["birch", "beta"]),
            ]),
        ),
        (
            "Starting".into(),
            Instance::Repeated(vec![
                output_group("beta", "beta", 1, &["beta"]),
                output_group("apex", "apex,birch", 2, &["apex", "birch"]),
            ]),
        ),
        (
            "Block".into(),
            Instance::Repeated(vec![
                output_group("beta", "beta,apex", 1, &["beta", "apex"]),
                output_group("birch", "birch,cedar", 2, &["birch", "cedar"]),
            ]),
        ),
    ])
}

fn write_grouping_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("grouping-project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&grouping_project())?)?;
    Ok(path)
}

fn write_rust_harness(output: &Path) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/grouping_rust_harness.rs.txt"),
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
        include_str!("fixtures/grouping_csharp_harness.cs.txt"),
    )?;
    Ok(())
}

#[test]
fn grouping_matches_engine_and_generated_backends() -> TestResult<()> {
    let project = grouping_project();
    assert_eq!(
        engine::run(&project, &grouping_source(Value::String("2".into())))?,
        grouping_expected()
    );
    assert_eq!(
        engine::run(&project, &grouping_source(Value::Int(0))),
        Err(engine::EngineError::InvalidBlockSize { node: 8 })
    );
    assert_eq!(
        engine::run(&project, &grouping_source(Value::Bool(true))),
        Err(engine::EngineError::NotAnItemCount {
            node: 8,
            found: "bool",
        })
    );

    let directory = TempDir::new("grouping")?;
    let project_path = write_grouping_project(&directory.0)?;
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
        "generated Rust grouping failed:\nstdout:\n{}\nstderr:\n{}",
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
        "generated C# grouping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
