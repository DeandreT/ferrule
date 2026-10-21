//! #550 owned reads in scalar target construction: the interpreter, generated
//! Rust and generated C# run the same conformance corpus against its
//! expectations, and every Basic generation companion admits it.
use super::*;
use serde_json::{Value as Json, json};
use std::fs;
// These modules are the conformance corpus. Keep their bodies byte-identical to
// the authored files when formatting this harness.
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-scalar-construction/projects.rs"]
mod projects;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-scalar-construction/cases.rs"]
mod cases;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-scalar-construction/closed.rs"]
mod closed;
use super::basic_owned_sequence_item_at::codec;

const MODELS: usize = 9;
const CASES: usize = 25;
const CALLS: usize = 76;

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
        if !ids.insert(case.id) || case.calls.is_empty() {
            return Err(format!("duplicate case or empty call set: {}", case.id).into());
        }
        used.insert(model);
        records.push(json!({"id":case.id,"model":model,"source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,value)|json!([name,codec::encode(value)])).collect::<Vec<_>>(),
            "calls":case.calls}));
    }
    let calls = cases.iter().map(|case| case.calls.len()).sum::<usize>();
    if models.len() != MODELS || used.len() != MODELS || cases.len() != CASES || calls != CALLS {
        return Err(format!(
            "corpus changed: {} profiles, {} used, {} cases, {calls} calls",
            models.len(),
            used.len(),
            cases.len()
        )
        .into());
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

/// Scalar target scopes in every target of a Project.
fn scalar_scopes(project: &Project) -> usize {
    fn count(scope: &Scope) -> usize {
        usize::from(matches!(
            scope.construction,
            mapping::ScopeConstruction::Scalar { .. }
        )) + scope.children.iter().map(count).sum::<usize>()
    }
    count(&project.root)
        + project
            .extra_targets
            .iter()
            .map(|target| count(&target.root))
            .sum::<usize>()
}

#[test]
fn owned_scalar550_every_basic_companion_admits_and_emits() -> TestResult<()> {
    let (_, models) = corpus()?;
    let runtime = runtime_path()?;
    let options = rust_options("scalar550", &runtime);
    for (index, project) in models.iter().enumerate() {
        let profile = projects::PROFILES[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        assert!(
            engine::validate(project).is_empty(),
            "{profile}: {:?}",
            engine::validate(project)
        );
        let wire = mapping::project_file::encode_pretty(project)?;
        let decoded = mapping::project_file::decode_str(&wire)?;
        assert_eq!(
            mapping::project_file::encode_pretty(&decoded)?,
            wire,
            "{profile}"
        );
        assert!(scalar_scopes(&decoded) > 0, "{profile}");
        // Scalar construction is common Basic: every companion inherits it.
        let program = codegen::lower(project).map_err(|e| context(&e))?;
        for lowered in closed::lowerers(project) {
            assert_eq!(lowered.map_err(|e| context(&e))?, program, "{profile}");
        }
        for result in closed::validators(&program) {
            result.map_err(|e| context(&e))?;
        }
        let expected = program
            .extra_sources
            .iter()
            .map(|source| (source.name.clone(), "Optional".to_owned()))
            .collect::<Vec<_>>();
        for plan in closed::catalogues(&program) {
            let plan = plan.map_err(|e| context(&e))?;
            // Owned reads are demand-driven; no legacy read makes one required.
            let legacy = if profile == "S02-named-collision" {
                program
                    .extra_sources
                    .iter()
                    .map(|source| {
                        let state = if source.name == projects::REF {
                            "RequiredLegacy"
                        } else {
                            "Optional"
                        };
                        (source.name.clone(), state.to_owned())
                    })
                    .collect()
            } else {
                expected.clone()
            };
            assert_eq!(availability(&plan.all_outputs), legacy, "{profile}");
            assert_eq!(availability(&plan.primary), legacy, "{profile}");
        }
        let rust = closed::rust(&program, &options);
        let csharp = closed::csharp(&program);
        let text = |set: &codegen::ArtifactSet, suffix: &str| {
            set.files()
                .iter()
                .filter(|file| file.path.as_str().ends_with(suffix))
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        for set in rust {
            let set = set.map_err(|e| context(&e))?;
            let source = text(&set, "lib.rs");
            assert!(
                source.contains("execute_with_basic_owned_sources"),
                "{profile}"
            );
            if profile == "S03-literal-names" {
                assert!(
                    source.contains(r#"SourceOwnerRef::Named(" Ref \u{96ea} ")"#),
                    "{profile}"
                );
            }
        }
        for set in csharp {
            let set = set.map_err(|e| context(&e))?;
            let source = text(&set, "GeneratedMapping.cs");
            assert!(source.contains("ExecuteWithBasicOwnedSources"), "{profile}");
            if profile == "S03-literal-names" {
                assert!(source.contains(r#"Named(" Ref \u96EA ")"#), "{profile}");
            }
        }
        // The ordinary emitters keep refusing every owned read before artifacts.
        let refused =
            closed::ordinary_refusal(&program).ok_or_else(|| context(&"no owned read"))?;
        assert!(
            matches!(codegen_rust::emit(&program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == refused),
            "{profile}"
        );
        assert!(
            matches!(codegen_csharp::emit(&program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == refused),
            "{profile}"
        );
    }
    Ok(())
}

fn scope(recipe: &Json) -> Option<Box<mapping::SourceScopeLocation>> {
    let steps = recipe.get("scope")?.as_array()?;
    let target = match steps.first()?.as_str()? {
        "Primary" => mapping::SourceScopeTarget::Primary,
        name => mapping::SourceScopeTarget::Named {
            index: 0,
            name: name.into(),
        },
    };
    Some(Box::new(steps.iter().skip(1).fold(
        mapping::SourceScopeLocation::root(target),
        |route, step| {
            route.descendant(mapping::SourceScopeStep::Child(
                step.as_u64().unwrap_or_default() as usize,
            ))
        },
    )))
}

fn expected_engine_error(recipe: &Json) -> engine::EngineError {
    let text = |key: &str| recipe[key].as_str().unwrap_or_default();
    let node = || recipe["node"].as_u64().map(|node| node as u32);
    match text("kind") {
        "SourceUnavailable" => engine::EngineError::OwnedSourceAccess {
            node: node(),
            scope: scope(recipe),
            source: engine::OwnedSourceError::SourceUnavailable {
                owner: projects::owner(text("owner")),
            },
        },
        "Traversal" => engine::EngineError::OwnedSourceAccess {
            node: node(),
            scope: scope(recipe),
            source: engine::OwnedSourceError::MalformedOwnedTraversal {
                path: recipe["path"]
                    .as_array()
                    .map(|parts| {
                        parts
                            .iter()
                            .map(|part| part.as_str().unwrap_or_default().to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                found: match text("found") {
                    "scalar" => "a non-null scalar",
                    _ => panic!("authored found kind"),
                },
            },
        },
        "NotABool" => engine::EngineError::NotABool {
            node: node().unwrap_or_default(),
            found: match text("found") {
                "string" => "string",
                _ => panic!("authored found type"),
            },
        },
        _ => panic!("authored error recipe"),
    }
}

fn outputs(trees: &[(Option<String>, Instance)]) -> Json {
    json!(
        trees
            .iter()
            .map(|(name, tree)| json!([name, codec::encode(tree)]))
            .collect::<Vec<_>>()
    )
}

#[test]
fn owned_scalar550_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let execution = engine::ExecutionContext::new(Path::new("scalar550.mapping"));
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
            let actual: Result<Vec<(Option<String>, Instance)>, _> = if selection == "All" {
                engine::run_outputs_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                )
                .map(|outputs| {
                    std::iter::once((None, outputs.primary))
                        .chain(
                            outputs
                                .extras
                                .into_iter()
                                .map(|named| (Some(named.name), named.instance)),
                        )
                        .collect()
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
                    engine::SelectedTargetOutput::Primary(instance) => vec![(None, instance)],
                    engine::SelectedTargetOutput::Named(named) => {
                        vec![(Some(named.name), named.instance)]
                    }
                })
            };
            // The interpreter has no owned-source limits of its own.
            let expected = call.get("native").unwrap_or(&call["expected"]);
            let matched = match &actual {
                Ok(trees) => expected.get("Ok") == Some(&outputs(trees)),
                Err(error) => {
                    expected.get("Err").is_some()
                        && *error == expected_engine_error(&expected["Err"])
                }
            };
            if !matched {
                let actual = actual.as_ref().map(|trees| outputs(trees));
                differing.push(format!("{} {selection}: {actual:?}", case["id"]));
            }
        }
    }
    assert_eq!(calls, CALLS);
    assert!(differing.is_empty(), "{differing:#?}");
    Ok(())
}

#[test]
fn owned_scalar550_specialized_contexts_keep_their_refusals() -> TestResult<()> {
    let options = rust_options("scalar550-closed", &runtime_path()?);
    for (id, project, expected) in closed::projects() {
        let codegen::ProgramValidationError::OwnedSource { diagnostic } = &expected else {
            return Err(format!("{id}: authored owned refusal").into());
        };
        let diagnostic = codegen::Diagnostic::OwnedSource {
            diagnostic: diagnostic.clone(),
        };
        // A companion refuses while lowering, or else its emission gate refuses
        // the lowered Program before any artifact exists.
        for (route, result) in closed::closed_lowerers(id, &project)
            .into_iter()
            .enumerate()
        {
            match result {
                Err(error) => assert_eq!(
                    error.diagnostics(),
                    std::slice::from_ref(&diagnostic),
                    "{id} route {route}"
                ),
                Ok(program) => {
                    let rust = closed::rust(&program, &options);
                    let csharp = closed::csharp(&program);
                    assert!(
                        matches!(&rust[route], Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
                        "{id} route {route}: {:?}",
                        rust[route].as_ref().map(drop)
                    );
                    assert!(
                        matches!(&csharp[route], Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
                        "{id} route {route}: {:?}",
                        csharp[route].as_ref().map(drop)
                    );
                }
            }
        }
    }
    // Directly constructed Programs keep structural and dynamic-owner causes.
    let base = codegen::lower(&projects::project("S01-primary-root"))?;
    for (id, program, expected) in closed::programs(&base) {
        for result in closed::validators(&program) {
            assert_eq!(result, Err(expected.clone()), "{id}");
        }
        for result in closed::rust(&program, &options) {
            assert!(
                matches!(&result, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
                "{id}: {result:?}"
            );
        }
        for result in closed::csharp(&program) {
            assert!(
                matches!(&result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
                "{id}: {result:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn owned_scalar550_ordinary_scalar_construction_keeps_its_route() -> TestResult<()> {
    // The same scalar targets with legacy reads keep the ordinary emitters and
    // never reach the paired runtime.
    let mut project = projects::project("S01-primary-root");
    for node in project.graph.nodes.values_mut() {
        if let Node::OwnedSourceField(field) = node {
            let mut path = match &field.source.owner {
                mapping::SourceOwner::Primary => Vec::new(),
                mapping::SourceOwner::Named { name } => vec![name.clone()],
            };
            path.extend(field.source.path.iter().cloned());
            *node = Node::SourceField { path, frame: None };
        }
    }
    let program = codegen::lower(&project)?;
    for lowered in closed::lowerers(&project) {
        assert_eq!(lowered?, program);
    }
    let options = rust_options("scalar550-ordinary", &runtime_path()?);
    let rust = codegen_rust::emit(&program, &options)?;
    let csharp = codegen_csharp::emit(&program)?;
    for (set, mapping, entry) in [
        (&rust, "src/lib.rs", "execute_with_basic_owned_sources"),
        (
            &csharp,
            "GeneratedMapping.cs",
            "ExecuteWithBasicOwnedSources",
        ),
    ] {
        let file = set
            .files()
            .iter()
            .find(|file| file.path.as_str().ends_with(mapping))
            .ok_or("generated mapping")?;
        let text = String::from_utf8_lossy(&file.contents);
        assert!(!text.contains("BasicSourceRoots"), "{mapping}");
        assert!(!text.contains(entry), "{mapping}");
    }
    // The legacy `Ref/Value` read prefers Primary's colliding `Ref` group,
    // where the owned audit read selects the named endpoint.
    let output = engine::run_outputs_with_sources_and_context(
        &project,
        &cases::primary(),
        vec![(projects::REF.to_owned(), cases::primary())],
        &engine::ExecutionContext::new(Path::new("scalar550.mapping")),
    )?;
    assert_eq!(output.primary, cases::text("P"));
    assert_eq!(output.extras.len(), 1);
    assert_eq!(output.extras[0].instance, cases::text("P.ref"));
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
#[ignore = "builds and runs one generated Rust host and 9 generated C# hosts; requires cargo and the .NET 10 SDK"]
fn owned_scalar550_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let runtime = runtime_path()?;
    let temp = TempDir::new("scalar550_hosts")?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(&controls)?,
    )?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in models.iter().enumerate() {
        let program = codegen::lower(project)?;
        let options = rust_options(&format!("scalar550-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_sources(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_sources(&program)?,
        )?;
        let host = root.join(format!("csharp-host{index}"));
        write_file(&host.join("Host.csproj"), host_project(index).as_bytes())?;
        write_file(
            &host.join("Program.cs"),
            include_str!("basic_owned_scalar_construction/Host.cs.txt")
                .replace("@MODEL_CALLS@", &model_calls(&controls, index).to_string())
                .as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"scalar550-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "            {index} => invoke!(model{index}, selection, &source, &inputs, &execution, limits),\n"
        ));
    }
    write_file(
        &root.join("rust-host/src/main.rs"),
        include_str!("basic_owned_scalar_construction/Host.rs.txt")
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
            "[package]\nname=\"scalar550-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
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
        let run = Command::new(target.join("debug/scalar550-host"))
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
