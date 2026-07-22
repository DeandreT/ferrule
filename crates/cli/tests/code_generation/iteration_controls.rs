use super::*;
use mapping::{IterationOutput, SequenceWindow, SortFilterOrder, SortKey};

fn output_row(name: &str, repeating: bool) -> SchemaNode {
    let row = SchemaNode::group(
        name,
        vec![string("Label"), int("ParentPosition"), int("ItemPosition")],
    );
    if repeating { row.repeating() } else { row }
}

fn controls_project() -> Project {
    let source_row = SchemaNode::group(
        "Rows",
        vec![
            string("Label"),
            int("Primary"),
            string("Secondary"),
            bool_("Keep"),
            int("Divisor"),
        ],
    )
    .repeating();
    let source_parent =
        SchemaNode::group("Parents", vec![string("Name"), string("Bound"), source_row]).repeating();
    let target_parent = SchemaNode::group(
        "ParentOut",
        vec![
            string("Parent"),
            output_row("SortThenFilter", true),
            output_row("FilterThenSort", true),
            output_row("Windows", true),
            output_row("First", false),
            output_row("EmptyFirst", false),
            output_row("Mapped", false),
            SchemaNode::group("LazyFirst", vec![int("Value")]),
        ],
    )
    .repeating();

    let row_bindings = || {
        vec![
            Binding {
                target_field: "Label".into(),
                node: 12,
            },
            Binding {
                target_field: "ParentPosition".into(),
                node: 11,
            },
            Binding {
                target_field: "ItemPosition".into(),
                node: 16,
            },
        ]
    };
    let sorted = |target_field: &str, order| Scope {
        target_field: target_field.into(),
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        filter: Some(19),
        sort_by: Some(13),
        sort_descending: true,
        sort_then_by: vec![SortKey {
            node: 14,
            descending: false,
        }],
        sort_filter_order: order,
        bindings: row_bindings(),
        ..Scope::default()
    };

    Project {
        source: SchemaNode::group("Source", vec![source_parent]),
        target: SchemaNode::group("Target", vec![target_parent]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    11,
                    Node::Position {
                        collection: vec!["Parents".into()],
                    },
                ),
                (
                    12,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into(), "Rows".into()]),
                        path: vec!["Label".into()],
                    },
                ),
                (
                    13,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into(), "Rows".into()]),
                        path: vec!["Primary".into()],
                    },
                ),
                (
                    14,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into(), "Rows".into()]),
                        path: vec!["Secondary".into()],
                    },
                ),
                (
                    15,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into(), "Rows".into()]),
                        path: vec!["Keep".into()],
                    },
                ),
                (
                    16,
                    Node::Position {
                        collection: vec!["Rows".into()],
                    },
                ),
                (
                    17,
                    Node::Const {
                        value: Value::Int(4),
                    },
                ),
                (
                    18,
                    Node::Call {
                        function: "less_or_equal".into(),
                        args: vec![16, 17],
                    },
                ),
                (
                    19,
                    Node::Call {
                        function: "and".into(),
                        args: vec![15, 18],
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into()]),
                        path: vec!["Bound".into()],
                    },
                ),
                (
                    21,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    22,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    23,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    24,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    25,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
                (
                    26,
                    Node::SourceField {
                        frame: Some(vec!["Parents".into(), "Rows".into()]),
                        path: vec!["Divisor".into()],
                    },
                ),
                (
                    27,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    28,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![27, 26],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "ParentOut".into(),
                iteration: ScopeIteration::Source(vec!["Parents".into()]),
                bindings: vec![Binding {
                    target_field: "Parent".into(),
                    node: 10,
                }],
                children: vec![
                    sorted("SortThenFilter", SortFilterOrder::SortThenFilter),
                    sorted("FilterThenSort", SortFilterOrder::FilterThenSort),
                    Scope {
                        target_field: "Windows".into(),
                        iteration: ScopeIteration::Source(vec!["Rows".into()]),
                        windows: vec![
                            SequenceWindow::SkipFirst { count: 20 },
                            SequenceWindow::First { count: 21 },
                            SequenceWindow::From { position: 22 },
                            SequenceWindow::FromTo {
                                first: 23,
                                last: 24,
                            },
                            SequenceWindow::Last { count: 22 },
                        ],
                        bindings: row_bindings(),
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "First".into(),
                        iteration: ScopeIteration::Source(vec!["Rows".into()]),
                        filter: Some(15),
                        iteration_output: IterationOutput::First,
                        bindings: row_bindings(),
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "EmptyFirst".into(),
                        iteration: ScopeIteration::Source(vec!["Rows".into()]),
                        filter: Some(25),
                        iteration_output: IterationOutput::First,
                        bindings: row_bindings(),
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "Mapped".into(),
                        iteration: ScopeIteration::Source(vec!["Rows".into()]),
                        filter: Some(15),
                        windows: vec![SequenceWindow::First { count: 22 }],
                        iteration_output: IterationOutput::MappedSequence,
                        bindings: row_bindings(),
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "LazyFirst".into(),
                        iteration: ScopeIteration::Source(vec!["Rows".into()]),
                        iteration_output: IterationOutput::First,
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 28,
                        }],
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn controls_source(invalid_bound: bool) -> Instance {
    let row = |label: String, primary, secondary: &str, keep, divisor| {
        Instance::Group(vec![
            ("Label".into(), Instance::Scalar(Value::String(label))),
            ("Primary".into(), Instance::Scalar(Value::Int(primary))),
            (
                "Secondary".into(),
                Instance::Scalar(Value::String(secondary.into())),
            ),
            ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
            ("Divisor".into(), Instance::Scalar(Value::Int(divisor))),
        ])
    };
    let parent = |name: &str, bound: Value| {
        let rows = [
            ("A", 1, "b", true, 1),
            ("B", 3, "z", true, 0),
            ("C", 3, "a", true, 1),
            ("D", 3, "a", true, 1),
            ("E", 2, "x", true, 1),
            ("F", 4, "m", false, 1),
            ("G", 0, "q", true, 1),
            ("H", 5, "r", true, 1),
        ];
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ("Bound".into(), Instance::Scalar(bound)),
            (
                "Rows".into(),
                Instance::Repeated(
                    rows.into_iter()
                        .map(|(label, primary, secondary, keep, divisor)| {
                            row(format!("{name}-{label}"), primary, secondary, keep, divisor)
                        })
                        .collect(),
                ),
            ),
        ])
    };

    Instance::Group(vec![(
        "Parents".into(),
        Instance::Repeated(vec![
            parent(
                "P1",
                if invalid_bound {
                    Value::Bool(true)
                } else {
                    Value::String("1".into())
                },
            ),
            parent("P2", Value::Float(2.9)),
        ]),
    )])
}

fn controls_expected() -> Instance {
    let row = |label: &str, parent_position, item_position| {
        Instance::Group(vec![
            (
                "Label".into(),
                Instance::Scalar(Value::String(label.into())),
            ),
            (
                "ParentPosition".into(),
                Instance::Scalar(Value::Int(parent_position)),
            ),
            (
                "ItemPosition".into(),
                Instance::Scalar(Value::Int(item_position)),
            ),
        ])
    };
    let rows = |parent: &str, parent_position, labels: &[&str]| {
        labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                row(
                    &format!("{parent}-{label}"),
                    parent_position,
                    index as i64 + 1,
                )
            })
            .collect::<Vec<_>>()
    };
    let parent = |name: &str, parent_position, window_labels: &[&str]| {
        let mapped = rows(name, parent_position, &["A", "B"]);
        Instance::Group(vec![
            (
                "Parent".into(),
                Instance::Scalar(Value::String(name.into())),
            ),
            (
                "SortThenFilter".into(),
                Instance::Repeated(rows(name, parent_position, &["H", "C", "D"])),
            ),
            (
                "FilterThenSort".into(),
                Instance::Repeated(rows(name, parent_position, &["C", "D", "B", "A"])),
            ),
            (
                "Windows".into(),
                Instance::Repeated(rows(name, parent_position, window_labels)),
            ),
            (
                "First".into(),
                row(&format!("{name}-A"), parent_position, 1),
            ),
            ("EmptyFirst".into(), Instance::Group(Vec::new())),
            ("Mapped".into(), Instance::MappedSequence(mapped)),
            (
                "LazyFirst".into(),
                Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(1)))]),
            ),
        ])
    };

    Instance::Group(vec![(
        "ParentOut".into(),
        Instance::Repeated(vec![
            parent("P1", 1, &["C", "D"]),
            parent("P2", 2, &["D", "E"]),
        ]),
    )])
}

fn write_controls_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("iteration-controls-project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&controls_project())?)?;
    Ok(path)
}

fn write_rust_harness(output: &Path) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/iteration_controls_rust_harness.rs.txt"),
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
        include_str!("fixtures/iteration_controls_csharp_harness.cs.txt"),
    )?;
    Ok(())
}

#[test]
fn source_iteration_controls_match_engine_and_generated_backends() -> TestResult<()> {
    let project = controls_project();
    assert_eq!(
        engine::run(&project, &controls_source(false))?,
        controls_expected()
    );
    assert_eq!(
        engine::run(&project, &controls_source(true)),
        Err(engine::EngineError::NotAnItemCount {
            node: 20,
            found: "bool",
        })
    );

    let directory = TempDir::new("iteration_controls")?;
    let project_path = write_controls_project(&directory.0)?;
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
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust iteration controls failed:\nstdout:\n{}\nstderr:\n{}",
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
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "generated C# iteration controls failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
