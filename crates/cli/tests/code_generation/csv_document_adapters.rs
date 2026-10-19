use super::*;
use mapping::{AggregateOp, FormatOptions, TabularBoundaryKind};

fn document_project(grouped: bool, unquoted: bool) -> Project {
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: None,
    };
    let mut nodes = BTreeMap::from([
        (0, field("Key")),
        (1, field("Text")),
        (2, field("Count")),
        (3, field("Fail")),
        (
            4,
            Node::Const {
                value: Value::String("ok".into()),
            },
        ),
        (
            5,
            Node::RuntimeParameter {
                name: "Required".into(),
                ty: ScalarType::String,
                preview: None,
            },
        ),
        (
            6,
            Node::If {
                condition: 3,
                then: 5,
                else_: 4,
            },
        ),
        (
            7,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            8,
            Node::Call {
                function: "add".into(),
                args: vec![2, 7],
            },
        ),
        (
            9,
            Node::Const {
                value: Value::String("+".into()),
            },
        ),
    ]);
    if grouped {
        nodes.insert(
            10,
            Node::Aggregate {
                function: AggregateOp::Join,
                collection: vec![],
                value: vec!["Text".into()],
                expression: None,
                arg: Some(9),
            },
        );
        nodes.insert(
            11,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec![],
                value: vec!["Count".into()],
                expression: None,
                arg: None,
            },
        );
    }
    Project {
        source: SchemaNode::group(
            "Input",
            vec![string("Key"), string("Text"), int("Count"), bool_("Fail")],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                string("Key"),
                string("Text"),
                int("Count"),
                string("Status"),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            csv_preserve_empty_strings: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some('|'),
            csv_utf8_bom: true,
            csv_quote_disabled: unquoted,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            iteration: ScopeIteration::Source(vec![]),
            group_by: grouped.then_some(0),
            bindings: [
                ("Key", 0),
                ("Text", if grouped { 10 } else { 1 }),
                ("Count", if grouped { 11 } else { 8 }),
                ("Status", if grouped { 4 } else { 6 }),
            ]
            .into_iter()
            .map(|(target_field, node)| Binding {
                target_field: target_field.into(),
                node,
            })
            .collect(),
            ..Default::default()
        },
    }
}

#[test]
fn complete_csv_documents_compile_and_execute_in_both_backends() -> TestResult<()> {
    let directory = TempDir::new("csv_documents")?;
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/csv-documents/cases.json"
    ))?;
    for mode in ["rows", "grouped", "unquoted"] {
        let project = document_project(mode == "grouped", mode == "unquoted");
        let project_path = directory.0.join(format!("{mode}.json"));
        std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
        for language in ["rust", "csharp"] {
            let output = directory.0.join(format!("{mode}-{language}"));
            let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
            command
                .args([
                    "generate",
                    "--csv-document-adapters",
                    "--language",
                    language,
                    "--project",
                ])
                .arg(&project_path)
                .arg("--out")
                .arg(&output);
            if language == "rust" {
                command
                    .arg("--rust-runtime-path")
                    .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"));
            }
            let generated = command.output()?;
            assert!(
                generated.status.success(),
                "{}",
                String::from_utf8_lossy(&generated.stderr)
            );
            let fixture = if mode == "unquoted" { "rows" } else { mode };
            for (i, case) in cases[fixture].as_array().unwrap().iter().enumerate() {
                std::fs::write(
                    output.join(format!("input{i}.csv")),
                    case["input"].as_str().unwrap(),
                )?;
                std::fs::write(
                    output.join(format!("expected{i}.csv")),
                    case["expected"].as_str().unwrap(),
                )?;
            }
            let mut run = if language == "rust" {
                std::fs::write(
                    output.join("src/main.rs"),
                    include_str!("fixtures/csv_document_rust.rs.txt"),
                )?;
                let mut command = Command::new("cargo");
                command.args(["run", "--offline", "--quiet", "--", mode]);
                command
            } else {
                std::fs::create_dir(output.join("Harness"))?;
                std::fs::write(
                    output.join("Harness/Program.cs"),
                    include_str!("fixtures/csv_document_csharp.cs.txt"),
                )?;
                std::fs::write(
                    output.join("Harness/Harness.csproj"),
                    r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
                )?;
                let mut command = dotnet_command(&output);
                command.args([
                    "run",
                    "--project",
                    "Harness/Harness.csproj",
                    "--configuration",
                    "Release",
                    "-p:UseSharedCompilation=false",
                    "--",
                    mode,
                ]);
                command
            };
            let result = host_policy::recorded_output(
                run.current_dir(&output),
                &output,
                "complete-documents",
            )?;
            assert!(
                result.status.success(),
                "{mode}/{language}:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                String::from_utf8_lossy(&result.stdout).contains("complete CSV documents passed")
            );
        }
    }
    Ok(())
}

#[test]
fn csv_document_artifacts_preserve_existing_files_and_exact_runtime_sources() -> TestResult<()> {
    let project = document_project(false, false);
    let program = codegen::lower(&project)?;
    let policy = codegen::CsvDocumentBoundaryPolicy::from_project(&project)?;
    let runtime = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs"),
    )?;
    let baseline = codegen_csharp::emit_with_csv_output(&program, &policy.target)?;
    let emitted = codegen_csharp::emit_with_csv_document_adapters(&program, &policy)?;
    assert_eq!(
        emitted,
        codegen_csharp::emit_with_csv_document_adapters(&program, &policy)?
    );
    let mut paths = baseline
        .files()
        .iter()
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>();
    paths.extend([
        "GeneratedMapping.CsvDocument.cs",
        "Runtime/FerruleCsvInput.cs",
    ]);
    paths.sort_unstable();
    assert_eq!(
        paths,
        emitted
            .files()
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
    );
    for original in baseline.files() {
        let expected = if original.path.as_str() == "Ferrule.Generated.csproj" {
            let original = std::str::from_utf8(&original.contents)?;
            let entry = "    <Compile Include=\"GeneratedMapping.cs\" />\n";
            assert_eq!(original.matches(entry).count(), 1);
            original
                .replacen(
                    entry,
                    &format!(
                        "{entry}    <Compile Include=\"GeneratedMapping.CsvDocument.cs\" />\n"
                    ),
                    1,
                )
                .into_bytes()
        } else {
            original.contents.clone()
        };
        assert_eq!(
            emitted
                .files()
                .iter()
                .find(|f| f.path == original.path)
                .unwrap()
                .contents,
            expected
        );
    }
    assert_eq!(
        emitted
            .files()
            .iter()
            .find(|f| f.path.as_str() == "Runtime/FerruleCsvInput.cs")
            .unwrap()
            .contents,
        runtime
    );

    let options = codegen_rust::Options {
        package_name: "csv_document_inventory".into(),
        runtime_path: "../codegen-runtime".into(),
    };
    let baseline = codegen_rust::emit_with_csv_output(&program, &options, &policy.target)?;
    let emitted = codegen_rust::emit_with_csv_document_adapters(&program, &options, &policy)?;
    assert_eq!(
        emitted,
        codegen_rust::emit_with_csv_document_adapters(&program, &options, &policy)?
    );
    assert_eq!(
        emitted
            .files()
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["Cargo.toml", "src/lib.rs"]
    );
    for original in baseline.files() {
        let actual = &emitted
            .files()
            .iter()
            .find(|f| f.path == original.path)
            .unwrap()
            .contents;
        if original.path.as_str() == "src/lib.rs" {
            assert!(actual.starts_with(&original.contents));
        } else {
            assert_eq!(*actual, original.contents);
        }
    }
    Ok(())
}

#[test]
fn csv_document_admission_rejects_omitted_endpoints_and_options_before_publication()
-> TestResult<()> {
    let directory = TempDir::new("csv_document_refusals")?;
    for case in [
        "source_identity",
        "target_identity",
        "metadata",
        "options",
        "named_source",
        "named_target",
        "single_root",
    ] {
        let mut project = document_project(false, false);
        match case {
            "source_identity" => project.source_options.tabular_kind = None,
            "target_identity" => project.target_options.tabular_kind = None,
            "metadata" => project.source.nullable = true,
            "options" => project.source_options.json_document = true,
            "named_source" => project.extra_sources.push(mapping::NamedSource {
                name: "Extra".into(),
                path: "unused.csv".into(),
                schema: project.source.clone(),
                options: project.source_options.clone(),
                dynamic_path: None,
            }),
            "named_target" => project.extra_targets.push(mapping::NamedTarget {
                name: "Extra".into(),
                path: None,
                schema: project.target.clone(),
                options: project.target_options.clone(),
                root: project.root.clone(),
            }),
            "single_root" => project.root.iteration = ScopeIteration::None,
            _ => unreachable!(),
        }
        let path = directory.0.join(format!("{case}.json"));
        std::fs::write(&path, serde_json::to_vec(&project)?)?;
        for (name, backend) in [
            (
                "rust",
                GenerateTarget::Rust {
                    runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
                },
            ),
            ("csharp", GenerateTarget::CSharp),
        ] {
            let output = directory.0.join(format!("{case}-{name}"));
            assert!(
                cli::generate_project_with_csv_document_adapters(&path, &output, backend).is_err(),
                "{case}/{name}"
            );
            assert!(!output.exists());
        }
    }
    Ok(())
}
