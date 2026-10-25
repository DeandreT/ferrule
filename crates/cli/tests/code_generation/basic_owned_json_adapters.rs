//! Independent, bounded public Basic JSON/text/UTF8 qualification (#520).
//! Preparation is compiler-free; generated consumers use the coordinator's lane.
use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest as _, Sha256};
use std::{fs, io::Write as _};

#[path = "basic_owned_json_adapters/fixtures.rs"]
mod fixtures;
#[path = "basic_owned_json_adapters/native.rs"]
mod native;

const DIRECTORY_VARIABLE: &str = "FERRULE_BASIC_JSON520_PREPARE_DIR";
const SUCCESS_FILE: &str = "SUCCESSFUL-PREPARATION.json";
struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os(DIRECTORY_VARIABLE)
                .ok_or("coordinator must supply a fresh absolute evidence directory")?,
        );
        if !path.is_absolute()
            || fs::symlink_metadata(&path).is_ok()
            || !path.parent().is_some_and(Path::is_dir)
        {
            return Err("fresh absolute evidence directory with existing parent required".into());
        }
        fs::create_dir(&path)?;
        host_policy::preflight_environment(&path)?;
        eprintln!("BASIC_JSON520_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write(&self, relative: &str, bytes: &[u8]) -> TestResult<()> {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().ok_or("evidence parent")?)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(())
    }
    fn debug(&self, path: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(path, format!("{value:#?}\n").as_bytes())
    }
    fn error(&self, path: &str, error: &(dyn std::error::Error + 'static)) -> TestResult<()> {
        let mut original = format!("Debug={error:#?}\nDisplay={error}\n");
        let mut source = error.source();
        while let Some(error) = source {
            original.push_str(&format!("CauseDebug={error:#?}\nCauseDisplay={error}\n"));
            source = error.source();
        }
        original.push_str("CAUSE_CHAIN_TERMINATED=None\n");
        self.write(path, original.as_bytes())
    }
    fn artifacts(&self, relative: &str, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
        let mut index = Vec::new();
        for file in artifacts.files() {
            self.write(
                &format!("{relative}/{}", file.path.as_str()),
                &file.contents,
            )?;
            index.push((
                file.path.as_str(),
                file.contents.len(),
                format!("{:x}", Sha256::digest(&file.contents)),
            ));
        }
        self.write(
            &format!("{relative}-COMPLETE-ARTIFACT-INDEX.json"),
            &serde_json::to_vec_pretty(&index)?,
        )
    }
}
fn hashes(files: ArtifactFiles) -> Vec<(String, usize, String)> {
    files
        .into_iter()
        .map(|(path, bytes)| (path, bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
        .collect()
}
fn source_bindings() -> TestResult<Vec<(String, usize, String)>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root")?;
    let mut files = Vec::new();
    for name in [
        "engine",
        "codegen",
        "codegen-rust",
        "codegen-csharp",
        "codegen-runtime",
        "codegen-schema",
        "functions",
        "format-csv",
        "format-edi",
        "format-json",
        "format-xml",
        "ir",
        "json-pattern",
        "mapping",
    ] {
        let relative = format!("crates/{name}");
        files.push((
            format!("{relative}/Cargo.toml"),
            fs::read(root.join(&relative).join("Cargo.toml"))?,
        ));
        for (path, bytes) in artifact_files(&root.join(&relative).join("src"))? {
            files.push((format!("{relative}/src/{path}"), bytes));
        }
    }
    for relative in [
        "runtime/csharp/Ferrule.Runtime",
        "crates/cli/tests/code_generation/basic_owned_json_adapters",
    ] {
        for (path, bytes) in artifact_files(&root.join(relative))? {
            files.push((format!("{relative}/{path}"), bytes));
        }
    }
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/cli/Cargo.toml",
        "crates/cli/tests/code_generation.rs",
        "crates/cli/tests/code_generation/host_policy.rs",
        "crates/cli/tests/code_generation/basic_owned_json_adapters.rs",
    ] {
        files.push((relative.into(), fs::read(root.join(relative))?));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(hashes(files))
}
fn binding(directory: &Path) -> TestResult<Json> {
    let files = artifact_files(directory)?
        .into_iter()
        .filter(|(path, _)| path != SUCCESS_FILE)
        .collect();
    Ok(
        json!({"version":1,"result":"success","models":fixtures::MODELS,"cases":fixtures::CASES,
        "portable_calls_per_language":fixtures::PORTABLE_CALLS,"csharp_only_calls":fixtures::CSHARP_ONLY_CALLS,
        "native_typed_calls":native::CALLS,"source_files":source_bindings()?,"evidence_files":hashes(files),
        "rust_runtime_path":Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime").canonicalize()?}),
    )
}
fn frozen_inventory() -> TestResult<Json> {
    let controls = fixtures::controls();
    if fixtures::projects().len() != fixtures::MODELS
        || controls["cases"].as_array().unwrap().len() != fixtures::CASES
        || fixtures::counts(&controls) != (fixtures::PORTABLE_CALLS, fixtures::CSHARP_ONLY_CALLS)
    {
        return Err("frozen fixture inventory counts changed".into());
    }
    for (model, bytes) in [18, 18, 31].into_iter().enumerate() {
        let output = &controls["cases"][model]["calls"][1]["expected"]["Ok"][0];
        let document = output[1].as_str().ok_or("frozen primary document")?;
        if document.len() != bytes || document.ends_with('\n') {
            return Err("exact compact oracle changed".into());
        }
    }
    Ok(controls)
}
#[test]
fn basic_owned_json520_frozen_inventory_is_complete() -> TestResult<()> {
    frozen_inventory().map(|_| ())
}

#[test]
#[ignore = "coordinator-owned compiler-free preparation; retains every failed original"]
fn prepare_basic_owned_json520_three_projects_and_public_callers() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let source_fence = source_bindings()?;
    let controls = frozen_inventory()?;
    let projects = fixtures::projects();
    // Publish complete independent recipes/graphs/options before any execution.
    evidence.write("controls.json", &serde_json::to_vec_pretty(&controls)?)?;
    evidence.write(
        "PROTOCOL.md",
        include_bytes!("basic_owned_json_adapters/PROTOCOL.md"),
    )?;
    evidence.debug("ALL3-COMPLETE-PUBLIC-PROJECTS.txt", &projects)?;
    for (index, project) in projects.iter().enumerate() {
        evidence.write(
            &format!("model{index}-project.json"),
            mapping::project_file::encode_pretty(project)?.as_bytes(),
        )?;
    }
    evidence.write("LIMITS-AND-API-INVENTORY.json", &serde_json::to_vec_pretty(&json!({
        "rust":["execute_json_outputs_with_basic_owned_sources","execute_json_bytes_outputs_with_basic_owned_sources",
            "execute_json_selected_target_with_basic_owned_sources","execute_json_bytes_selected_target_with_basic_owned_sources"],
        "csharp":["ExecuteJsonOutputsWithBasicOwnedSources","ExecuteJsonBytesOutputsWithBasicOwnedSources",
            "ExecuteJsonSelectedTargetWithBasicOwnedSources","ExecuteJsonBytesSelectedTargetWithBasicOwnedSources"],
        "emitter":"emit_with_basic_owned_sources","lowering":"lower",
        "context":{"mapping_file_path":"json520.mapping","main_mapping_file_path":"json520.mapping",
            "current_datetime":null,"parameters":null,"loader":null,"cancellation":null},
        "generous_limits":{"max_roots":8,"max_active_frames":8,"max_path_segments":32,"max_metadata_bytes":1048576,"max_walk_contexts":128},
        "new_workspace_numeric_expectations":false,"native_json_boundary_policies":false
    }))?)?;
    let native = native::run(&projects, &evidence)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut prepared = Vec::new();
    let mut dependencies = String::new();
    for (index, project) in projects.iter().enumerate() {
        let options = codegen_rust::Options {
            package_name: format!("json520-model{index}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        evidence.debug(&format!("model{index}-COMPLETE-RUST-OPTIONS.txt"), &options)?;
        let validation = engine::validate(project);
        evidence.debug(
            &format!("model{index}-COMPLETE-VALIDATION.txt"),
            &validation,
        )?;
        let lowered = codegen::lower(project);
        evidence.debug(
            &format!("model{index}-COMPLETE-PUBLIC-LOWERING.txt"),
            &lowered,
        )?;
        let program = match lowered {
            Ok(program) => program,
            Err(_) => {
                evidence.write(&format!("model{index}-EMISSION-NOT-INVOKED.txt"), b"Public lowering returned no Program. No fabricated Program or generated compiler claim.\n")?;
                prepared.push((index, false));
                continue;
            }
        };
        let rust = codegen_rust::emit_with_basic_owned_sources(&program, &options);
        let csharp = codegen_csharp::emit_with_basic_owned_sources(&program);
        // Persist both complete byte sets or original typed causes before
        // comparing either result. A partial paired preparation cannot succeed.
        match &rust {
            Ok(artifacts) => evidence.artifacts(&format!("rust-model{index}"), artifacts)?,
            Err(error) => {
                evidence.error(&format!("model{index}-COMPLETE-RUST-ERROR.txt"), error)?
            }
        }
        match &csharp {
            Ok(artifacts) => evidence.artifacts(&format!("csharp-model{index}"), artifacts)?,
            Err(error) => {
                evidence.error(&format!("model{index}-COMPLETE-CSHARP-ERROR.txt"), error)?
            }
        }
        let okay = validation.is_empty() && rust.is_ok() && csharp.is_ok();
        if csharp.is_ok() {
            evidence.write(&format!("csharp-host{index}/Host.csproj"), format!(
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{index}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n").as_bytes())?;
            evidence.write(
                &format!("csharp-host{index}/Program.cs"),
                include_bytes!("basic_owned_json_adapters/Host.cs.txt"),
            )?;
        }
        dependencies.push_str(&format!("model{index} = {{ package = \"json520-model{index}\", path = \"../rust-model{index}\" }}\n"));
        prepared.push((index, okay));
    }
    evidence.write(
        "rust-host/src/main.rs",
        include_bytes!("basic_owned_json_adapters/Host.rs.txt"),
    )?;
    evidence.write("rust-host/Cargo.toml", format!("[package]\nname = \"json520-public-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[workspace]\n[dependencies]\ncodegen-runtime = {{ path = {} }}\nserde_json = \"=1.0.150\"\n{dependencies}", serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;
    evidence.debug("ALL-COMPLETE-NATIVE-COMPARISONS.txt", &native)?;
    evidence.debug("ALL3-COMPLETE-PREPARATION-OUTCOMES.txt", &prepared)?;
    if prepared.len() != fixtures::MODELS
        || prepared.iter().any(|(_, okay)| !okay)
        || native.len() != native::CALLS
        || native.iter().any(|(_, _, okay)| !okay)
    {
        return Err("JSON520 native/public admission/emission differs; complete originals retained; no compiler started".into());
    }
    if source_fence != source_bindings()? {
        return Err("sources changed during preparation; originals retained".into());
    }
    evidence.write(
        SUCCESS_FILE,
        &serde_json::to_vec_pretty(&binding(&evidence.0)?)?,
    )?;
    Ok(())
}

#[test]
#[ignore = "successful unchanged preparation and coordinator's serialized compiler lane required"]
fn qualify_basic_owned_json520_prepared_generated_consumers() -> TestResult<()> {
    let directory = PathBuf::from(
        std::env::var_os(DIRECTORY_VARIABLE).ok_or("successful preparation directory required")?,
    );
    if !directory.is_absolute() || !directory.is_dir() {
        return Err("absolute existing preparation directory required".into());
    }
    let success: Json = serde_json::from_slice(&fs::read(directory.join(SUCCESS_FILE))?)?;
    if success != binding(&directory)? {
        return Err("stale or altered successful preparation".into());
    }
    if fs::read(directory.join("controls.json"))?
        != serde_json::to_vec_pretty(&frozen_inventory()?)?
    {
        return Err("prepared controls differ from frozen recipes".into());
    }
    let mut originals = Vec::new();
    for (index, project) in fixtures::projects().iter().enumerate() {
        if fs::read(directory.join(format!("model{index}-project.json")))?
            != mapping::project_file::encode_pretty(project)?.as_bytes()
        {
            return Err("prepared genuine Project differs".into());
        }
        for language in ["rust", "csharp"] {
            let path = directory.join(format!("{language}-model{index}"));
            originals.push((path.clone(), artifact_files(&path)?));
        }
    }
    let mut statuses = Vec::new();
    let mut rust = Command::new("cargo");
    rust.args(["run", "--quiet", "--jobs", "1", "--manifest-path"])
        .arg(directory.join("rust-host/Cargo.toml"))
        .arg("--")
        .arg(directory.join("controls.json"))
        .arg(directory.join("rust-actual"))
        .current_dir(&directory)
        .env("RUSTFLAGS", "-D warnings");
    let output = host_policy::recorded_output(&mut rust, &directory, "json520-rust-host")?;
    statuses.push(("rust".to_owned(), output.status.success()));
    for index in 0..fixtures::MODELS {
        let artifacts = directory.join(format!("dotnet-artifacts{index}"));
        let mut build = dotnet_command(&directory);
        build
            .args([
                "build",
                "--configuration",
                "Release",
                "--nologo",
                "--verbosity",
                "quiet",
                "--disable-build-servers",
                "-m:1",
                "-p:UseSharedCompilation=false",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .arg(directory.join(format!("csharp-host{index}/Host.csproj")))
            .current_dir(&directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let output = host_policy::recorded_output(
            &mut build,
            &directory,
            &format!("json520-csharp{index}-build"),
        )?;
        statuses.push((format!("csharp{index}-build"), output.status.success()));
        if !output.status.success() {
            continue;
        }
        let mut run = dotnet_command(&directory);
        run.arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(directory.join("controls.json"))
            .arg(index.to_string())
            .arg(directory.join(format!("csharp{index}-actual")))
            .current_dir(&directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let output = host_policy::recorded_output(
            &mut run,
            &directory,
            &format!("json520-csharp{index}-host"),
        )?;
        statuses.push((format!("csharp{index}-host"), output.status.success()));
    }
    let evidence = Evidence(directory.clone());
    evidence.debug("ALL-GENERATED-EXECUTION-STATUSES.txt", &statuses)?;
    for (path, original) in originals {
        if artifact_files(&path)? != original {
            return Err("emitted sources changed during qualification; originals retained".into());
        }
    }
    if success["source_files"] != serde_json::to_value(source_bindings()?)? {
        return Err(
            "runtime/backend/fixture sources changed during qualification; originals retained"
                .into(),
        );
    }
    if statuses.len() != 7 || statuses.iter().any(|(_, okay)| !okay) {
        return Err(
            "JSON520 generated consumers differ; complete commands/inputs/outcomes/causes retained"
                .into(),
        );
    }
    Ok(())
}

/// The harness Projects, for the owned capability preset record.
pub(super) fn capability_corpus() -> TestResult<super::owned_capability_presets::Corpus> {
    Ok(super::owned_capability_presets::projects(
        fixtures::projects()
            .into_iter()
            .enumerate()
            .map(|(index, project)| (format!("model-{index}"), project)),
    ))
}
