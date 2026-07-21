use super::*;

const JOIN: mapping::JoinId = mapping::JoinId::new(77);

fn join_project() -> Result<Project, mapping::JoinPlanError> {
    let plan = mapping::JoinPlan::new(
        mapping::JoinSource::new(vec!["A".into()]),
        mapping::JoinSource::new(vec!["catalog".into(), "B".into()]),
        mapping::JoinConditions::new(mapping::JoinKey::new(
            vec!["A".into()],
            vec!["Id".into()],
            vec!["Aid".into()],
        ))
        .and(mapping::JoinKey::new(
            vec!["A".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        )),
    )?
    .then(
        mapping::JoinSource::new(vec!["C".into()]),
        mapping::JoinConditions::new(mapping::JoinKey::new(
            vec!["catalog".into(), "B".into()],
            vec!["Code".into()],
            vec!["Code".into()],
        ))
        .and(mapping::JoinKey::new(
            vec!["A".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        )),
    )?;

    let a = SchemaNode::group("A", vec![int("Id"), string("Region"), string("Label")]).repeating();
    let b = SchemaNode::group(
        "B",
        vec![
            string("Aid"),
            string("Region"),
            string("Code"),
            string("Tag"),
            int("Rank"),
        ],
    )
    .repeating();
    let c =
        SchemaNode::group("C", vec![string("Code"), string("Region"), string("Value")]).repeating();
    let row = SchemaNode::group(
        "Row",
        vec![
            string("ALabel"),
            string("BTag"),
            string("CValue"),
            int("JoinPosition"),
            int("APosition"),
            int("BPosition"),
            int("CPosition"),
            int("Rank"),
            SchemaNode::group("Details", vec![string("Summary")]),
        ],
    )
    .repeating();

    Ok(Project {
        source: SchemaNode::group("Source", vec![a, c]),
        target: SchemaNode::group("Target", vec![row]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "catalog".into(),
            path: "ignored/catalog.json".into(),
            schema: SchemaNode::group("Catalog", vec![b]),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::JoinField {
                        join: JOIN,
                        collection: vec!["A".into()],
                        path: vec!["Label".into()],
                    },
                ),
                (
                    2,
                    Node::JoinField {
                        join: JOIN,
                        collection: vec!["catalog".into(), "B".into()],
                        path: vec!["Tag".into()],
                    },
                ),
                (
                    3,
                    Node::JoinField {
                        join: JOIN,
                        collection: vec!["C".into()],
                        path: vec!["Value".into()],
                    },
                ),
                (
                    4,
                    Node::JoinField {
                        join: JOIN,
                        collection: vec!["catalog".into(), "B".into()],
                        path: vec!["Rank".into()],
                    },
                ),
                (5, Node::JoinPosition { join: JOIN }),
                (
                    6,
                    Node::Position {
                        collection: vec!["A".into()],
                    },
                ),
                (
                    7,
                    Node::Position {
                        collection: vec!["catalog".into(), "B".into()],
                    },
                ),
                (
                    8,
                    Node::Position {
                        collection: vec!["C".into()],
                    },
                ),
                (
                    9,
                    Node::Const {
                        value: Value::Int(10),
                    },
                ),
                (
                    10,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![4, 9],
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::Int(4),
                    },
                ),
                (
                    12,
                    Node::Const {
                        value: Value::String(":".into()),
                    },
                ),
                (
                    13,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![1, 12, 3],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin { id: JOIN, plan },
                filter: Some(10),
                sort_by: Some(4),
                sort_descending: true,
                windows: vec![mapping::SequenceWindow::First { count: 11 }],
                bindings: vec![
                    Binding {
                        target_field: "ALabel".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "BTag".into(),
                        node: 2,
                    },
                    Binding {
                        target_field: "CValue".into(),
                        node: 3,
                    },
                    Binding {
                        target_field: "JoinPosition".into(),
                        node: 5,
                    },
                    Binding {
                        target_field: "APosition".into(),
                        node: 6,
                    },
                    Binding {
                        target_field: "BPosition".into(),
                        node: 7,
                    },
                    Binding {
                        target_field: "CPosition".into(),
                        node: 8,
                    },
                    Binding {
                        target_field: "Rank".into(),
                        node: 4,
                    },
                ],
                children: vec![Scope {
                    target_field: "Details".into(),
                    bindings: vec![Binding {
                        target_field: "Summary".into(),
                        node: 13,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    })
}

fn primary_source() -> Instance {
    Instance::Group(vec![
        (
            "A".into(),
            Instance::Repeated(vec![
                record(&[
                    ("Id", Value::Int(1)),
                    ("Region", Value::String("west".into())),
                    ("Label", Value::String("A1".into())),
                ]),
                record(&[
                    ("Id", Value::Int(1)),
                    ("Region", Value::String("west".into())),
                    ("Label", Value::String("A2".into())),
                ]),
                record(&[
                    ("Id", Value::Null),
                    ("Region", Value::String("west".into())),
                    ("Label", Value::String("AN".into())),
                ]),
            ]),
        ),
        (
            "C".into(),
            Instance::Repeated(vec![
                record(&[
                    ("Code", Value::String("X".into())),
                    ("Region", Value::String("west".into())),
                    ("Value", Value::String("CX1".into())),
                ]),
                record(&[
                    ("Code", Value::String("X".into())),
                    ("Region", Value::String("west".into())),
                    ("Value", Value::String("CX2".into())),
                ]),
                record(&[
                    ("Code", Value::String("Y".into())),
                    ("Region", Value::String("west".into())),
                    ("Value", Value::String("CY".into())),
                ]),
                record(&[
                    ("Code", Value::String("X".into())),
                    ("Region", Value::String("east".into())),
                    ("Value", Value::String("CE".into())),
                ]),
            ]),
        ),
    ])
}

fn catalog_source() -> Instance {
    Instance::Group(vec![(
        "B".into(),
        Instance::Repeated(vec![
            b_row(Value::String("1".into()), "west", "X", "low", 10),
            b_row(Value::Int(1), "west", "X", "high", 30),
            b_row(Value::Int(1), "west", "Y", "mid", 20),
            b_row(Value::xml_nil(), "west", "X", "nil", 40),
            b_row(Value::Int(1), "east", "X", "east", 50),
        ]),
    )])
}

fn b_row(aid: Value, region: &str, code: &str, tag: &str, rank: i64) -> Instance {
    record(&[
        ("Aid", aid),
        ("Region", Value::String(region.into())),
        ("Code", Value::String(code.into())),
        ("Tag", Value::String(tag.into())),
        ("Rank", Value::Int(rank)),
    ])
}

fn record(fields: &[(&str, Value)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| ((*name).into(), Instance::Scalar(value.clone())))
            .collect(),
    )
}

fn expected() -> Instance {
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![
            target_row("A1", "CX1", 1, 1, 1),
            target_row("A1", "CX2", 2, 1, 2),
            target_row("A2", "CX1", 1, 2, 3),
            target_row("A2", "CX2", 2, 2, 4),
        ]),
    )])
}

fn target_row(
    label: &str,
    c_value: &str,
    c_position: i64,
    a_position: i64,
    join_position: i64,
) -> Instance {
    Instance::Group(vec![
        (
            "ALabel".into(),
            Instance::Scalar(Value::String(label.into())),
        ),
        (
            "BTag".into(),
            Instance::Scalar(Value::String("high".into())),
        ),
        (
            "CValue".into(),
            Instance::Scalar(Value::String(c_value.into())),
        ),
        (
            "JoinPosition".into(),
            Instance::Scalar(Value::Int(join_position)),
        ),
        ("APosition".into(), Instance::Scalar(Value::Int(a_position))),
        ("BPosition".into(), Instance::Scalar(Value::Int(2))),
        ("CPosition".into(), Instance::Scalar(Value::Int(c_position))),
        ("Rank".into(), Instance::Scalar(Value::Int(30))),
        (
            "Details".into(),
            Instance::Group(vec![(
                "Summary".into(),
                Instance::Scalar(Value::String(format!("{label}:{c_value}"))),
            )]),
        ),
    ])
}

fn engine_sources() -> Vec<(String, Instance)> {
    vec![("catalog".into(), catalog_source())]
}

#[test]
fn root_inner_joins_match_engine_and_generated_backends() -> TestResult<()> {
    let project = join_project()?;
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let engine_output = engine::run_with_sources(&project, &primary_source(), engine_sources())?;
    let rows = engine_output
        .field("Row")
        .and_then(Instance::as_repeated)
        .ok_or("engine join output has no rows")?;
    assert_eq!(rows.len(), 4);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(
            row.field("JoinPosition").and_then(Instance::as_scalar),
            Some(&Value::Int((index + 1) as i64))
        );
    }
    assert_eq!(engine_output, expected());

    let directory = TempDir::new("joins")?;
    let project_path = directory.0.join("joins.json");
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
        include_str!("fixtures/joins_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust join failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/joins_csharp_harness.cs.txt"),
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
        .output()?;
    assert!(
        csharp.status.success(),
        "generated C# join failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
