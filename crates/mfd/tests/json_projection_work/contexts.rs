//! Generated sorted owned-grouping coverage, plus explicitly direct-runtime
//! filter/map callback-boundary controls. The latter are not a generated-graph
//! filter/map conformance claim.
use super::*;

fn source_files(root: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            source_files(&entry.path(), extension, files);
        } else if entry
            .path()
            .extension()
            .is_some_and(|value| value == extension)
        {
            files.push(entry.path());
        }
    }
}

#[test]
fn context_construction_requires_explicit_invocation_state() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let csharp = workspace.join("runtime/csharp/Ferrule.Runtime");
    let constructor = std::fs::read_to_string(csharp.join("ScopeContext.cs")).unwrap();
    assert!(constructor.contains(
        "private ScopeContext(\n        FerruleFunctions.JsonArrayProjectionState jsonProjections,"
    ));
    assert!(
        !constructor.contains("JsonArrayProjectionState?"),
        "descendant constructors must not silently allocate state"
    );
    let mut files = Vec::new();
    source_files(&csharp, "cs", &mut files);
    let mut allocations = 0;
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let count = text
            .matches("new FerruleFunctions.JsonArrayProjectionState()")
            .count();
        if count != 0 {
            assert_eq!(path.file_name().unwrap(), "ScopeContext.cs");
        }
        allocations += count;
    }
    assert_eq!(
        allocations, 3,
        "only FromSource, FromSources and CreateBasicRoot allocate C# invocation state"
    );
    let mut files = Vec::new();
    source_files(
        &workspace.join("crates/codegen-runtime/src"),
        "rs",
        &mut files,
    );
    let mut roots = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        for (offset, _) in text.match_indices("json_projections: Default::default()") {
            assert_eq!(
                path,
                workspace.join("crates/codegen-runtime/src/context/mod.rs")
            );
            roots.push(
                text[..offset]
                    .rsplit_once("pub fn ")
                    .unwrap()
                    .1
                    .split('(')
                    .next()
                    .unwrap()
                    .to_string(),
            );
        }
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("json_projections:"))
        {
            assert!(
                line.ends_with(".clone(),")
                    || line == "json_projections: Default::default(),"
                    || line == "json_projections: json_projection::ProjectionState,"
                    || line == "json_projections: super::json_projection::ProjectionState,",
                "unaudited Rust state construction in {}: {line}",
                path.display()
            );
        }
    }
    roots.sort();
    assert_eq!(
        roots,
        [
            "new",
            "with_execution_context",
            "with_named_inputs",
            "with_named_inputs_and_execution_context"
        ]
    );
}

fn payload_schema() -> SchemaNode {
    SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("F0", ScalarType::String)
                        .nullable()
                        .unwrap(),
                ],
            )
            .repeating(),
        ],
    )
}

fn grouped_project() -> Project {
    let mut project = super::project(1);
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Report",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::scalar("Title", ScalarType::String),
                    SchemaNode::group(
                        "Members",
                        vec![SchemaNode::scalar("Label", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes = BTreeMap::from([
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
                value: Value::String(serde_json::to_string(&payload_schema()).unwrap()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(r#"["Title"]"#.into()),
            },
        ),
        (
            3,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![0, 1, 2],
            },
        ),
        (
            4,
            Node::OwnedSourceField(mapping::OwnedSourceField {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Primary,
                    path: vec!["Rows".into(), "Rank".into()],
                },
                frame: None,
            }),
        ),
        (
            5,
            Node::OwnedSourceField(mapping::OwnedSourceField {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Primary,
                    path: vec!["Rows".into(), "Label".into()],
                },
                frame: Some(vec!["Rows".into()]),
            }),
        ),
    ]);
    let iteration = || ScopeIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: vec!["Rows".into()],
        },
        output_path: None,
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Title".into(),
            node: 3,
        }],
        children: vec![Scope {
            target_field: "Groups".into(),
            iteration: iteration(),
            group_by: Some(3),
            sort_by: Some(4),
            bindings: vec![Binding {
                target_field: "Title".into(),
                node: 3,
            }],
            children: vec![Scope {
                target_field: "Members".into(),
                iteration: iteration(),
                bindings: vec![Binding {
                    target_field: "Label".into(),
                    node: 5,
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    project
}

fn cases() -> Vec<serde_json::Value> {
    let mut cases = Vec::new();
    for rows in [0, 1, 4, 13] {
        for title in ["team", "different", "team"] {
            let source_rows: Vec<_> = (0..rows)
                .rev()
                .map(|row| serde_json::json!({"Rank":row, "Label":format!("R{row}")}))
                .collect();
            let members: Vec<_> = (0..rows)
                .map(|row| serde_json::json!({"Label":format!("R{row}")}))
                .collect();
            let groups = if rows == 0 {
                serde_json::json!([])
            } else {
                serde_json::json!([{"Title":title,"Members":members}])
            };
            cases.push(serde_json::json!({"rows":rows,"source":{"Rows":source_rows}, "input":serde_json::json!({"Title":title,"Limit":3,"Rows":[{}, {"F0":null}]}).to_string(), "expected":{"Title":title,"Groups":groups}}));
        }
    }
    cases.push(serde_json::json!({"rows":3,"source":{"Rows":[{"Rank":2,"Label":"R2"},{"Rank":1,"Label":"R1"},{"Rank":0,"Label":"R0"}]},"input":r#"{"Title":"bad-late","Limit":3,"Rows":[{"F0":"ok"},{"F0":42}]}"#,"error":"input does not match the JSON schema"}));
    cases
}

#[test]
fn sorted_grouping_interpreter_reuses_document_for_keys_and_finished_targets() {
    let project = grouped_project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    for case in cases() {
        let source = format_json::from_str(&case["source"].to_string(), &project.source).unwrap();
        let mut parameters = engine::RuntimeParameters::new();
        parameters
            .insert(
                "InputText",
                Value::String(case["input"].as_str().unwrap().into()),
            )
            .unwrap();
        let context =
            engine::ExecutionContext::new(Path::new("synthetic.mfd")).with_parameters(&parameters);
        functions::reset_json_projection_full_parse_count();
        let result = engine::run_with_context(&project, &source, &context);
        assert_eq!(functions::json_projection_full_parse_count(), 1, "{case}");
        if case.get("error").is_some() {
            assert!(matches!(
                result,
                Err(engine::EngineError::Function(
                    functions::FunctionError::InvalidArgument {
                        function: "json_parse_field",
                        message: "input does not match the JSON schema"
                    }
                ))
            ));
        } else {
            let expected =
                format_json::from_str(&case["expected"].to_string(), &project.target).unwrap();
            assert_eq!(result.unwrap(), expected, "{case}");
        }
    }
}

fn emit(root: &Path, language: &str) {
    let project = grouped_project();
    let program = codegen::lower_with_basic_owned_group_by(&project).unwrap();
    let artifacts = if language == "rust" {
        codegen_rust::emit_with_basic_owned_group_by(
            &program,
            &codegen_rust::Options {
                package_name: "json-context-mapping".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../codegen-runtime")
                        .canonicalize()
                        .unwrap()
                        .display()
                        .to_string(),
                ),
            },
        )
        .unwrap()
    } else {
        codegen_csharp::emit_with_basic_owned_group_by(&program).unwrap()
    };
    assert!(
        artifacts
            .files()
            .iter()
            .any(
                |file| String::from_utf8_lossy(&file.contents).contains(if language == "rust" {
                    "map_group_by_source_cohort"
                } else {
                    "MapGroupBySourceCohort"
                })
            ),
        "must exercise actual owned grouping emission"
    );
    for file in artifacts.files() {
        let path = root.join(language).join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
    for (name, value) in [
        ("source", &project.source),
        ("target", &project.target),
        ("parser", &payload_schema()),
    ] {
        std::fs::write(
            root.join(format!("{name}.json")),
            serde_json::to_string(value).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("cases.json"),
        serde_json::to_string(&cases()).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "compiles C# Release generated sorted grouping plus direct filter/map boundary controls"]
fn csharp_context_propagation_release_controls() {
    let dir = TempDir::new();
    emit(&dir.0, "csharp");
    let host = dir.0.join("csharp/host");
    std::fs::create_dir(&host).unwrap();
    std::fs::write(
        dir.0.join("NuGet.Config"),
        "<configuration><packageSources><clear/></packageSources></configuration>",
    )
    .unwrap();
    std::fs::write(host.join("Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
    std::fs::write(
        host.join("Program.cs"),
        include_str!("../fixtures/json-projection-work/context_host.cs"),
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
            .arg(&dir.0)
            .env("DOTNET_CLI_HOME", dir.0.join("dotnet-home"))
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1"),
    );
}

#[test]
#[ignore = "compiles Rust Release generated sorted grouping plus direct filter/map boundary parity controls"]
fn rust_context_propagation_release_controls() {
    let dir = TempDir::new();
    emit(&dir.0, "rust");
    let root = dir.0.join("rust");
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("[dependencies]", "[dependencies]\nserde_json = \"1\"")
        .replace(
            "codegen-runtime = {",
            "codegen-runtime = { features = [\"json-projection-instrumentation\"],",
        );
    std::fs::write(
        &manifest,
        format!("{text}\n[[bin]]\nname = \"context-host\"\npath = \"host.rs\"\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("host.rs"),
        include_str!("../fixtures/json-projection-work/context_host.rs"),
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
            .arg(dir.0.join("host-target"))
            .arg("--manifest-path")
            .arg(&manifest)
            .args(["--bin", "context-host", "--"])
            .arg(&dir.0)
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env("RUSTFLAGS", "-D warnings"),
    );
}
