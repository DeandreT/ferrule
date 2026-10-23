//! Test-only execution bindings for portable conformance cases.
//!
//! A cohort supplies genuine Projects and one controls document in the shared
//! lossless recipe codec (`basic_owned_sequence_item_at/codec.rs`): every case
//! names its model, typed source, named inputs and the complete expected
//! outcome of each call. These bindings own the rest: how the interpreter and
//! the generated Rust and C# hosts are launched, where their builds are cached,
//! which environment they receive and what a run retains. Production execution
//! is unchanged. A missing toolchain fails the run instead of skipping it.
//!
//! Cache: each run builds under its own `TempDir`, or the dedicated
//! `FERRULE_CODEGEN_HOST_TARGET_DIR` (see `host_policy`). Retention: the run
//! directory, including per-call host evidence, is removed unless
//! `FERRULE_CODEGEN_KEEP_ARTIFACTS=1`, which also writes its compact manifest
//! (see `run_manifest`).
use super::*;
use serde_json::{Value as Json, json};
use std::fs;

use super::basic_owned_sequence_item_at::{codec, run_native_with_input_after};
use super::run_manifest::Recorder;

/// Bytes of host stdout/stderr quoted in one failure message.
const FAILURE_EXCERPT: usize = 32 * 1024;

/// The public lowering of one explicit generation profile.
pub(super) type Lower = fn(&Project) -> Result<codegen::Program, codegen::LowerError>;

/// The public emitters of one explicit generation profile.
#[derive(Clone, Copy)]
pub(super) struct Emitters {
    pub rust: fn(
        &codegen::Program,
        &codegen_rust::Options,
    ) -> Result<codegen::ArtifactSet, codegen_rust::EmitError>,
    pub csharp: fn(&codegen::Program) -> Result<codegen::ArtifactSet, codegen_csharp::EmitError>,
}

/// Parse a cohort's `cases.json`, resolving each case's model name to its
/// index in `models`, the order the generated hosts receive.
pub(super) fn controls(cases: &str, models: &[&str]) -> TestResult<Json> {
    let mut controls: Json = serde_json::from_str(cases)?;
    if controls["version"] != 1 {
        return Err("unknown cases version".into());
    }
    for case in controls["cases"].as_array_mut().ok_or("cases")? {
        let model = models
            .iter()
            .position(|model| case["model"] == *model)
            .ok_or_else(|| format!("{}: unknown model {}", case["id"], case["model"]))?;
        case["model"] = json!(model);
    }
    Ok(controls)
}

fn recipe_round_trips(tree: &Json) -> bool {
    codec::encode(&codec::decode(tree)) == *tree
}

/// Check the shape of resolved controls: unique case ids, lossless source,
/// input and output recipes, and exactly one expected outcome per call.
/// Returns the case ids in order.
pub(super) fn check_case_recipes(controls: &Json) -> TestResult<Vec<String>> {
    let mut ids = Vec::new();
    for case in controls["cases"].as_array().ok_or("cases")? {
        let id = case["id"].as_str().ok_or("case id")?.to_owned();
        if ids.contains(&id) {
            return Err(format!("duplicate case {id}").into());
        }
        if !recipe_round_trips(&case["source"]) {
            return Err(format!("{id}: source recipe").into());
        }
        for input in case["inputs"].as_array().ok_or("inputs")? {
            if !input[0].is_string() || !recipe_round_trips(&input[1]) {
                return Err(format!("{id}: input recipe").into());
            }
        }
        let calls = case["calls"].as_array().ok_or("calls")?;
        if calls.is_empty() {
            return Err(format!("{id}: no calls").into());
        }
        for call in calls {
            if !call["selection"].is_string() {
                return Err(format!("{id}: selection").into());
            }
            match (call["expected"].get("Ok"), call["expected"].get("Err")) {
                (Some(outputs), None) => {
                    for output in outputs.as_array().ok_or("outputs")? {
                        if !(output[0].is_null() || output[0].is_string())
                            || !recipe_round_trips(&output[1])
                        {
                            return Err(format!("{id}: output recipe").into());
                        }
                    }
                }
                (None, Some(error)) if error["kind"].is_string() => {}
                _ => return Err(format!("{id}: one expected outcome per call").into()),
            }
        }
        ids.push(id);
    }
    Ok(ids)
}

/// Every model validates without issues, passes owned-source admission,
/// lowers with `lower` and emits with `emitters`, leaving its Project unchanged.
pub(super) fn check_emission(
    tag: &str,
    names: &[&str],
    models: &[Project],
    lower: Lower,
    emitters: Emitters,
) -> TestResult<()> {
    let runtime = runtime_path()?;
    for (index, (name, project)) in names.iter().zip(models).enumerate() {
        let before = mapping::project_file::encode_pretty(project)?;
        let issues = engine::validate(project);
        if !issues.is_empty() {
            return Err(format!("{name}: {issues:?}").into());
        }
        engine::check_source_ownership_capability(project, None)
            .map_err(|error| format!("{name}: {error}"))?;
        let program = lower(project).map_err(|error| format!("{name}: {error}"))?;
        let options = rust_options(&format!("{tag}-model{index}"), &runtime);
        (emitters.rust)(&program, &options).map_err(|error| format!("{name}: {error}"))?;
        (emitters.csharp)(&program).map_err(|error| format!("{name}: {error}"))?;
        if mapping::project_file::encode_pretty(project)? != before {
            return Err(format!("{name}: lowering changed the Project").into());
        }
    }
    Ok(())
}

pub(super) fn runtime_path() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?)
}

pub(super) fn rust_options(package: &str, runtime: &Path) -> codegen_rust::Options {
    codegen_rust::Options {
        package_name: package.into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    }
}

pub(super) fn total_calls(controls: &Json) -> usize {
    controls["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|case| case["calls"].as_array().map_or(0, Vec::len))
        .sum()
}

fn model_calls(controls: &Json, model: usize) -> usize {
    controls["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|case| case["model"] == model)
        .map(|case| case["calls"].as_array().map_or(0, Vec::len))
        .sum()
}

/// Run every call through the interpreter and require each to match.
pub(super) fn check_interpreter(tag: &str, controls: &Json, models: &[Project]) -> TestResult<()> {
    let temp = TempDir::new(&format!("{tag}_native"))?;
    let native = run_native_with_input_after(controls, models, &temp.0.join("native"))?;
    Recorder::new(&temp.0).finish(&format!("{tag}-native"), &[])?;
    if native.len() != total_calls(controls) {
        return Err(format!("{tag}: {} interpreter calls ran", native.len()).into());
    }
    let differing: Vec<_> = native.iter().filter(|row| row["matched"] != true).collect();
    if !differing.is_empty() {
        return Err(format!("{tag} interpreter differs: {differing:#?}").into());
    }
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
fn replace_once(source: String, needle: &str, replacement: &str) -> TestResult<String> {
    if source.matches(needle).count() != 1 {
        return Err(format!("host template seam changed: {needle}").into());
    }
    Ok(source.replace(needle, replacement))
}
// The Starting hosts compare every call against the same recipe vocabulary.
fn rust_host(tag: &str, dispatch: &str, calls: usize) -> TestResult<String> {
    let template =
        include_str!("basic_owned_group_starting_with/Host.rs.txt").replace("itemat473", tag);
    let template = replace_once(template, "@DISPATCH@", dispatch)?;
    replace_once(template, "@CORE_CALLS@", &calls.to_string())
}
/// These corpora have no ordinary saved-project control inside the host.
const NO_LEGACY_CONTROL: &str = "use serde_json::Value as Json;\nuse std::path::Path;\npub fn run(_directory:&Path,_contract:&Json)->Result<Vec<Json>,Box<dyn std::error::Error>> {Ok(Vec::new())}\n";
fn csharp_host(tag: &str, calls: usize) -> TestResult<String> {
    let template = include_str!("basic_owned_group_starting_with/Host.cs.txt")
        .replace("itemat473", tag)
        + "\n"
        + include_str!("basic_owned_group_starting_with/HostProof.cs.txt");
    replace_once(template, "@MODEL_CALLS@", &calls.to_string())
}
fn host_project(model: &str) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
fn host_contract(
    root: &Path,
    tag: &str,
    label: &str,
    engine: &str,
    program: &Path,
    assembly: Option<&Path>,
    output: &Path,
) -> TestResult<PathBuf> {
    let path = root.join(format!("run-contracts/{label}.json"));
    let mut contract = json!({"version":2,"engine":engine,"run_id":format!("{tag}-{label}"),
        "output_directory":output,"program":{"path":program}});
    if let Some(assembly) = assembly {
        contract["assembly"] = json!({"path":assembly});
    }
    write_file(&path, &serde_json::to_vec_pretty(&contract)?)?;
    Ok(path)
}
fn excerpt(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(FAILURE_EXCERPT);
    let prefix = if start > 0 { "…" } else { "" };
    format!("{prefix}{}", String::from_utf8_lossy(&bytes[start..]))
}
fn failed(stage: &str, output: &Output) -> String {
    format!(
        "{stage}: {:?}\n{}{}",
        output.status,
        excerpt(&output.stdout),
        excerpt(&output.stderr)
    )
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
    Err(failed(
        &format!(
            "{}: {matched}/{} matched of {calls}",
            output.display(),
            rows.len()
        ),
        run,
    ))
}

/// C# exception wording is outside the shared typed comparison. A call whose
/// expected error lists `csharp_messages` also requires the retained messages
/// of that many leading exceptions: the outer one, then each inner one.
fn csharp_messages(controls: &Json, model: usize, output: &Path) -> Result<(), String> {
    let cases = controls["cases"].as_array().ok_or("cases")?;
    for (ordinal, case) in cases.iter().enumerate() {
        if case["model"] != model {
            continue;
        }
        for call in case["calls"].as_array().ok_or("calls")? {
            let Some(expected) = call["expected"]["Err"].get("csharp_messages") else {
                continue;
            };
            let selection = call["selection"].as_str().ok_or("selection")?;
            let path = output.join(format!(
                "{ordinal:02}-{selection}-COMPLETE-OBSERVATION.json"
            ));
            let record: Json = fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let expected = expected.as_array().ok_or("csharp_messages")?;
            let mut error = &record["record"]["actual"]["Err"];
            let mut actual = Vec::new();
            for _ in expected {
                actual.push(error["message"].clone());
                error = &error["inner"];
            }
            if actual != *expected {
                return Err(format!(
                    "{}: messages {} != {}",
                    path.display(),
                    Json::from(actual),
                    Json::from(expected.clone())
                ));
            }
        }
    }
    Ok(())
}

/// Emit every model with `emitters`, then build and run one generated Rust host
/// covering all models and one generated C# host per model. Every call of the
/// controls must match in both languages.
pub(super) fn check_generated_hosts(
    tag: &str,
    controls: &Json,
    programs: &[codegen::Program],
    emitters: Emitters,
) -> TestResult<()> {
    let runtime = runtime_path()?;
    let temp = TempDir::new(&format!("{tag}_hosts"))?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(controls)?,
    )?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, program) in programs.iter().enumerate() {
        let options = rust_options(&format!("{tag}-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &(emitters.rust)(program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &(emitters.csharp)(program)?,
        )?;
        let host = root.join(format!("csharp-host{index}"));
        write_file(
            &host.join("Host.csproj"),
            host_project(&format!("model{index}")).as_bytes(),
        )?;
        write_file(
            &host.join("Program.cs"),
            csharp_host(tag, model_calls(controls, index))?.as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"{tag}-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!("{index}=>invoke!(model{index},selection,&source,&inputs,&execution,limits,&directory,&prefix,call,&before,&named,&identity,&raw_before),\n"));
    }
    write_file(
        &root.join("rust-host/src/main.rs"),
        rust_host(tag, &dispatch, total_calls(controls))?.as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/codec.rs"),
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    write_file(
        &root.join("rust-host/src/proof.rs"),
        include_bytes!("basic_owned_group_starting_with/HostProof.rs.txt"),
    )?;
    write_file(
        &root.join("rust-host/src/legacy_control.rs"),
        NO_LEGACY_CONTROL.as_bytes(),
    )?;
    write_file(&root.join("rust-host/Cargo.toml"),format!("[package]\nname=\"{tag}-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;

    let recorder = Recorder::new(&root);
    let mut failures = Vec::new();
    if let Err(error) = run_rust_host(&root, tag, controls, &recorder) {
        failures.push(format!("Rust host: {error}"));
    }
    let dotnet = dotnet_executable()?;
    let next = AtomicUsize::new(0);
    let csharp_failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= programs.len() {
                        break;
                    }
                    if let Err(error) =
                        run_csharp_host(&root, tag, &dotnet, controls, index, &recorder)
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
    // The per-run Rust target and the .NET sandboxes are restorable caches.
    let mut caches = vec![PathBuf::from("cargo-target")];
    caches.extend(
        (0..programs.len()).map(|index| PathBuf::from(format!("dotnet-sandbox-model{index}"))),
    );
    recorder.finish(&format!("{tag}-hosts"), &caches)?;
    if !failures.is_empty() {
        return Err(failures.join("\n\n").into());
    }
    Ok(())
}

fn run_rust_host(root: &Path, tag: &str, controls: &Json, recorder: &Recorder) -> TestResult<()> {
    let build = host_policy::with_cargo_target(root, |target| {
        let mut command = Command::new(env!("CARGO"));
        command
            .args(["build", "--quiet", "--manifest-path"])
            .arg(root.join("rust-host/Cargo.toml"))
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_BUILD_BUILD_DIR", target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        let build = command.output()?;
        recorder.record("rust-build", &command, &build)?;
        let executable = target.join(format!("debug/{tag}-public-host"));
        io::Result::Ok((build, executable))
    })?;
    let (build, executable) = build?;
    if !build.status.success() {
        return Err(failed("build", &build).into());
    }
    let executable = executable.canonicalize()?;
    let output = root.join("rust-actual");
    let contract = host_contract(root, tag, "rust", "rust", &executable, None, &output)?;
    let mut command = Command::new(&executable);
    command
        .arg(root.join("controls.json"))
        .arg(&output)
        .arg(&contract);
    let run = command.output()?;
    recorder.record("rust-run", &command, &run)?;
    Ok(host_result(&output, total_calls(controls), &run)?)
}

fn dotnet_executable() -> TestResult<PathBuf> {
    // An absolute program keeps the hosts outside the in-process dotnet lock.
    Ok(
        std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH")?)
            .map(|directory| directory.join("dotnet"))
            .find(|candidate| candidate.is_file())
            .ok_or("dotnet not found on PATH")?
            .canonicalize()?,
    )
}

fn run_csharp_host(
    root: &Path,
    tag: &str,
    dotnet: &Path,
    controls: &Json,
    index: usize,
    recorder: &Recorder,
) -> Result<(), String> {
    let label = format!("model{index}");
    let artifacts = root.join(format!("dotnet-artifacts-{label}"));
    let sandbox = root.join(format!("dotnet-sandbox-{label}"));
    let mut command = dotnet_command_at(dotnet.as_os_str(), &sandbox);
    command
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
        .arg(root.join(format!("csharp-host{index}/Host.csproj")));
    let build = command
        .output()
        .map_err(|error| format!("C# {label} build: {error}"))?;
    recorder
        .record(&format!("csharp-{label}-build"), &command, &build)
        .map_err(|error| format!("C# {label} build record: {error}"))?;
    if !build.status.success() {
        return Err(format!("C# {label} {}", failed("build", &build)));
    }
    let assembly = artifacts.join("bin/Host/release/Host.dll");
    let output = root.join(format!("csharp-{label}-actual"));
    let contract = host_contract(
        root,
        tag,
        &format!("csharp-{label}"),
        "csharp",
        dotnet,
        Some(&assembly),
        &output,
    )
    .map_err(|error| format!("C# {label} contract: {error}"))?;
    let mut command = dotnet_command_at(dotnet.as_os_str(), &sandbox);
    command
        .arg(&assembly)
        .arg(root.join("controls.json"))
        .arg(index.to_string())
        .arg(&output)
        .arg(&contract);
    let run = command
        .output()
        .map_err(|error| format!("C# {label} run: {error}"))?;
    recorder
        .record(&format!("csharp-{label}-run"), &command, &run)
        .map_err(|error| format!("C# {label} run record: {error}"))?;
    host_result(&output, model_calls(controls, index), &run)
        .and_then(|()| csharp_messages(controls, index, &output))
        .map_err(|error| format!("C# {label}: {error}"))
}

#[test]
fn failure_excerpts_keep_the_bounded_tail() {
    let long = vec![b'x'; FAILURE_EXCERPT + 10];
    let quoted = excerpt(&long);
    assert!(quoted.starts_with('…'));
    assert_eq!(quoted.len(), '…'.len_utf8() + FAILURE_EXCERPT);
    assert_eq!(excerpt(b"short"), "short");
}

#[test]
fn call_counts_follow_the_controls() {
    let controls = json!({"cases":[
        {"model":0,"calls":[{},{}]},
        {"model":1,"calls":[{}]},
        {"model":0,"calls":[{}]}
    ]});
    assert_eq!(total_calls(&controls), 4);
    assert_eq!(model_calls(&controls, 0), 3);
    assert_eq!(model_calls(&controls, 1), 1);
    assert_eq!(model_calls(&controls, 2), 0);
}
