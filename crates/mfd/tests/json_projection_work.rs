//! Synthetic, independent whole-output and real parse-call-count checks.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, SequenceExpr};

#[path = "json_projection_work/contexts.rs"]
mod contexts;

fn fields(count: usize) -> Vec<SchemaNode> {
    (0..count)
        .map(|field| {
            SchemaNode::scalar(format!("F{field}"), ScalarType::String)
                .nullable()
                .unwrap()
        })
        .collect()
}

fn project(count: usize) -> Project {
    let schema = SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            SchemaNode::group("Rows", fields(count)).repeating(),
        ],
    );
    let descriptor = serde_json::to_string(&schema).unwrap();
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::RuntimeParameter {
                name: "InputText".into(),
                ty: ScalarType::String,
                preview: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(descriptor.clone()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(r#"["Rows"]"#.into()),
            },
        ),
        (
            3,
            Node::Call {
                function: "json_parse_array_length".into(),
                args: vec![0, 1, 2],
            },
        ),
        (
            4,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            5,
            Node::Const {
                value: Value::String(r#"["Title"]"#.into()),
            },
        ),
        (
            6,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![0, 1, 5],
            },
        ),
    ]);
    let mut bindings = Vec::new();
    let mut roots = vec![Binding {
        target_field: "Title".into(),
        node: 6,
    }];
    let mut target_fields = vec![SchemaNode::scalar("Title", ScalarType::String)];
    for field in 0..count {
        let id = 10 + field as u32 * 5;
        // Separate equal descriptors deliberately have different node IDs.
        nodes.insert(
            id,
            Node::Const {
                value: Value::String(descriptor.clone()),
            },
        );
        nodes.insert(
            id + 1,
            Node::Const {
                value: Value::String(format!("[\"F{field}\"]")),
            },
        );
        nodes.insert(
            id + 2,
            Node::Call {
                function: "json_parse_array_field".into(),
                args: vec![0, id, 2, id + 1, 4],
            },
        );
        nodes.insert(
            id + 3,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![0, id, 5],
            },
        );
        bindings.push(Binding {
            target_field: format!("F{field}"),
            node: id + 2,
        });
        roots.push(Binding {
            target_field: format!("Root{field}"),
            node: id + 3,
        });
        target_fields.push(SchemaNode::scalar(
            format!("Root{field}"),
            ScalarType::String,
        ));
    }
    target_fields.push(SchemaNode::group("Records", fields(count)).repeating());
    Project {
        execution_contract: None,
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Report", target_fields),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings: roots,
            children: vec![Scope {
                target_field: "Records".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                    from: None,
                    to: 3,
                    item: 4,
                }),
                bindings,
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

fn cases(count: usize) -> Vec<serde_json::Value> {
    let mut cases = Vec::new();
    for rows in [0, 1, 9, 43] {
        for title in ["first", "different", "first"] {
            let mut input_rows = Vec::new();
            let mut output_rows = Vec::new();
            for row in 0..rows {
                let mut input = serde_json::Map::new();
                let mut expected = serde_json::Map::new();
                for field in 0..count {
                    if (row + field) % 5 == 0 {
                        continue;
                    }
                    let value = if (row + field) % 7 == 0 {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!(format!("{title}-{row}-{field}"))
                    };
                    input.insert(format!("F{field}"), value.clone());
                    expected.insert(format!("F{field}"), value);
                }
                input_rows.push(serde_json::Value::Object(input));
                output_rows.push(serde_json::Value::Object(expected));
            }
            let mut expected = serde_json::Map::from_iter([
                ("Title".into(), serde_json::json!(title)),
                ("Records".into(), serde_json::json!(output_rows)),
            ]);
            for field in 0..count {
                expected.insert(format!("Root{field}"), serde_json::json!(title));
            }
            cases.push(serde_json::json!({"rows":rows,"input":serde_json::json!({"Title":title,"Rows":input_rows}).to_string(),"expected":expected}));
        }
    }
    for input in [
        r#"{"Title":"absent"}"#,
        r#"{"Title":"duplicate","Rows":[{"F0":"old","F0":"last"}]}"#,
    ] {
        let mut expected = serde_json::Map::new();
        let duplicate = input.contains("duplicate");
        let title = if duplicate { "duplicate" } else { "absent" };
        expected.insert("Title".into(), serde_json::json!(title));
        expected.insert(
            "Records".into(),
            if duplicate {
                serde_json::json!([{"F0":"last"}])
            } else {
                serde_json::json!([])
            },
        );
        for field in 0..count {
            expected.insert(format!("Root{field}"), serde_json::json!(title));
        }
        cases.push(serde_json::json!({"input":input,"expected":expected}));
    }
    for input in [
        r#"{"Title":"bad-late","Rows":[{"F0":"valid"},{"F0":42}]}"#,
        r#"{"Title":"invalid-empty","Rows":[],"late":1e400}"#,
        r#"{"Title":"invalid-absent","late":1e400}"#,
        r#"{"Rows":[{"F0":1e400,"F0":"last"}]}"#,
    ] {
        cases.push(
            serde_json::json!({"input":input,"error":"input does not match the JSON schema"}),
        );
    }
    cases
}

fn verify(project: &Project, count: usize) {
    verify_cases(project, count, cases(count));
}

fn verify_cases(project: &Project, count: usize, cases: Vec<serde_json::Value>) {
    assert!(
        engine::validate(project).is_empty(),
        "{:?}",
        engine::validate(project)
    );
    for case in cases {
        let mut parameters = engine::RuntimeParameters::new();
        parameters
            .insert(
                "InputText",
                Value::String(case["input"].as_str().unwrap().into()),
            )
            .unwrap();
        let execution =
            engine::ExecutionContext::new(Path::new("synthetic.mfd")).with_parameters(&parameters);
        functions::reset_json_projection_full_parse_count();
        let result =
            engine::run_with_context(project, &Instance::Group(Vec::new().into()), &execution);
        let parses = case["parses"].as_u64().unwrap_or(1) as usize;
        assert_eq!(
            functions::json_projection_full_parse_count(),
            parses,
            "{case}"
        );
        if case.get("error").is_some() {
            assert!(
                matches!(
                    result,
                    Err(engine::EngineError::Function(
                        functions::FunctionError::InvalidArgument {
                            function: "json_parse_field" | "json_parse_array_length",
                            message: "input does not match the JSON schema"
                        }
                    ))
                ),
                "{case}: {result:?}"
            );
        } else {
            let expected =
                format_json::from_str(&case["expected"].to_string(), &project.target).unwrap();
            assert_eq!(result.unwrap(), expected, "{case}");
        }
        println!(
            "interpreter fields={count} rows={} full_parses={parses}",
            case["rows"]
        );
    }
}

fn add_stateless_udf(project: &mut Project, cases: &mut [serde_json::Value]) {
    let function = mapping::FunctionId::new(7);
    project.user_functions.insert(
        function,
        mapping::UserFunction {
            library: "synthetic".into(),
            name: "RootTitle".into(),
            description: None,
            parameters: vec![],
            output_name: "Title".into(),
            output_type: ScalarType::String,
            // Deliberately reuse main-graph IDs with a different document.
            body: Graph {
                nodes: BTreeMap::from([
                    (
                        0,
                        Node::Const {
                            value: Value::String(r#"{"Title":"udf"}"#.into()),
                        },
                    ),
                    (1, project.graph.nodes[&1].clone()),
                    (5, project.graph.nodes[&5].clone()),
                    (
                        6,
                        Node::Call {
                            function: "json_parse_field".into(),
                            args: vec![0, 1, 5],
                        },
                    ),
                ]),
            },
            output: 6,
        },
    );
    for (node, name) in [(10000, "UdfA"), (10001, "UdfB")] {
        project.graph.nodes.insert(
            node,
            Node::UserFunctionCall {
                function,
                args: vec![],
            },
        );
        project.root.bindings.push(Binding {
            target_field: name.into(),
            node,
        });
        let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
            unreachable!()
        };
        children.insert(
            children.len() - 1,
            SchemaNode::scalar(name, ScalarType::String),
        );
        for case in cases.iter_mut().filter(|case| case.get("error").is_none()) {
            case["expected"][name] = serde_json::json!("udf");
            case["parses"] = serde_json::json!(3);
        }
    }
}

#[test]
fn udf_body_node_ids_do_not_alias_main_graph_invocation_state() {
    let mut project = project(1);
    let mut cases = cases(1);
    add_stateless_udf(&mut project, &mut cases);
    verify_cases(&project, 1, cases);
}

#[test]
fn interpreter_full_parse_work_is_bounded_across_rows_fields_and_invocations() {
    for count in [1, 4, 13] {
        verify(&project(count), count);
    }
}

#[test]
fn concurrent_invocations_do_not_share_documents_or_instrumentation() {
    let threads: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(|| verify(&project(4), 4)))
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_projection_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn checked(command: &mut std::process::Command) {
    let output = command.output().unwrap();
    println!(
        "{command:?}\nstatus={}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "generated host failed");
}

#[test]
#[ignore = "compiles instrumented generated Rust/C# Release hosts; requires Cargo and .NET 10"]
fn generated_release_full_parse_work_and_whole_outputs() {
    let dir = TempDir::new();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap();
    for (count, udf) in [(1, false), (4, false), (13, false), (1, true)] {
        let mut project = project(count);
        let mut cases = cases(count);
        if udf {
            add_stateless_udf(&mut project, &mut cases);
        }
        verify_cases(&project, count, cases.clone());
        let program = codegen::lower(&project).unwrap();
        let root = dir.0.join(format!("fields-{count}-udf-{udf}"));
        let rust = codegen_rust::emit(
            &program,
            &codegen_rust::Options {
                package_name: "json-projection-mapping".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    runtime.display().to_string(),
                ),
            },
        )
        .unwrap();
        let csharp = codegen_csharp::emit(&program).unwrap();
        for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
            for file in artifacts.files() {
                let path = root.join(language).join(file.path.as_str());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &file.contents).unwrap();
            }
        }
        let cases_path = root.join("cases.json");
        std::fs::write(&cases_path, serde_json::to_string(&cases).unwrap()).unwrap();
        let schema_path = root.join("target.json");
        let parser_schema_path = root.join("parser.json");
        let Node::Const {
            value: Value::String(parser_schema),
        } = &project.graph.nodes[&1]
        else {
            unreachable!()
        };
        std::fs::write(&parser_schema_path, parser_schema).unwrap();
        std::fs::write(
            &schema_path,
            serde_json::to_string(&project.target).unwrap(),
        )
        .unwrap();
        let rust = root.join("rust");
        let manifest = rust.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .unwrap()
            .replace("[dependencies]", "[dependencies]\nserde_json = \"1\"")
            .replace(
                "codegen-runtime = {",
                "codegen-runtime = { features = [\"json-projection-instrumentation\"],",
            );
        std::fs::write(
            &manifest,
            format!("{text}\n[[bin]]\nname = \"projection-host\"\npath = \"host.rs\"\n"),
        )
        .unwrap();
        std::fs::write(
            rust.join("host.rs"),
            include_str!("fixtures/json-projection-work/rust_host.rs"),
        )
        .unwrap();
        checked(
            std::process::Command::new("cargo")
                .args([
                    "run",
                    "--release",
                    "--offline",
                    "-j",
                    "1",
                    "--config",
                    "profile.release.debug=0",
                    "--config",
                    "profile.release.incremental=false",
                    "--target-dir",
                ])
                .arg(dir.0.join("rust-host-target"))
                .arg("--manifest-path")
                .arg(&manifest)
                .args(["--bin", "projection-host", "--"])
                .arg(&cases_path)
                .arg(&schema_path)
                .arg(count.to_string())
                .arg(&parser_schema_path)
                .env_remove("CARGO_BUILD_BUILD_DIR")
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .env("RUSTFLAGS", "-D warnings"),
        );
        let host = root.join("csharp/host");
        std::fs::create_dir(&host).unwrap();
        std::fs::write(
            root.join("NuGet.Config"),
            "<configuration><packageSources><clear/></packageSources></configuration>",
        )
        .unwrap();
        std::fs::write(host.join("Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/json-projection-work/csharp_host.cs"),
        )
        .unwrap();
        checked(
            std::process::Command::new("dotnet")
                .current_dir(&host)
                .args([
                    "run",
                    "--configuration",
                    "Release",
                    "--disable-build-servers",
                    "-p:UseSharedCompilation=false",
                    "-p:BuildInParallel=false",
                    "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                    "-p:AutomaticallyUseReferenceAssemblyPackages=false",
                    "-p:DefineConstants=FERRULE_JSON_PROJECTION_INSTRUMENTATION",
                    "--project",
                ])
                .arg(host.join("Host.csproj"))
                .arg("--")
                .arg(&cases_path)
                .arg(&schema_path)
                .arg(count.to_string())
                .arg(&parser_schema_path)
                .env("DOTNET_CLI_HOME", dir.0.join("dotnet-home"))
                .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
                .env("DOTNET_NOLOGO", "1")
                .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
                .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
                .env("MSBUILDDISABLENODEREUSE", "1"),
        );
    }
}
