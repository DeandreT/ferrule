use super::*;

fn extra_target_project() -> Project {
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                10,
                Node::SourceField {
                    path: vec!["Name".into()],
                    frame: None,
                },
            ),
            (
                20,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: Some(vec!["Rows".into()]),
                },
            ),
            (
                30,
                Node::SourceField {
                    path: vec!["FailExtra".into()],
                    frame: None,
                },
            ),
            (
                40,
                Node::Const {
                    value: Value::Int(9),
                },
            ),
            (
                50,
                Node::Call {
                    function: "normalize_space".into(),
                    args: vec![40],
                },
            ),
            (
                60,
                Node::Const {
                    value: Value::String("safe-extra".into()),
                },
            ),
            (
                70,
                Node::If {
                    condition: 30,
                    then: 50,
                    else_: 60,
                },
            ),
        ]),
    };
    let audit_schema = SchemaNode::group(
        "AuditTarget",
        vec![SchemaNode::group("AuditRow", vec![int("Value")]).repeating()],
    );
    let audit_root = Scope {
        children: vec![Scope {
            target_field: "AuditRow".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 20,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let late_schema = SchemaNode::group("LateTarget", vec![string("Status")]);
    let late_root = Scope {
        bindings: vec![Binding {
            target_field: "Status".into(),
            node: 70,
        }],
        ..Scope::default()
    };

    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                string("Name"),
                bool_("FailExtra"),
                SchemaNode::group("Rows", vec![int("Value")]).repeating(),
            ],
        ),
        target: SchemaNode::group("PrimaryTarget", vec![string("PrimaryName")]),
        source_path: None,
        target_path: Some("primary.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![
            mapping::NamedTarget {
                name: "audit".into(),
                path: Some("reports/audit.json".into()),
                schema: audit_schema,
                options: Default::default(),
                root: audit_root,
            },
            mapping::NamedTarget {
                name: "late".into(),
                path: Some("reports/late.json".into()),
                schema: late_schema,
                options: Default::default(),
                root: late_root,
            },
        ],
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "PrimaryName".into(),
                node: 10,
            }],
            ..Scope::default()
        },
    }
}

fn source(fail_extra: bool) -> Instance {
    Instance::Group(vec![
        (
            "Name".into(),
            Instance::Scalar(Value::String("primary".into())),
        ),
        (
            "FailExtra".into(),
            Instance::Scalar(Value::Bool(fail_extra)),
        ),
        (
            "Rows".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(3)))]),
                Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(5)))]),
            ]),
        ),
    ])
}

fn primary_expected() -> Instance {
    Instance::Group(vec![(
        "PrimaryName".into(),
        Instance::Scalar(Value::String("primary".into())),
    )])
}

fn audit_expected() -> Instance {
    Instance::Group(vec![(
        "AuditRow".into(),
        Instance::Repeated(vec![
            Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(3)))]),
            Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(5)))]),
        ]),
    )])
}

fn late_expected() -> Instance {
    Instance::Group(vec![(
        "Status".into(),
        Instance::Scalar(Value::String("safe-extra".into())),
    )])
}

#[test]
fn extra_targets_match_engine_and_legacy_entry_points_evaluate_them() -> TestResult<()> {
    let project = extra_target_project();
    assert!(engine::validate(&project).is_empty());
    let output = engine::run_outputs(&project, &source(false))?;
    assert_eq!(output.primary, primary_expected());
    assert_eq!(output.extras.len(), 2);
    assert_eq!(output.extras[0].name, "audit");
    assert_eq!(output.extras[0].instance, audit_expected());
    assert_eq!(output.extras[1].name, "late");
    assert_eq!(output.extras[1].instance, late_expected());
    assert_eq!(engine::run(&project, &source(false))?, primary_expected());
    let error = engine::run(&project, &source(true))
        .expect_err("legacy engine execution must evaluate the late extra target");
    assert_eq!(
        error.to_string(),
        "`normalize_space` cannot accept a int argument"
    );

    let directory = TempDir::new("extra_targets")?;
    let project_path = directory.0.join("extra-targets.json");
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
        include_str!("fixtures/extra_targets_rust_harness.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .output()?;
    assert!(
        rust.status.success(),
        "generated Rust extra targets failed:\nstdout:\n{}\nstderr:\n{}",
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
        include_str!("fixtures/extra_targets_csharp_harness.cs.txt"),
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
        "generated C# extra targets failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
