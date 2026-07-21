use super::*;
use mapping::AggregateOp;

fn float_(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
}

fn aggregate_item_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Amount"),
            string("Label"),
            int("Factor"),
            int("Divisor"),
        ],
    )
    .repeating()
}

fn aggregate_project() -> Project {
    let contact = SchemaNode::group("Contacts", vec![string("First")]).repeating();
    let office = SchemaNode::group("Offices", vec![contact]).repeating();
    let order = SchemaNode::group(
        "Orders",
        vec![
            string("Separator"),
            float_("Pick"),
            aggregate_item_schema("Items"),
            aggregate_item_schema("EmptyItems"),
        ],
    )
    .repeating();
    let order_out = SchemaNode::group(
        "OrderOut",
        vec![
            int("Count"),
            float_("Sum"),
            float_("Average"),
            float_("Minimum"),
            int("Maximum"),
            string("Joined"),
            string("Picked"),
            int("Computed"),
            int("EvaluatedCount"),
            int("EmptyCount"),
            int("EmptySum"),
            float_("EmptyAverage"),
        ],
    )
    .repeating();

    let aggregate = |function, value: &[&str], arg| Node::Aggregate {
        function,
        collection: vec!["Items".into()],
        value: value.iter().map(|segment| (*segment).into()).collect(),
        expression: None,
        arg,
    };
    let empty = |function| Node::Aggregate {
        function,
        collection: vec!["EmptyItems".into()],
        value: vec!["Amount".into()],
        expression: None,
        arg: None,
    };

    Project {
        source: SchemaNode::group("Source", vec![office, order]),
        target: SchemaNode::group("Target", vec![string("AllContacts"), order_out]),
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
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (
                    2,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Offices".into(), "Contacts".into()],
                        value: vec!["First".into()],
                        expression: None,
                        arg: Some(1),
                    },
                ),
                (10, aggregate(AggregateOp::Count, &["Amount"], None)),
                (11, aggregate(AggregateOp::Sum, &["Amount"], None)),
                (12, aggregate(AggregateOp::Avg, &["Amount"], None)),
                (13, aggregate(AggregateOp::Min, &["Amount"], None)),
                (14, aggregate(AggregateOp::Max, &["Amount"], None)),
                (
                    15,
                    Node::SourceField {
                        path: vec!["Separator".into()],
                        frame: Some(vec!["Orders".into()]),
                    },
                ),
                (16, aggregate(AggregateOp::Join, &["Label"], Some(15))),
                (
                    17,
                    Node::SourceField {
                        path: vec!["Pick".into()],
                        frame: Some(vec!["Orders".into()]),
                    },
                ),
                (18, aggregate(AggregateOp::ItemAt, &["Label"], Some(17))),
                (
                    19,
                    Node::SourceField {
                        path: vec!["Factor".into()],
                        frame: Some(vec!["Orders".into(), "Items".into()]),
                    },
                ),
                (
                    20,
                    Node::Position {
                        collection: vec!["Items".into()],
                    },
                ),
                (
                    21,
                    Node::Call {
                        function: "multiply".into(),
                        args: vec![19, 20],
                    },
                ),
                (
                    22,
                    Node::Aggregate {
                        function: AggregateOp::Sum,
                        collection: vec!["Items".into()],
                        value: Vec::new(),
                        expression: Some(21),
                        arg: None,
                    },
                ),
                (23, empty(AggregateOp::Count)),
                (24, empty(AggregateOp::Sum)),
                (25, empty(AggregateOp::Avg)),
                (
                    26,
                    Node::SourceField {
                        path: vec!["Divisor".into()],
                        frame: Some(vec!["Orders".into(), "Items".into()]),
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
                (
                    29,
                    Node::Aggregate {
                        function: AggregateOp::Count,
                        collection: vec!["Items".into()],
                        value: Vec::new(),
                        expression: Some(28),
                        arg: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "AllContacts".into(),
                node: 2,
            }],
            children: vec![Scope {
                target_field: "OrderOut".into(),
                iteration: ScopeIteration::Source(vec!["Orders".into()]),
                bindings: [
                    ("Count", 10),
                    ("Sum", 11),
                    ("Average", 12),
                    ("Minimum", 13),
                    ("Maximum", 14),
                    ("Joined", 16),
                    ("Picked", 18),
                    ("Computed", 22),
                    ("EvaluatedCount", 29),
                    ("EmptyCount", 23),
                    ("EmptySum", 24),
                    ("EmptyAverage", 25),
                ]
                .into_iter()
                .map(|(target_field, node)| Binding {
                    target_field: target_field.into(),
                    node,
                })
                .collect(),
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[derive(Clone, Copy)]
enum Failure {
    None,
    Overflow,
    NonFinite,
    Projection,
}

fn aggregate_source(failure: Failure) -> Instance {
    let contact = |first: &str| {
        Instance::Group(vec![(
            "First".into(),
            Instance::Scalar(Value::String(first.into())),
        )])
    };
    let office = |names: &[&str]| {
        Instance::Group(vec![(
            "Contacts".into(),
            Instance::Repeated(names.iter().map(|name| contact(name)).collect()),
        )])
    };
    let item = |amount: Value, label: Value, factor, divisor| {
        Instance::Group(vec![
            ("Amount".into(), Instance::Scalar(amount)),
            ("Label".into(), Instance::Scalar(label)),
            ("Factor".into(), Instance::Scalar(Value::Int(factor))),
            ("Divisor".into(), Instance::Scalar(Value::Int(divisor))),
        ])
    };
    let order = |separator: &str, pick: f64, items: Vec<Instance>| {
        Instance::Group(vec![
            (
                "Separator".into(),
                Instance::Scalar(Value::String(separator.into())),
            ),
            ("Pick".into(), Instance::Scalar(Value::Float(pick))),
            ("Items".into(), Instance::Repeated(items)),
            ("EmptyItems".into(), Instance::Repeated(Vec::new())),
        ])
    };

    let orders = match failure {
        Failure::None => vec![
            order(
                "|",
                2.5,
                vec![
                    item(Value::String("10".into()), Value::String("A".into()), 2, 1),
                    item(Value::String("2.5".into()), Value::Null, 3, 1),
                    item(Value::String("junk".into()), Value::xml_nil(), 4, 1),
                    item(Value::Null, Value::String("B".into()), 5, 1),
                ],
            ),
            order(
                "~",
                2.0,
                vec![
                    item(Value::String("4".into()), Value::String("X".into()), 1, 1),
                    item(Value::String("6".into()), Value::String("Y".into()), 1, 1),
                ],
            ),
        ],
        Failure::Overflow => vec![order(
            "|",
            1.0,
            vec![
                item(
                    Value::String(i64::MAX.to_string()),
                    Value::String("A".into()),
                    1,
                    1,
                ),
                item(Value::String("1".into()), Value::String("B".into()), 1, 1),
            ],
        )],
        Failure::NonFinite => vec![order(
            "|",
            1.0,
            vec![item(
                Value::String("inf".into()),
                Value::String("A".into()),
                1,
                1,
            )],
        )],
        Failure::Projection => vec![order(
            "|",
            1.0,
            vec![item(
                Value::String("1".into()),
                Value::String("A".into()),
                1,
                0,
            )],
        )],
    };

    Instance::Group(vec![
        (
            "Offices".into(),
            Instance::Repeated(vec![office(&["Ada", "Lin"]), office(&["Sam"])]),
        ),
        ("Orders".into(), Instance::Repeated(orders)),
    ])
}

fn aggregate_expected() -> Instance {
    let order = |values: Vec<(&str, Value)>| {
        Instance::Group(
            values
                .into_iter()
                .map(|(name, value)| (name.into(), Instance::Scalar(value)))
                .collect(),
        )
    };
    Instance::Group(vec![
        (
            "AllContacts".into(),
            Instance::Scalar(Value::String("Ada,Lin,Sam".into())),
        ),
        (
            "OrderOut".into(),
            Instance::Repeated(vec![
                order(vec![
                    ("Count", Value::Int(4)),
                    ("Sum", Value::Float(12.5)),
                    ("Average", Value::Float(6.25)),
                    ("Minimum", Value::Float(2.5)),
                    ("Maximum", Value::Int(10)),
                    ("Joined", Value::String("A||B".into())),
                    ("Picked", Value::xml_nil()),
                    ("Computed", Value::Int(40)),
                    ("EvaluatedCount", Value::Int(4)),
                    ("EmptyCount", Value::Int(0)),
                    ("EmptySum", Value::Int(0)),
                    ("EmptyAverage", Value::Null),
                ]),
                order(vec![
                    ("Count", Value::Int(2)),
                    ("Sum", Value::Float(10.0)),
                    ("Average", Value::Float(5.0)),
                    ("Minimum", Value::Float(4.0)),
                    ("Maximum", Value::Int(6)),
                    ("Joined", Value::String("X~Y".into())),
                    ("Picked", Value::String("Y".into())),
                    ("Computed", Value::Int(3)),
                    ("EvaluatedCount", Value::Int(2)),
                    ("EmptyCount", Value::Int(0)),
                    ("EmptySum", Value::Int(0)),
                    ("EmptyAverage", Value::Null),
                ]),
            ]),
        ),
    ])
}

fn write_aggregate_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("aggregate-project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&aggregate_project())?)?;
    Ok(path)
}

#[test]
fn ordinary_aggregates_match_engine_and_generated_backends() -> TestResult<()> {
    let project = aggregate_project();
    assert_eq!(
        engine::run(&project, &aggregate_source(Failure::None))?,
        aggregate_expected()
    );
    assert!(matches!(
        engine::run(&project, &aggregate_source(Failure::Overflow)),
        Err(engine::EngineError::AggregateIntegerOverflow {
            function: AggregateOp::Sum
        })
    ));
    assert!(matches!(
        engine::run(&project, &aggregate_source(Failure::NonFinite)),
        Err(engine::EngineError::AggregateNonFinite {
            function: AggregateOp::Sum
        })
    ));
    assert!(matches!(
        engine::run(&project, &aggregate_source(Failure::Projection)),
        Err(engine::EngineError::Function(_))
    ));

    let directory = TempDir::new("aggregates")?;
    let project_path = write_aggregate_project(&directory.0)?;
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
        "generated Rust aggregates failed:\nstdout:\n{}\nstderr:\n{}",
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
        "generated C# aggregates failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}

fn write_rust_harness(output: &Path) -> TestResult<()> {
    std::fs::write(output.join("src/main.rs"), rust_harness())?;
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
    std::fs::write(harness.join("Program.cs"), csharp_harness())?;
    Ok(())
}

fn rust_harness() -> &'static str {
    include_str!("fixtures/aggregate_rust_harness.rs.txt")
}

fn csharp_harness() -> &'static str {
    include_str!("fixtures/aggregate_csharp_harness.cs.txt")
}
