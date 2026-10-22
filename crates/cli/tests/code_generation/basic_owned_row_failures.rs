//! Public one-segment Primary Group failure qualification (#808).
//! Preparation is compiler-free; the coordinator owns the serialized compiler lane.
use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::Write as _;

#[path = "basic_owned_row_failures/fixtures.rs"]
mod fixtures;
#[path = "basic_owned_row_failures/native.rs"]
mod native;

struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os("FERRULE_BASIC_ROW808_PREPARE_DIR")
                .ok_or("coordinator must supply a fresh absolute evidence directory")?,
        );
        if !path.is_absolute() || fs::symlink_metadata(&path).is_ok() {
            return Err("evidence directory must be fresh and absolute".into());
        }
        if !path.parent().is_some_and(Path::is_dir) {
            return Err("evidence parent must already exist".into());
        }
        fs::create_dir(&path)?;
        eprintln!("BASIC_ROW808_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().ok_or("evidence parent")?)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(())
    }
    fn debug(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(name, format!("{value:#?}\n").as_bytes())
    }
    fn artifacts(&self, directory: &str, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
        self.debug(&format!("{directory}-COMPLETE-ARTIFACTS.txt"), artifacts)?;
        for file in artifacts.files() {
            self.write(
                &format!("{directory}/{}", file.path.as_str()),
                &file.contents,
            )?;
        }
        Ok(())
    }
}

const SUCCESS_FILE: &str = "SUCCESSFUL-PREPARATION.json";

fn hashes(files: ArtifactFiles) -> Vec<(String, usize, String)> {
    files
        .into_iter()
        .map(|(path, bytes)| {
            let digest = format!("{:x}", Sha256::digest(&bytes));
            (path, bytes.len(), digest)
        })
        .collect()
}

fn source_bindings() -> TestResult<Vec<(String, usize, String)>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace source root")?;
    let mut files = Vec::new();
    // Bind the native/backend implementations and every local Rust runtime
    // dependency, not just manifests that can survive stale preparation.
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
        "crates/cli/tests/code_generation/basic_owned_row_failures",
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
        "crates/cli/tests/code_generation/basic_owned_row_failures.rs",
    ] {
        files.push((relative.into(), fs::read(root.join(relative))?));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(hashes(files))
}

fn preparation_binding(directory: &Path) -> TestResult<Json> {
    let files = artifact_files(directory)?
        .into_iter()
        .filter(|(path, _)| path != SUCCESS_FILE)
        .collect();
    Ok(json!({
        "schema_version":1,"result":"success","models":3,"cases":31,"native_calls":93,
        "selections":["All","Primary","audit"],
        "rust_runtime_path":Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime").canonicalize()?,
        "evidence_files":hashes(files),"source_files":source_bindings()?
    }))
}

fn verify_successful_preparation(directory: &Path) -> TestResult<Json> {
    let original: Json = serde_json::from_slice(&fs::read(directory.join(SUCCESS_FILE))?)?;
    if original != preparation_binding(directory)? {
        return Err("successful preparation is stale or its exact model/case/artifact/source binding changed".into());
    }
    Ok(original)
}

#[test]
#[ignore = "coordinator prepares native/public generated row-failure evidence before separately compiling fresh consumers"]
fn prepare_basic_owned_row808_three_models_ninety_three_calls() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let source_fence = source_bindings()?;
    let projects = fixtures::projects();
    let controls = fixtures::controls();
    // Complete independent inputs, expectations, schemas, graph, options and call
    // inventory are fixed and persisted before the first mapping invocation.
    evidence.write("controls.json", &serde_json::to_vec_pretty(&controls)?)?;
    evidence.debug("ALL3-COMPLETE-PUBLIC-PROJECTS.txt", &projects)?;
    evidence.write("LIMITS-AND-API-INVENTORY.json", &serde_json::to_vec_pretty(&json!({
        "models":3,"cases":31,"calls_per_language":93,
        "selections":["All","Primary","audit"],
        "native":["run_outputs_with_sources_and_context","run_selected_target_with_sources_and_context"],
        "rust":["execute_with_basic_owned_sources","execute_selected_with_basic_owned_sources"],
        "csharp":["ExecuteWithBasicOwnedSources","ExecuteSelectedWithBasicOwnedSources"],
        "context":{"mapping_file_path":"row808.mapping","main_mapping_file_path":"row808.mapping","current_datetime":null,"parameters":null,"loader":null,"cancellation":null},
        "limits":{"max_roots":8,"max_active_frames":8,"max_path_segments":32,"max_metadata_bytes":1048576,"max_walk_contexts":128},
        "specialized_provenance":false,"observer_gate":false
    }))?)?;
    for (index, project) in projects.iter().enumerate() {
        evidence.write(
            &format!("model{index}-project.json"),
            mapping::project_file::encode_pretty(project)?.as_bytes(),
        )?;
    }
    let native_checks = native::run(&projects, &controls, &evidence)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut preparation = Vec::new();
    let mut dependencies = String::new();
    for (index, project) in projects.iter().enumerate() {
        let options = codegen_rust::Options {
            package_name: format!("row808-model{index}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        evidence.debug(&format!("model{index}-RUST-OPTIONS.txt"), &options)?;
        let validation = engine::validate(project);
        evidence.debug(
            &format!("model{index}-COMPLETE-VALIDATION.txt"),
            &validation,
        )?;
        let lowered = codegen::lower_with_basic_owned_row_failures(project);
        evidence.debug(
            &format!("model{index}-COMPLETE-PUBLIC-LOWERING.txt"),
            &lowered,
        )?;
        let program = match lowered {
            Ok(program) => program,
            Err(error) => {
                for language in ["RUST", "CSHARP"] {
                    evidence.write(
                        &format!("model{index}-{language}-EMISSION-NOT-INVOKED.txt"),
                        b"Public lowering returned no Program. No direct Program substitution, emitter invocation, or generated compiler claim was made.\n",
                    )?;
                }
                preparation.push((index, false, format!("{error:#?}")));
                continue;
            }
        };
        let rust = codegen_rust::emit_with_basic_owned_row_failures(&program, &options);
        let csharp = codegen_csharp::emit_with_basic_owned_row_failures(&program);
        // Neither backend result is inspected before both complete originals exist.
        evidence.debug(&format!("model{index}-COMPLETE-RUST-EMISSION.txt"), &rust)?;
        evidence.debug(
            &format!("model{index}-COMPLETE-CSHARP-EMISSION.txt"),
            &csharp,
        )?;
        let mut okay = validation.is_empty();
        match rust {
            Ok(artifacts) => evidence.artifacts(&format!("rust-model{index}"), &artifacts)?,
            Err(_) => okay = false,
        }
        match csharp {
            Ok(artifacts) => {
                evidence.artifacts(&format!("csharp-model{index}"), &artifacts)?;
                evidence.write(&format!("csharp-host{index}/Host.csproj"), format!(
                    "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{index}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
                ).as_bytes())?;
                evidence.write(
                    &format!("csharp-host{index}/Program.cs"),
                    include_bytes!("basic_owned_row_failures/Host.cs.txt"),
                )?;
            }
            Err(_) => okay = false,
        }
        dependencies.push_str(&format!("model{index} = {{ package = \"row808-model{index}\", path = \"../rust-model{index}\" }}\n"));
        preparation.push((index, okay, String::new()));
    }
    evidence.write(
        "rust-host/src/main.rs",
        include_bytes!("basic_owned_row_failures/Host.rs.txt"),
    )?;
    evidence.write("rust-host/Cargo.toml", format!(
        "[package]\nname = \"row808-public-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[workspace]\n[dependencies]\ncodegen-runtime = {{ path = {} }}\nserde_json = \"=1.0.150\"\n{dependencies}",
        serde_json::to_string(&runtime.display().to_string())?
    ).as_bytes())?;
    evidence.debug("ALL3-COMPLETE-PREPARATION-OUTCOMES.txt", &preparation)?;
    evidence.debug("ALL93-COMPLETE-NATIVE-COMPARISONS.txt", &native_checks)?;
    if preparation.len() != 3
        || preparation.iter().any(|(_, okay, _)| !okay)
        || native_checks.len() != 93
        || native_checks.iter().any(|(_, _, matched)| !matched)
    {
        return Err("row808 native or public admission/emission differs; complete originals retained (no compiler was started)".into());
    }
    if source_fence != source_bindings()? {
        return Err(
            "runtime/backend/fixture sources changed during preparation; originals retained".into(),
        );
    }
    // This is the only publication of success: failed native comparisons or
    // partial/failed public emission never produce a runnable qualification.
    evidence.write(
        SUCCESS_FILE,
        &serde_json::to_vec_pretty(&preparation_binding(&evidence.0)?)?,
    )?;
    Ok(())
}

#[test]
#[ignore = "requires successful row808 preparation, unchanged runtime/backend sources, and the coordinator's serialized compiler lane"]
fn qualify_basic_owned_row808_prepared_generated_consumers() -> TestResult<()> {
    let directory = PathBuf::from(
        std::env::var_os("FERRULE_BASIC_ROW808_PREPARE_DIR")
            .ok_or("coordinator must supply the successful preparation directory")?,
    );
    if !directory.is_absolute() || !directory.is_dir() {
        return Err("prepared evidence directory must exist and be absolute".into());
    }
    let successful_preparation = verify_successful_preparation(&directory)?;
    if fs::read(directory.join("controls.json"))?
        != serde_json::to_vec_pretty(&fixtures::controls())?
    {
        return Err(
            "prepared controls differ from the complete independently frozen inventory".into(),
        );
    }
    for (index, project) in fixtures::projects().iter().enumerate() {
        if fs::read(directory.join(format!("model{index}-project.json")))?
            != mapping::project_file::encode_pretty(project)?.as_bytes()
        {
            return Err("prepared public Project differs from the frozen fixture".into());
        }
    }
    // Failed preparation is intentionally not converted into a generated-code
    // claim or a hand-authored Program. Its complete originals remain retained.
    for index in 0..3 {
        for file in [
            format!("rust-model{index}/Cargo.toml"),
            format!("csharp-model{index}/Ferrule.Generated.csproj"),
        ] {
            if !directory.join(file).is_file() {
                return Err(
                    "public admission/emission did not prepare all three paired models".into(),
                );
            }
        }
    }
    let mut originals = Vec::new();
    for index in 0..3 {
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
    let output = host_policy::recorded_output(&mut rust, &directory, "row808-rust-host")?;
    statuses.push(("rust".to_owned(), output.status.success()));
    for index in 0..3 {
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
            &format!("row808-csharp{index}-build"),
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
            &format!("row808-csharp{index}-host"),
        )?;
        statuses.push((format!("csharp{index}-host"), output.status.success()));
    }
    // Builds may add host locks and separate caches, never alter emitted code.
    for (path, original) in originals {
        assert_eq!(
            artifact_files(&path)?,
            original,
            "emitted library changed: {}",
            path.display()
        );
    }
    let mut result = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("ALL-GENERATED-EXECUTION-STATUSES.txt"))?;
    writeln!(result, "{statuses:#?}")?;
    result.flush()?;
    if successful_preparation["source_files"] != serde_json::to_value(source_bindings()?)? {
        return Err("runtime/backend/fixture sources changed during qualification; complete originals retained".into());
    }
    if statuses.len() != 7 || statuses.iter().any(|(_, okay)| !okay) {
        return Err("row808 generated consumer differs; complete build/command/input/output/cause originals retained".into());
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
