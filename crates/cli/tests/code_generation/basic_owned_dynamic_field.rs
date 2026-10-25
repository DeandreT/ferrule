//! #467 owned dynamic source-field parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expectations.
use super::*;
use serde_json::{Value as Json, json};
use std::fs;
// These modules are the conformance corpus. Keep their bodies byte-identical to
// the authored files when formatting this harness.
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-dynamic-field/projects.rs"]
mod projects;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-dynamic-field/cases.rs"]
mod cases;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-dynamic-field/closed.rs"]
mod closed;
use super::basic_owned_sequence_item_at::codec;

const MODELS: usize = 10;
const CASES: usize = 30;
const CALLS: usize = 3 * CASES;

pub(super) struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
}

/// Controls JSON shared by every engine, plus the Project for each model.
fn corpus() -> TestResult<(Json, Vec<Project>)> {
    let models: Vec<Project> = projects::PROFILES
        .iter()
        .map(|profile| projects::project(profile))
        .collect();
    let cases = cases::cases();
    let mut ids = std::collections::BTreeSet::new();
    let mut used = std::collections::BTreeSet::new();
    let mut records = Vec::new();
    for case in &cases {
        let model = projects::PROFILES
            .iter()
            .position(|profile| *profile == case.profile)
            .ok_or_else(|| format!("unknown profile {}", case.profile))?;
        if !ids.insert(case.id) || case.calls.len() != 3 {
            return Err(format!("duplicate case or call set: {}", case.id).into());
        }
        used.insert(model);
        records.push(json!({"id":case.id,"model":model,"source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,value)|json!([name,codec::encode(value)])).collect::<Vec<_>>(),
            "calls":case.calls}));
    }
    if models.len() != MODELS || used.len() != MODELS || cases.len() != CASES {
        return Err("10 profiles with 30 cases changed".into());
    }
    Ok((json!({"version":1,"cases":records}), models))
}

fn runtime_path() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?)
}
fn rust_options(package: &str, runtime: &Path) -> codegen_rust::Options {
    codegen_rust::Options {
        package_name: package.into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    }
}

fn availability(plan: &codegen::NamedInputCatalogue<'_>) -> Vec<(String, String)> {
    plan.entries
        .iter()
        .map(|entry| (entry.name.to_owned(), format!("{:?}", entry.availability)))
        .collect()
}

/// Owned dynamic reads inside a Project's main graph.
fn owned_reads(project: &Project) -> usize {
    project
        .graph
        .nodes
        .values()
        .filter(|node| matches!(node, Node::OwnedDynamicSourceField(_)))
        .count()
}

#[test]
fn owned_dynamic467_profiles_lower_validate_and_emit() -> TestResult<()> {
    let (_, models) = corpus()?;
    let runtime = runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let profile = projects::PROFILES[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        assert!(
            engine::validate(project).is_empty(),
            "{profile}: {:?}",
            engine::validate(project)
        );
        let wire = mapping::project_file::encode_pretty(project)?;
        assert!(wire.contains("\"owned_dynamic_source_field\""), "{profile}");
        assert!(!wire.contains("\"dynamic_source_field\""), "{profile}");
        let decoded = mapping::project_file::decode_str(&wire)?;
        assert_eq!(
            mapping::project_file::encode_pretty(&decoded)?,
            wire,
            "{profile}"
        );
        let program =
            codegen::lower_with_basic_owned_dynamic_field(project).map_err(|e| context(&e))?;
        let original = program.clone();
        codegen::validate_program_with_basic_owned_dynamic_field(&program)
            .map_err(|e| context(&e))?;
        codegen::validate_basic_owned_dynamic_field_emission(&program).map_err(|e| context(&e))?;
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(&program)
                .map_err(|e| context(&e))?;
        // Owned objects are demand-driven, like every owned read.
        let expected = vec![
            ("Lookup".to_owned(), "Optional".to_owned()),
            ("Settings".to_owned(), "Optional".to_owned()),
            ("Tree".to_owned(), "Optional".to_owned()),
        ];
        assert_eq!(availability(&plan.all_outputs), expected, "{profile}");
        assert_eq!(availability(&plan.primary), expected, "{profile}");
        assert_eq!(
            availability(&plan.named_targets[0].catalogue),
            expected,
            "{profile}"
        );
        let rust = codegen_rust::emit_with_basic_owned_dynamic_field(
            &program,
            &rust_options(&format!("dynamic467-model{index}"), &runtime),
        )
        .map_err(|e| context(&e))?;
        let csharp = codegen_csharp::emit_with_basic_owned_dynamic_field(&program)
            .map_err(|e| context(&e))?;
        let text = |set: &codegen::ArtifactSet, suffix: &str| {
            set.files()
                .iter()
                .filter(|file| file.path.as_str().ends_with(suffix))
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        let reads = owned_reads(project);
        assert_eq!(
            text(&rust, "lib.rs")
                .matches("context.owned_dynamic_field(")
                .count(),
            reads,
            "{profile}"
        );
        assert_eq!(
            text(&csharp, "GeneratedMapping.cs")
                .matches("context.ResolveOwnedDynamicField(")
                .count(),
            reads,
            "{profile}"
        );
        assert_eq!(program, original, "{profile}");
    }
    Ok(())
}

fn expected_engine_error(recipe: &Json) -> engine::EngineError {
    let node = recipe["node"].as_u64().map(|node| node as u32);
    let owner = || projects::owner(recipe["owner"].as_str().unwrap_or_default());
    let strings = |key: &str| {
        recipe[key]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .map(|part| part.as_str().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    match (recipe["kind"].as_str(), recipe["cause"].as_str()) {
        (Some("MappingException"), _) => engine::EngineError::MappingException {
            node: node.unwrap_or_default(),
            message: None,
        },
        (Some("OwnedSource"), Some(cause)) => engine::EngineError::OwnedSourceAccess {
            node,
            scope: None,
            source: match cause {
                "SourceUnavailable" => {
                    engine::OwnedSourceError::SourceUnavailable { owner: owner() }
                }
                "UniteratedRepetition" => engine::OwnedSourceError::UniteratedOwnedRepetition {
                    path: strings("path"),
                },
                "InactiveSourceFrame" => engine::OwnedSourceError::InactiveSourceFrame {
                    owner: owner(),
                    collection: Some(strings("collection")),
                },
                _ => panic!("authored owned cause"),
            },
        },
        _ => panic!("authored error recipe"),
    }
}

#[test]
fn owned_dynamic467_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let execution = engine::ExecutionContext::new(Path::new("dynamic467.mapping"));
    let mut differing = Vec::new();
    let mut calls = 0;
    for case in controls["cases"].as_array().ok_or("cases")? {
        let project = &models[case["model"].as_u64().ok_or("model")? as usize];
        let source = codec::decode(&case["source"]);
        let inputs: Vec<(String, Instance)> = case["inputs"]
            .as_array()
            .ok_or("inputs")?
            .iter()
            .map(|input| {
                (
                    input[0].as_str().unwrap_or_default().to_owned(),
                    codec::decode(&input[1]),
                )
            })
            .collect();
        for call in case["calls"].as_array().ok_or("calls")? {
            calls += 1;
            let selection = call["selection"].as_str().ok_or("selection")?;
            let actual = if selection == "All" {
                engine::run_outputs_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                )
                .map(|outputs| {
                    std::iter::once(json!([null, codec::encode(&outputs.primary)]))
                        .chain(
                            outputs
                                .extras
                                .iter()
                                .map(|named| json!([named.name, codec::encode(&named.instance)])),
                        )
                        .collect::<Vec<_>>()
                })
            } else {
                let target = if selection == "Primary" {
                    engine::TargetSelection::Primary
                } else {
                    engine::TargetSelection::Named(selection)
                };
                engine::run_selected_target_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                    target,
                )
                .map(|output| match output {
                    engine::SelectedTargetOutput::Primary(instance) => {
                        vec![json!([null, codec::encode(&instance)])]
                    }
                    engine::SelectedTargetOutput::Named(named) => {
                        vec![json!([named.name, codec::encode(&named.instance)])]
                    }
                })
            };
            let matched = match (&actual, call["expected"].get("Ok")) {
                (Ok(trees), Some(expected)) => json!(trees) == *expected,
                (Err(error), None) => *error == expected_engine_error(&call["expected"]["Err"]),
                _ => false,
            };
            if !matched {
                differing.push(format!("{} {selection}: {actual:?}", case["id"]));
            }
        }
    }
    assert_eq!(calls, CALLS);
    assert!(differing.is_empty(), "{differing:#?}");
    Ok(())
}

#[test]
fn owned_dynamic467_older_profiles_and_closed_controls_refuse() -> TestResult<()> {
    // Older generation profiles keep refusing the owned read at its node.
    let project = projects::project("D02-named-object");
    let program = codegen::lower_with_basic_owned_dynamic_field(&project)?;
    let expected = closed::lookup_read_closed();
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = &expected else {
        unreachable!("authored owned refusal");
    };
    let diagnostic = codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    };
    for result in closed::old_lower(&project) {
        assert!(
            matches!(&result, Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic)),
            "{result:?}"
        );
    }
    for result in closed::old_validators(&program)
        .into_iter()
        .chain(closed::old_catalogues(&program))
    {
        assert_eq!(result, Err(expected.clone()));
    }
    let options = rust_options("dynamic467-old", &runtime_path()?);
    for result in closed::old_rust(&program, &options) {
        assert!(
            matches!(&result, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
            "{result:?}"
        );
    }
    for result in closed::old_csharp(&program) {
        assert!(
            matches!(&result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
            "{result:?}"
        );
    }

    // Unsupported owned-read Projects refuse before artifacts exist.
    for (id, project, expected) in closed::projects() {
        let actual = codegen::lower_with_basic_owned_dynamic_field(&project);
        assert!(
            matches!(&actual, Err(error) if error.diagnostics() == expected.as_slice()),
            "{id}: {actual:?}"
        );
    }

    // Directly constructed Programs are refused by every new-profile entry.
    for (id, program, expected) in closed::programs(&program) {
        assert_eq!(
            codegen::validate_program_with_basic_owned_dynamic_field(&program),
            Err(expected.clone()),
            "{id}"
        );
        assert_eq!(
            codegen::validate_basic_owned_dynamic_field_emission(&program),
            Err(expected.clone()),
            "{id}"
        );
        assert_eq!(
            codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(&program)
                .map(drop),
            Err(expected.clone()),
            "{id}"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_dynamic_field(&program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_dynamic_field(&program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == expected),
            "{id} C#"
        );
        assert!(codegen::validate_program(&program).is_err(), "{id}");
    }
    Ok(())
}

#[test]
fn owned_dynamic467_legacy_dynamic_field_keeps_wire_and_generation() -> TestResult<()> {
    // The legacy form over the same schema keeps its wire, outward fallback and
    // ordinary emitters.
    let mut project = projects::base();
    project.extra_targets.clear();
    project.graph.nodes.retain(|id, _| *id < 20);
    project.graph.nodes.insert(
        60,
        Node::DynamicSourceField {
            object: projects::path(&["Props"]),
            frame: None,
            key: projects::COLOR,
        },
    );
    project.target = SchemaNode::group(
        "Out",
        vec![
            SchemaNode::group("Items", vec![SchemaNode::scalar("V1", ScalarType::String)])
                .repeating(),
        ],
    );
    project.root = Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: ScopeIteration::Source(projects::path(&["Rows"])),
            bindings: vec![Binding {
                target_field: "V1".into(),
                node: 60,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert!(!wire.contains("owned_dynamic_source_field"));
    assert!(wire.contains("\"kind\": \"dynamic_source_field\""));
    let program = codegen::lower(&project)?;
    let options = rust_options("dynamic467-legacy", &runtime_path()?);
    codegen_rust::emit(&program, &options)?;
    codegen_csharp::emit(&program)?;
    // A row without an object falls back to the root object.
    let source = cases::group(vec![
        ("Props", cases::group(vec![("color", cases::text("P.red"))])),
        (
            "Rows",
            Instance::Repeated(vec![
                cases::group(vec![(
                    "Props",
                    cases::group(vec![("color", cases::text("P.row1"))]),
                )]),
                cases::group(vec![("Id", cases::text("P2"))]),
            ]),
        ),
    ]);
    let row = |value: &str| cases::group(vec![("V1", cases::text(value))]);
    assert_eq!(
        engine::run(&project, &source)?,
        cases::group(vec![(
            "Items",
            Instance::Repeated(vec![row("P.row1"), row("P.red")]),
        )])
    );
    assert_eq!(mapping::project_file::encode_pretty(&project)?, wire);
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> TestResult<()> {
    fs::create_dir_all(path.parent().ok_or("file parent")?)?;
    fs::write(path, bytes)?;
    Ok(())
}
fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.files() {
        write_file(&directory.join(file.path.as_str()), &file.contents)?;
    }
    Ok(())
}
fn host_project(model: usize) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
fn model_calls(controls: &Json, model: usize) -> usize {
    controls["cases"]
        .as_array()
        .map(|cases| {
            cases
                .iter()
                .filter(|case| case["model"] == model)
                .map(|case| case["calls"].as_array().map_or(0, Vec::len))
                .sum()
        })
        .unwrap_or_default()
}
/// Each host compares its calls itself; also require its complete row count.
fn host_result(results: &Path, calls: usize, run: &Output) -> Result<(), String> {
    let rows: Vec<Json> = fs::read(results)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .unwrap_or_default();
    let matched = rows.iter().filter(|row| row["matched"] == true).count();
    if run.status.success() && rows.len() == calls && matched == calls {
        return Ok(());
    }
    let differing: Vec<_> = rows.iter().filter(|row| row["matched"] != true).collect();
    Err(format!(
        "{}: status {:?}, {matched}/{} matched of {calls}\n{differing:#?}\n{}{}",
        results.display(),
        run.status,
        rows.len(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    ))
}

#[test]
#[ignore = "builds and runs one generated Rust host and 10 generated C# hosts; requires cargo and the .NET 10 SDK"]
fn owned_dynamic467_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let runtime = runtime_path()?;
    let temp = TempDir::new("dynamic467_hosts")?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(&controls)?,
    )?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in models.iter().enumerate() {
        let program = codegen::lower_with_basic_owned_dynamic_field(project)?;
        let options = rust_options(&format!("dynamic467-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_dynamic_field(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_dynamic_field(&program)?,
        )?;
        let host = root.join(format!("csharp-host{index}"));
        write_file(&host.join("Host.csproj"), host_project(index).as_bytes())?;
        write_file(
            &host.join("Program.cs"),
            include_str!("basic_owned_dynamic_field/Host.cs.txt")
                .replace("@MODEL_CALLS@", &model_calls(&controls, index).to_string())
                .as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"dynamic467-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "            {index} => invoke!(model{index}, selection, &source, &inputs, &execution, limits),\n"
        ));
    }
    write_file(
        &root.join("rust-host/src/main.rs"),
        include_str!("basic_owned_dynamic_field/Host.rs.txt")
            .replace("@DISPATCH@", &dispatch)
            .replace("@CALLS@", &CALLS.to_string())
            .as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/codec.rs"),
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    write_file(
        &root.join("rust-host/Cargo.toml"),
        format!(
            "[package]\nname=\"dynamic467-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime.display().to_string())?
        )
        .as_bytes(),
    )?;

    let mut failures = Vec::new();
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| root.join("target"));
    let build = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--manifest-path"])
        .arg(root.join("rust-host/Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()?;
    if build.status.success() {
        let results = root.join("rust-results.json");
        let run = Command::new(target.join("debug/dynamic467-host"))
            .arg(root.join("controls.json"))
            .arg(&results)
            .output()?;
        if let Err(error) = host_result(&results, CALLS, &run) {
            failures.push(format!("Rust host: {error}"));
        }
    } else {
        failures.push(format!(
            "Rust host build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }

    // Each C# model has its own Ferrule.Generated assembly, so each needs a host.
    let dotnet = std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH")?)
        .map(|directory| directory.join("dotnet"))
        .find(|candidate| candidate.is_file())
        .ok_or("dotnet not found on PATH")?
        .canonicalize()?;
    let next = AtomicUsize::new(0);
    let csharp_failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= MODELS {
                        break;
                    }
                    if let Err(error) = run_csharp_host(&root, &dotnet, &controls, index)
                        && let Ok(mut failures) = csharp_failures.lock()
                    {
                        failures.push(error);
                    }
                }
            });
        }
    });
    let mut csharp_failures = csharp_failures
        .into_inner()
        .map_err(|_| "C# worker panicked")?;
    csharp_failures.sort();
    failures.extend(csharp_failures);
    if !failures.is_empty() {
        return Err(failures.join("\n\n").into());
    }
    Ok(())
}

fn run_csharp_host(
    root: &Path,
    dotnet: &Path,
    controls: &Json,
    index: usize,
) -> Result<(), String> {
    let artifacts = root.join(format!("dotnet-artifacts-model{index}"));
    let failed = |stage: &str, output: &Output| {
        format!(
            "C# model{index} {stage}: {:?}\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };
    let build = Command::new(dotnet)
        .dotnet_temp(root)
        .args([
            "build",
            "--configuration",
            "Release",
            "--nologo",
            "--verbosity",
            "quiet",
            "-nodeReuse:false",
            "-p:UseSharedCompilation=false",
            "--artifacts-path",
        ])
        .arg(&artifacts)
        .arg(root.join(format!("csharp-host{index}/Host.csproj")))
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .output()
        .map_err(|error| format!("C# model{index} build: {error}"))?;
    if !build.status.success() {
        return Err(failed("build", &build));
    }
    let results = root.join(format!("csharp-model{index}-results.json"));
    let run = Command::new(dotnet)
        .dotnet_temp(root)
        .arg(artifacts.join("bin/Host/release/Host.dll"))
        .arg(root.join("controls.json"))
        .arg(index.to_string())
        .arg(&results)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .output()
        .map_err(|error| format!("C# model{index} run: {error}"))?;
    host_result(&results, model_calls(controls, index), &run)
        .map_err(|error| format!("C# model{index}: {error}"))
}

/// The models, the closed Projects and the directly constructed Programs, for
/// the owned capability preset record.
pub(super) fn capability_corpus() -> TestResult<super::owned_capability_presets::Corpus> {
    use super::owned_capability_presets as presets;
    let mut corpus = presets::projects(
        projects::PROFILES
            .iter()
            .map(|profile| (*profile, projects::project(profile))),
    );
    corpus.extend(presets::projects(
        closed::projects()
            .into_iter()
            .map(|(id, project, _)| (format!("closed/{id}"), project)),
    ));
    let base =
        codegen::lower_with_basic_owned_dynamic_field(&projects::project("D02-named-object"))?;
    corpus.extend(presets::programs(
        closed::programs(&base)
            .into_iter()
            .map(|(id, program, _)| (format!("direct/{id}"), program)),
    ));
    Ok(corpus)
}
