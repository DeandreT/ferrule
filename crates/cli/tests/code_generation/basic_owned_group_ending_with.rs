//! #444 owned `group_ending_with` parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expected results.
use super::*;
use serde_json::{Value as Json, json};
use std::fs;

#[path = "../../../../conformance/basic-owned-group-ending-with/cases.rs"]
mod cases;
#[path = "../../../../conformance/basic-owned-group-ending-with/closed.rs"]
mod closed;
#[path = "../../../../conformance/basic-owned-group-ending-with/projects.rs"]
mod projects;
// Reuse the #473 codec and native runner rather than a second implementation.
use super::basic_owned_sequence_item_at::{codec, run_native_with_input_after as run_native};

const CORPUS: &str = "../../conformance/basic-owned-group-ending-with";
const LEGACY_ID: &str = "L01-ordinary-ending";
const LEGACY_CALLS: usize = 2;

struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
    native: bool,
}

/// Controls JSON for the shared runners, the Projects by model and their names.
fn corpus() -> TestResult<(Json, Vec<Project>, Vec<String>)> {
    let cases = cases::cases();
    let mut ids = std::collections::BTreeSet::new();
    let mut profiles = Vec::<String>::new();
    let mut models = Vec::new();
    let mut records = Vec::new();
    for case in &cases {
        if !ids.insert(case.id) {
            return Err(format!("duplicate case {}", case.id).into());
        }
        let model = match profiles.iter().position(|p| p == case.profile) {
            Some(index) => index,
            None => {
                profiles.push(case.profile.into());
                models.push(projects::project(case.profile));
                profiles.len() - 1
            }
        };
        records.push(json!({"id":case.id,"profile":case.profile,"model":model,"native":case.native,
            "source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,value)|json!([name,codec::encode(value)])).collect::<Vec<_>>(),
            "calls":case.calls}));
    }
    // Every authored profile has at least one case, and only those.
    let mut declared = projects::PROFILES.to_vec();
    declared.sort_unstable();
    let mut used: Vec<_> = profiles.iter().map(String::as_str).collect();
    used.sort_unstable();
    if declared != used {
        return Err("authored profiles and cases differ".into());
    }
    Ok((
        json!({"version":1,"cases":records,"models":profiles}),
        models,
        profiles,
    ))
}
fn calls(controls: &Json) -> usize {
    controls["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["calls"].as_array().unwrap().len())
        .sum()
}
fn model_calls(controls: &Json, model: usize) -> usize {
    controls["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["model"] == model)
        .map(|case| case["calls"].as_array().unwrap().len())
        .sum()
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
fn actual_catalogues(plan: &codegen::NamedInputAdmissionPlan<'_>) -> Json {
    fn catalogue(value: &codegen::NamedInputCatalogue<'_>) -> Json {
        json!(
            value
                .entries
                .iter()
                .map(|entry| json!({"declaration_index":entry.declaration_index,
                    "name":entry.name,"availability":format!("{:?}",entry.availability)}))
                .collect::<Vec<_>>()
        )
    }
    json!({"all_outputs":catalogue(&plan.all_outputs),"primary":catalogue(&plan.primary),
        "named_targets":plan.named_targets.iter().map(|target|json!({"target_index":target.target_index,
            "name":target.name,"catalogue":catalogue(&target.catalogue)})).collect::<Vec<_>>()})
}
/// Every profile keeps the original declaration ordinals; owned reads stay
/// optional and the dynamic declaration stays reserved.
fn expected_catalogues() -> Json {
    let entries = json!([
        {"declaration_index":0,"name":"unusedStatic","availability":"Optional"},
        {"declaration_index":1,"name":"Rows","availability":"Optional"},
        {"declaration_index":2,"name":projects::EXACT,"availability":"Optional"},
        {"declaration_index":3,"name":"unavailableDynamic","availability":"ReservedUnavailable"}
    ]);
    json!({"all_outputs":entries,"primary":entries,"named_targets":[
        {"target_index":0,"name":projects::AUDIT,"catalogue":entries},
        {"target_index":1,"name":projects::UNICODE_TARGET,"catalogue":entries}
    ]})
}
fn legacy_project() -> TestResult<Project> {
    Ok(mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-ending-with/legacy-project.json"
    ))?)
}
fn legacy_controls() -> TestResult<(Json, Project)> {
    let source: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-ending-with/legacy-input.tree.json"
    ))?;
    let expected: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-ending-with/legacy-expected.tree.json"
    ))?;
    let expected = json!({"Ok":[[null,expected]]});
    Ok((
        json!({"version":1,"cases":[{"id":LEGACY_ID,"model":0,"native":true,
            "source":source,"inputs":[],"calls":[{"selection":"All","expected":expected},
            {"selection":"Primary","expected":expected}]}]}),
        legacy_project()?,
    ))
}

// The Starting hosts are generic over the corpus; only their labels change.
fn replace_once(source: String, needle: &str, replacement: &str) -> TestResult<String> {
    if source.matches(needle).count() != 1 {
        return Err(format!("host template seam changed: {needle}").into());
    }
    Ok(source.replace(needle, replacement))
}
fn relabel(template: &str) -> String {
    template
        .replace("itemat473", "ending444")
        .replace("L01-ordinary-starting", LEGACY_ID)
}
fn rust_host(dispatch: &str, calls: usize) -> TestResult<String> {
    let template = relabel(include_str!("basic_owned_group_starting_with/Host.rs.txt"));
    let template = replace_once(template, "@DISPATCH@", dispatch)?;
    replace_once(template, "@CORE_CALLS@", &calls.to_string())
}
fn rust_legacy_control(cases: usize) -> TestResult<String> {
    replace_once(
        relabel(include_str!(
            "basic_owned_group_starting_with/legacy_control.rs.txt"
        )),
        "identity(contract,109,",
        &format!("identity(contract,{cases},"),
    )
}
fn csharp_host(calls: usize) -> TestResult<String> {
    let template = relabel(include_str!("basic_owned_group_starting_with/Host.cs.txt"))
        + "\n"
        + &relabel(include_str!(
            "basic_owned_group_starting_with/HostProof.cs.txt"
        ));
    replace_once(template, "@MODEL_CALLS@", &calls.to_string())
}
fn csharp_legacy_host(cases: usize) -> TestResult<String> {
    let host = replace_once(
        relabel(include_str!(
            "basic_owned_group_starting_with/LegacyHost.cs.txt"
        )),
        "Identity(contract, 109,",
        &format!("Identity(contract, {cases},"),
    )?;
    Ok(host
        + "\n"
        + &relabel(include_str!(
            "basic_owned_group_starting_with/HostProof.cs.txt"
        )))
}
fn host_project(model: &str) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}

#[test]
fn basic_ending444_corpus_codecs_and_host_templates() -> TestResult<()> {
    let (controls, models, profiles) = corpus()?;
    assert_eq!(models.len(), projects::PROFILES.len());
    assert_eq!(profiles.len(), models.len());
    for case in controls["cases"].as_array().unwrap() {
        assert_eq!(
            codec::encode(&codec::decode(&case["source"])),
            case["source"]
        );
        for call in case["calls"].as_array().unwrap() {
            if let Some(outputs) = call["expected"].get("Ok") {
                for output in outputs.as_array().unwrap() {
                    assert_eq!(codec::encode(&codec::decode(&output[1])), output[1]);
                }
            }
        }
    }
    let total = calls(&controls);
    assert_eq!(
        (0..models.len())
            .map(|model| model_calls(&controls, model))
            .sum::<usize>(),
        total
    );
    let cases = controls["cases"].as_array().unwrap().len();
    assert!(rust_host("dispatch", total)?.contains("ending444"));
    assert!(rust_legacy_control(cases)?.contains(LEGACY_ID));
    assert!(csharp_host(1)?.contains(LEGACY_ID));
    assert!(csharp_legacy_host(cases)?.contains(LEGACY_ID));
    Ok(())
}

#[test]
fn basic_ending444_public_profiles_lower_validate_and_emit() -> TestResult<()> {
    let (_, models, profiles) = corpus()?;
    let runtime = runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let profile = &profiles[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        let before = mapping::project_file::encode_pretty(project)?;
        let issues = engine::validate(project);
        assert!(issues.is_empty(), "{profile}: {issues:?}");
        let program = codegen::lower_with_basic_owned_group_ending_with(project)
            .map_err(|error| context(&error))?;
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{profile}"
        );
        let original = program.clone();
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&program)
                .map_err(|error| context(&error))?;
        assert_eq!(actual_catalogues(&plan), expected_catalogues(), "{profile}");
        codegen::validate_program_with_basic_owned_group_ending_with(&program)
            .map_err(|error| context(&error))?;
        codegen::validate_basic_owned_group_ending_with_emission(&program)
            .map_err(|error| context(&error))?;
        codegen_rust::emit_with_basic_owned_group_ending_with(
            &program,
            &rust_options(&format!("ending444-model{index}"), &runtime),
        )
        .map_err(|error| context(&error))?;
        codegen_csharp::emit_with_basic_owned_group_ending_with(&program)
            .map_err(|error| context(&error))?;
        assert_eq!(program, original, "{profile}");
    }
    Ok(())
}

#[test]
fn basic_ending444_refusals_and_every_earlier_profile_gate() -> TestResult<()> {
    for (id, project, expected) in closed::projects() {
        let before = mapping::project_file::encode_pretty(&project)?;
        let actual = codegen::lower_with_basic_owned_group_ending_with(&project);
        assert!(
            matches!(&actual, Err(error) if error.diagnostics() == expected.as_slice()),
            "{id}: {actual:?}"
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&project)?,
            before,
            "{id}"
        );
    }

    let base = projects::base();
    let program = codegen::lower_with_basic_owned_group_ending_with(&base)?;
    let options = rust_options("ending444-refusal", &runtime_path()?);
    for (id, direct, expected) in closed::programs(&program) {
        assert_eq!(
            codegen::validate_program_with_basic_owned_group_ending_with(&direct),
            Err(expected.clone()),
            "{id} validator"
        );
        assert_eq!(
            codegen::validate_basic_owned_group_ending_with_emission(&direct),
            Err(expected.clone()),
            "{id} emission validator"
        );
        assert!(
            matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&direct),
                Err(error) if error == expected),
            "{id} catalogue"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_group_ending_with(&direct, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_group_ending_with(&direct),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == expected),
            "{id} C#"
        );
    }

    // Every earlier generation profile, including Starting, keeps refusing
    // owned Ending with its existing typed Grouping diagnostic.
    let expected = closed::grouping_error();
    let diagnostic = closed::diagnostic(expected.clone());
    let products = closed::old_products(&base, &program, &options);
    assert_eq!(
        [
            products.lowering.len(),
            products.validators.len(),
            products.catalogues.len(),
            products.rust.len(),
            products.csharp.len()
        ],
        [9, 18, 9, 10, 10]
    );
    assert!(products.lowering.iter().all(
        |r| matches!(r, Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic))
    ));
    assert!(
        products
            .validators
            .iter()
            .all(|r| matches!(r, Err(error) if *error == expected))
    );
    assert!(
        products
            .catalogues
            .iter()
            .all(|r| matches!(r, Err(error) if *error == expected))
    );
    assert!(products.rust.iter().all(
        |r| matches!(r, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected)
    ));
    assert!(products.csharp.iter().all(
        |r| matches!(r, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected)
    ));
    Ok(())
}

#[test]
fn basic_ending444_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models, _) = corpus()?;
    let (legacy, project) = legacy_controls()?;
    let temp = TempDir::new("ending444_native")?;
    let native = run_native(&controls, &models, &temp.0.join("native"))?;
    let ordinary = run_native(&legacy, &[project], &temp.0.join("legacy"))?;
    assert_eq!(native.len(), calls(&controls));
    assert_eq!(ordinary.len(), LEGACY_CALLS);
    let differing: Vec<_> = native
        .iter()
        .chain(&ordinary)
        .filter(|row| row["matched"] != true)
        .collect();
    assert!(differing.is_empty(), "{differing:#?}");
    Ok(())
}

#[test]
fn basic_ending444_legacy_saved_project_roundtrips_and_emits() -> TestResult<()> {
    let original =
        include_str!("../../../../conformance/basic-owned-group-ending-with/legacy-project.json");
    let project = legacy_project()?;
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert_eq!(wire, original);
    assert!(!wire.contains("owned_source"));
    assert!(wire.contains("\"group_ending_with\": 2"));
    let program = codegen::lower(&project)?;
    let options = rust_options("ferrule-generated-mapping", &runtime_path()?);
    codegen_rust::emit(&program, &options)?;
    codegen_csharp::emit(&program)?;
    // The ordinary design emits identically through the Ending companion.
    let ending = codegen::lower_with_basic_owned_group_ending_with(&project)?;
    assert_eq!(ending, program);
    codegen_rust::emit_with_basic_owned_group_ending_with(&program, &options)?;
    codegen_csharp::emit_with_basic_owned_group_ending_with(&program)?;
    assert_eq!(mapping::project_file::encode_pretty(&project)?, original);
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
fn host_contract(
    root: &Path,
    label: &str,
    engine: &str,
    program: &Path,
    assembly: Option<&Path>,
    output: &Path,
) -> TestResult<PathBuf> {
    let path = root.join(format!("run-contracts/{label}.json"));
    let mut contract = json!({"version":2,"engine":engine,"run_id":format!("ending444-{label}"),
        "output_directory":output,"program":{"path":program}});
    if let Some(assembly) = assembly {
        contract["assembly"] = json!({"path":assembly});
    }
    write_file(&path, &serde_json::to_vec_pretty(&contract)?)?;
    Ok(path)
}
/// The hosts compare every call themselves; also require their complete row count.
fn host_result(output: &Path, calls: usize, run: &Output) -> Result<(), String> {
    let rows: Vec<Json> = fs::read(output.join("ALL-COMPLETE-COMPARISONS.json"))
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .unwrap_or_default();
    let matched = rows.iter().filter(|row| row["matched"] == true).count();
    if run.status.success() && rows.len() == calls && matched == calls {
        return Ok(());
    }
    Err(format!(
        "{}: status {:?}, {matched}/{} matched of {calls}\n{}{}",
        output.display(),
        run.status,
        rows.len(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    ))
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_ending444_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models, _) = corpus()?;
    let (legacy_controls, legacy) = legacy_controls()?;
    let total = calls(&controls);
    let cases = controls["cases"].as_array().unwrap().len();
    let runtime = runtime_path()?;
    let temp = TempDir::new("ending444_hosts")?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(&controls)?,
    )?;
    write_file(
        &root.join("legacy-controls.json"),
        &serde_json::to_vec_pretty(&legacy_controls)?,
    )?;

    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in models.iter().enumerate() {
        let program = codegen::lower_with_basic_owned_group_ending_with(project)?;
        let options = rust_options(&format!("ending444-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_group_ending_with(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_group_ending_with(&program)?,
        )?;
        let host = root.join(format!("csharp-host{index}"));
        write_file(
            &host.join("Host.csproj"),
            host_project(&format!("model{index}")).as_bytes(),
        )?;
        write_file(
            &host.join("Program.cs"),
            csharp_host(model_calls(&controls, index))?.as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"ending444-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!("{index}=>invoke!(model{index},selection,&source,&inputs,&execution,limits,&directory,&prefix,call,&before,&named,&identity,&raw_before),\n"));
    }

    // The ordinary saved project runs through the ordinary emitters.
    let program = codegen::lower(&legacy)?;
    write_artifacts(
        &root.join("rust-legacy"),
        &codegen_rust::emit(
            &program,
            &rust_options("ferrule-generated-mapping", &runtime),
        )?,
    )?;
    write_artifacts(
        &root.join("csharp-legacy"),
        &codegen_csharp::emit(&program)?,
    )?;
    let conformance = |name: &str| -> TestResult<Vec<u8>> {
        Ok(fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(CORPUS)
                .join(name),
        )?)
    };
    for name in ["legacy-input.json", "legacy-expected.json"] {
        let bytes = conformance(name)?;
        write_file(&root.join(name), &bytes)?;
        write_file(&root.join("rust-host/src").join(name), &bytes)?;
    }
    for name in ["legacy-input.tree.json", "legacy-expected.tree.json"] {
        write_file(&root.join("rust-host/src").join(name), &conformance(name)?)?;
    }
    write_file(
        &root.join("rust-host/src/legacy_control.rs"),
        rust_legacy_control(cases)?.as_bytes(),
    )?;
    write_file(
        &root.join("csharp-legacy-host/Host.csproj"),
        host_project("legacy").as_bytes(),
    )?;
    write_file(
        &root.join("csharp-legacy-host/Program.cs"),
        csharp_legacy_host(cases)?.as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/main.rs"),
        rust_host(&dispatch, total)?.as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/codec.rs"),
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    write_file(
        &root.join("rust-host/src/proof.rs"),
        relabel(include_str!(
            "basic_owned_group_starting_with/HostProof.rs.txt"
        ))
        .as_bytes(),
    )?;
    write_file(&root.join("rust-host/Cargo.toml"),format!("[package]\nname=\"ending444-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\nlegacy0={{package=\"ferrule-generated-mapping\",path=\"../rust-legacy\"}}\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;

    let mut failures = Vec::new();

    // One Rust host links every model and also runs the ordinary saved project.
    let target = root.join("target");
    let build = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--manifest-path"])
        .arg(root.join("rust-host/Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()?;
    if build.status.success() {
        let executable = target.join("debug/ending444-public-host").canonicalize()?;
        let output = root.join("rust-actual");
        let contract = host_contract(&root, "rust", "rust", &executable, None, &output)?;
        let run = Command::new(&executable)
            .arg(root.join("controls.json"))
            .arg(&output)
            .arg(&contract)
            .output()?;
        if let Err(error) = host_result(&output, total + LEGACY_CALLS, &run) {
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
    let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index > models.len() {
                        break;
                    }
                    let legacy = index == models.len();
                    if let Err(error) = run_csharp_host(&root, &dotnet, &controls, index, legacy) {
                        csharp_failures.lock().unwrap().push(error);
                    }
                }
            });
        }
    });
    let mut csharp_failures = csharp_failures.into_inner().unwrap();
    csharp_failures.sort();
    failures.extend(csharp_failures);
    if !failures.is_empty() {
        return Err(failures.join("\n\n").into());
    }
    Ok(())
}
/// Build and run the C# host for one model, or the ordinary saved project.
fn run_csharp_host(
    root: &Path,
    dotnet: &Path,
    controls: &Json,
    index: usize,
    legacy: bool,
) -> Result<(), String> {
    let label = if legacy {
        "legacy".to_owned()
    } else {
        format!("model{index}")
    };
    let host = if legacy {
        "csharp-legacy-host".to_owned()
    } else {
        format!("csharp-host{index}")
    };
    let artifacts = root.join(format!("dotnet-artifacts-{label}"));
    let failed = |stage: &str, output: &Output| {
        format!(
            "C# {label} {stage}: {:?}\n{}{}",
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
        .arg(root.join(&host).join("Host.csproj"))
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .output()
        .map_err(|error| format!("C# {label} build: {error}"))?;
    if !build.status.success() {
        return Err(failed("build", &build));
    }
    let assembly = artifacts.join("bin/Host/release/Host.dll");
    let output = root.join(format!("csharp-{label}-actual"));
    let (arguments, calls) = if legacy {
        (
            vec![
                root.join("legacy-input.json"),
                root.join("legacy-expected.json"),
            ],
            LEGACY_CALLS,
        )
    } else {
        (
            vec![root.join("controls.json"), PathBuf::from(index.to_string())],
            model_calls(controls, index),
        )
    };
    let contract = host_contract(
        root,
        &format!("csharp-{label}"),
        "csharp",
        dotnet,
        Some(&assembly),
        &output,
    )
    .map_err(|error| format!("C# {label} contract: {error}"))?;
    let run = Command::new(dotnet)
        .arg(&assembly)
        .args(&arguments)
        .arg(&output)
        .arg(&contract)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .output()
        .map_err(|error| format!("C# {label} run: {error}"))?;
    host_result(&output, calls, &run).map_err(|error| format!("C# {label}: {error}"))
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
    let base = codegen::lower_with_basic_owned_group_ending_with(&projects::base())?;
    corpus.extend(presets::programs(
        closed::programs(&base)
            .into_iter()
            .map(|(id, program, _)| (format!("direct/{id}"), program)),
    ));
    Ok(corpus)
}
