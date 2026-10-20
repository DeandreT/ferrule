//! #442 public-Project three-engine campaign. Preparation is compiler-free;
//! execution is a separate ignored, coordinator-serialized operation.
use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest as _, Sha256};
use std::{fs, io::Write as _};
#[path = "../../../../conformance/basic-owned-group-adjacent-by/cases.rs"]
mod cases;
#[path = "../../../../conformance/basic-owned-group-adjacent-by/closed.rs"]
mod closed;
#[path = "../../../../conformance/basic-owned-group-adjacent-by/closed_specialized.rs"]
mod closed_specialized;
#[path = "../../../../conformance/basic-owned-group-adjacent-by/projects.rs"]
mod projects;
// Reuse the single qualified #473 codec/native implementation and its original
// evidence writer. The borrowed helper paths are included in source_bindings.
use super::basic_owned_sequence_item_at::{codec, run_native};
const ENVIRONMENT: &str = "FERRULE_BASIC_ADJACENT442_PREPARE_DIR";
const SUCCESS: &str = "SUCCESSFUL-COMPILER-FREE-PREPARATION.json";
const LEGACY_CALLS: usize = 2;
const HISTORICAL_MANIFEST_SHA256: &str =
    "f28c83b23c1c1901e9221a91254c9e8a345369c20e8b514919c3e48b24db4dcf";
const HISTORICAL_MANIFEST: &[u8] = include_bytes!(
    "../../../../conformance/basic-owned-group-adjacent-by/legacy-baseline/manifest.json"
);
const HISTORICAL_CARGO: &[u8] = include_bytes!(
    "../../../../conformance/basic-owned-group-adjacent-by/legacy-baseline/Cargo.toml.historical"
);
const HISTORICAL_CARGO_TEMPLATE: &str = include_str!(
    "../../../../conformance/basic-owned-group-adjacent-by/legacy-baseline/Cargo.toml.template"
);
const HISTORICAL_BRIDGE: &[u8] = include_bytes!(
    "../../../../conformance/basic-owned-group-adjacent-by/legacy-baseline/provenance/GENERATOR-BRIDGE.md"
);
const MODELS: usize = 86;
const CASES: usize = 89;
const CORE_CALLS: usize = 263;
const CLOSED_PROJECTS: usize = 39;
const DIRECT_PROGRAMS: usize = 44;
struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
    native: bool,
}
struct Evidence(PathBuf);
impl Evidence {
    fn new() -> TestResult<Self> {
        let path = PathBuf::from(
            std::env::var_os(ENVIRONMENT).ok_or("fresh absolute preparation directory required")?,
        );
        if !path.is_absolute()
            || fs::symlink_metadata(&path).is_ok()
            || !path.parent().is_some_and(Path::is_dir)
        {
            return Err(
                "fresh absent absolute preparation root with existing parent required".into(),
            );
        }
        fs::create_dir(&path)?;
        host_policy::preflight_environment(&path)?;
        eprintln!("ADJACENT442_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write_io(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.0.join(name);
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| io::Error::other("evidence parent"))?,
        )?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        self.write_io(name, bytes)?;
        Ok(())
    }
    fn json(&self, name: &str, value: &impl serde::Serialize) -> TestResult<()> {
        self.write(name, &serde_json::to_vec_pretty(value)?)
    }
    fn debug(&self, name: &str, value: &impl std::fmt::Debug) -> TestResult<()> {
        self.write(name, format!("{value:#?}\n").as_bytes())
    }
    fn error(&self, name: &str, error: &(dyn std::error::Error + 'static)) -> TestResult<()> {
        let mut text = format!("Debug={error:#?}\nDisplay={error}\n");
        let mut cause = error.source();
        while let Some(error) = cause {
            text.push_str(&format!("CauseDebug={error:#?}\nCauseDisplay={error}\n"));
            cause = error.source();
        }
        text.push_str("CAUSE_CHAIN_TERMINATED=None\n");
        self.write(name, text.as_bytes())
    }
    fn artifacts(&self, name: &str, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
        for file in artifacts.files() {
            self.write(&format!("{name}/{}", file.path.as_str()), &file.contents)?;
        }
        self.json(
            &format!("{name}-COMPLETE-ARTIFACT-INDEX.json"),
            &artifact_manifest(artifacts),
        )
    }
}
fn root() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace")?
        .to_owned())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hashes(files: ArtifactFiles) -> Vec<legacy_artifact_policy::ManifestRow> {
    files
        .into_iter()
        .map(|(path, bytes)| (path, bytes.len(), digest(&bytes)))
        .collect()
}
fn artifact_manifest(artifacts: &codegen::ArtifactSet) -> Vec<legacy_artifact_policy::ManifestRow> {
    artifacts
        .files()
        .iter()
        .map(|f| legacy_artifact_policy::row(f.path.as_str(), &f.contents))
        .collect()
}
fn source_bindings() -> TestResult<Vec<legacy_artifact_policy::ManifestRow>> {
    let root = root()?;
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
        // Includes the complete tracked baseline and provenance.
        "conformance/basic-owned-group-adjacent-by",
        "crates/cli/tests",
    ] {
        for (path, bytes) in artifact_files(&root.join(relative))? {
            files.push((format!("{relative}/{path}"), bytes));
        }
    }
    for relative in ["Cargo.toml", "Cargo.lock", "crates/cli/Cargo.toml"] {
        files.push((relative.into(), fs::read(root.join(relative))?));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(hashes(files))
}
fn command_runtime_binding() -> TestResult<Json> {
    let search = std::env::var_os("PATH").ok_or("PATH required")?;
    let mut tools = Vec::new();
    // Binding is read-only: preparation does not invoke even --version.
    for name in ["cargo", "rustc", "dotnet"] {
        let filename = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        };
        let path = std::env::split_paths(&search)
            .map(|p| p.join(&filename))
            .find(|p| p.is_file())
            .ok_or_else(|| format!("missing command {name}"))?
            .canonicalize()?;
        let bytes = fs::read(&path)?;
        tools.push(json!({"name":name,"path":path,"bytes":bytes.len(),"sha256":digest(&bytes)}));
    }
    // Arch's cargo/rustc entries are rustup proxies. Bind the installed real
    // tool binaries and selector settings too, without executing a resolver or
    // adding an unreviewed TOML parser. Likewise dotnet is an SDK host.
    let mut delegated = Vec::new();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|p| p.join(".rustup")));
    if let Some(rustup) = rustup.filter(|p| p.is_dir()) {
        let settings = rustup.join("settings.toml");
        if settings.is_file() {
            delegated.push((settings.display().to_string(), fs::read(&settings)?));
        }
        let toolchains = rustup.join("toolchains");
        if toolchains.is_dir() {
            for entry in fs::read_dir(toolchains)? {
                let entry = entry?.path();
                if entry.is_dir() {
                    for name in ["cargo", "rustc", "rustdoc"] {
                        let path = entry.join("bin").join(if cfg!(windows) {
                            format!("{name}.exe")
                        } else {
                            name.into()
                        });
                        if path.is_file() {
                            delegated.push((path.display().to_string(), fs::read(path)?));
                        }
                    }
                }
            }
        }
    }
    let dotnet = tools
        .iter()
        .find(|tool| tool["name"] == "dotnet")
        .and_then(|tool| tool["path"].as_str())
        .map(PathBuf::from)
        .ok_or("bound dotnet host")?;
    if let Some(parent) = dotnet.parent() {
        let sdks = parent.join("sdk");
        if sdks.is_dir() {
            for entry in fs::read_dir(sdks)? {
                let entry = entry?.path();
                if entry.is_dir() {
                    for relative in ["MSBuild.dll", "dotnet.dll", "Roslyn/bincore/csc.dll"] {
                        let path = entry.join(relative);
                        if path.is_file() {
                            delegated.push((path.display().to_string(), fs::read(path)?));
                        }
                    }
                }
            }
        }
    }
    delegated.sort_by(|a, b| a.0.cmp(&b.0));
    let selectors = [
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_HOME",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "DOTNET_ROOT",
        "DOTNET_ROLL_FORWARD",
        "DOTNET_MSBUILD_SDK_RESOLVER_SDKS_DIR",
    ]
    .into_iter()
    .map(|name| {
        (
            name,
            std::env::var_os(name).map(|value| value.to_string_lossy().into_owned()),
        )
    })
    .collect::<BTreeMap<_, _>>();
    reject_compiler_overrides(&selectors)?;
    let native = std::env::current_exe()?.canonicalize()?;
    let bytes = fs::read(&native)?;
    Ok(
        json!({"tools":tools,"delegated_tool_binaries_and_settings":hashes(delegated),"selectors":selectors,"native_test_binary":{"path":native,"bytes":bytes.len(),"sha256":digest(&bytes)},"path":search.to_string_lossy(),
        "rust_command":"cargo run --quiet --jobs 1 --manifest-path rust-host/Cargo.toml -- controls.json rust-actual",
        "csharp_build":"dotnet build Release --disable-build-servers -m:1 -p:UseSharedCompilation=false --artifacts-path <model-artifacts> <Host.csproj>",
        "csharp_run":"dotnet <fresh model Host.dll> <frozen controls> <model ordinal> <fresh evidence>",
        "required_owned_limits":{"max_roots":16,"max_active_frames":32,"max_path_segments":64,"max_metadata_bytes":16777216,"max_walk_contexts":512}}),
    )
}
fn reject_compiler_overrides(selectors: &BTreeMap<&str, Option<String>>) -> TestResult<()> {
    for name in ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        if selectors.get(name).is_some_and(Option::is_some) {
            return Err(format!(
                "{name} must be unset: this campaign binds the default compiler route"
            )
            .into());
        }
    }
    Ok(())
}
fn closed_projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    closed::projects()
        .into_iter()
        .chain(closed_specialized::projects())
        .collect()
}
fn closed_programs(
    base: &codegen::Program,
) -> Vec<(
    &'static str,
    codegen::Program,
    codegen::ProgramValidationError,
)> {
    closed::programs(base)
        .into_iter()
        .chain(closed_specialized::programs(base))
        .collect()
}
fn corpus() -> TestResult<(Json, Vec<Project>, Vec<String>)> {
    let cases = cases::cases();
    let mut models = Vec::<String>::new();
    let mut projects = Vec::new();
    let mut recipes = Vec::new();
    for case in &cases {
        let model = match models.iter().position(|p| p == case.profile) {
            Some(i) => i,
            None => {
                models.push(case.profile.into());
                projects.push(projects::project(case.profile));
                models.len() - 1
            }
        };
        recipes.push(json!({"id":case.id,"profile":case.profile,"model":model,"native":case.native,"source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,value)|json!([name,codec::encode(value)])).collect::<Vec<_>>(),"calls":case.calls}));
    }
    let calls = cases.iter().map(|c| c.calls.len()).sum::<usize>();
    let ids = cases
        .iter()
        .map(|c| c.id)
        .collect::<std::collections::BTreeSet<_>>();
    if closed_projects().len() != CLOSED_PROJECTS
        || models.len() != MODELS
        || projects::PROFILES.len() != MODELS
        || cases.len() != CASES
        || calls != CORE_CALLS
        || ids.len() != cases.len()
        || models
            .iter()
            .any(|p| !projects::PROFILES.contains(&p.as_str()))
    {
        return Err(format!(
            "literal inventory changed: cases={} calls={calls} models={}",
            cases.len(),
            models.len()
        )
        .into());
    }
    Ok((
        json!({"version":1,"cases":recipes,"models":models,"case_count":cases.len(),"core_calls_per_engine":calls,"legacy_calls_per_engine":LEGACY_CALLS}),
        projects,
        models,
    ))
}
fn binding(directory: &Path) -> TestResult<Json> {
    let files = artifact_files(directory)?
        .into_iter()
        .filter(|(path, _)| path != SUCCESS)
        .collect();
    let (controls, _, _) = corpus()?;
    Ok(
        json!({"version":1,"result":"success","cases":controls["case_count"],"core_calls_per_engine":controls["core_calls_per_engine"],"legacy_calls_per_engine":LEGACY_CALLS,
        "models":MODELS,"closed_projects":CLOSED_PROJECTS,"direct_programs":DIRECT_PROGRAMS,"sources":source_bindings()?,"evidence":hashes(files),"commands":command_runtime_binding()?,
        "runtime":Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime").canonicalize()?}),
    )
}
fn verify_original_evidence(directory: &Path, rows: &Json) -> TestResult<()> {
    let rows: Vec<legacy_artifact_policy::ManifestRow> = serde_json::from_value(rows.clone())?;
    for (relative, len, sha) in rows {
        let bytes = fs::read(directory.join(&relative))?;
        if bytes.len() != len || digest(&bytes) != sha {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("prepared original changed: {relative}"),
            )
            .into());
        }
    }
    Ok(())
}
fn expected_catalogues(profile: &str) -> Json {
    let names = [
        "unusedStatic",
        if profile.contains("Alias") {
            "Alias"
        } else {
            "Rows"
        },
        projects::EXACT,
        "unavailableDynamic",
    ];
    let catalogue = |required: bool| {
        json!(names.iter().enumerate().map(|(index,name)|json!({"declaration_index":index,"name":name,"availability":if index==3 {"ReservedUnavailable"} else if index==1 && required {"RequiredLegacy"} else {"Optional"}})).collect::<Vec<_>>())
    };
    let global = profile == "C03";
    let primary = global
        || ["A22-key", "A22-post", "A23", "C02"].contains(&profile)
        || profile.contains("Alias");
    json!({"all_outputs":catalogue(primary),"primary":catalogue(primary),"named_targets":[{"target_index":0,"name":projects::AUDIT,"catalogue":catalogue(global)},{"target_index":1,"name":projects::UNICODE_TARGET,"catalogue":catalogue(global)}]})
}
fn actual_catalogues(plan: &codegen::NamedInputAdmissionPlan<'_>) -> Json {
    fn catalogue(value: &codegen::NamedInputCatalogue<'_>) -> Json {
        json!(value.entries.iter().map(|e|json!({"declaration_index":e.declaration_index,"name":e.name,"availability":format!("{:?}",e.availability)})).collect::<Vec<_>>())
    }
    json!({"all_outputs":catalogue(&plan.all_outputs),"primary":catalogue(&plan.primary),"named_targets":plan.named_targets.iter().map(|t|json!({"target_index":t.target_index,"name":t.name,"catalogue":catalogue(&t.catalogue)})).collect::<Vec<_>>()})
}
fn host_project(model: &str) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
fn rust_host(dispatch: &str, calls: usize) -> String {
    include_str!("basic_owned_sequence_item_at/Host.rs.txt").replace("itemat473","adjacent442").replace("@DISPATCH@",dispatch).replace("@CORE_CALLS@",&calls.to_string())
        .replace("let selection=call[\"selection\"].as_str().unwrap();let prefix=", "let limits=if call[\"invalid_limits\"]==true {OwnedSourceLimits {max_roots:0,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0}} else {limits};\n            let selection=call[\"selection\"].as_str().unwrap();let prefix=")
}
fn csharp_host(calls: usize) -> String {
    include_str!("basic_owned_sequence_item_at/Host.cs.txt").replace("itemat473","adjacent442").replace("@MODEL_CALLS@",&calls.to_string())
        .replace("var limits = new FerruleOwnedSourceLimits", "var defaultLimits = new FerruleOwnedSourceLimits")
        .replace("var call = callNode!;", "var call = callNode!; var limits = call[\"invalid_limits\"]?.GetValue<bool>() == true ? new FerruleOwnedSourceLimits(0, 0, 0, 0, 0) : defaultLimits;")
        .replace("limits = new { roots = 16, frames = 32, path = 64, bytes = 16777216, walk = 512 }", "limits = new { roots = (ulong)limits.MaxRoots, frames = (ulong)limits.MaxActiveFrames, path = (ulong)limits.MaxPathSegments, bytes = (ulong)limits.MaxMetadataBytes, walk = (ulong)limits.MaxWalkContexts }")
}

#[test]
fn basic_adjacent442_compiler_override_routes_are_rejected() -> TestResult<()> {
    let mut selectors = BTreeMap::from([
        ("RUSTC", None),
        ("RUSTC_WRAPPER", None),
        ("RUSTC_WORKSPACE_WRAPPER", None),
    ]);
    reject_compiler_overrides(&selectors)?;
    for name in ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        for value in ["", "rustc", "/external/compiler-or-wrapper"] {
            selectors.insert(name, Some(value.to_owned()));
            assert!(
                reject_compiler_overrides(&selectors).is_err(),
                "{name}={value}"
            );
        }
        selectors.insert(name, None);
    }
    reject_compiler_overrides(&selectors)
}

#[test]
fn basic_adjacent442_authored_inventory_lossless_tags_and_catalogues() -> TestResult<()> {
    let (controls, models, profiles) = corpus()?;
    assert_eq!(models.len(), MODELS);
    const { assert!(MODELS > 80) };
    for case in controls["cases"].as_array().unwrap() {
        assert_eq!(
            codec::encode(&codec::decode(&case["source"])),
            case["source"]
        );
        for input in case["inputs"].as_array().unwrap() {
            assert_eq!(codec::encode(&codec::decode(&input[1])), input[1]);
        }
        for call in case["calls"].as_array().unwrap() {
            if let Some(outputs) = call["expected"].get("Ok") {
                for output in outputs.as_array().unwrap() {
                    assert_eq!(codec::encode(&codec::decode(&output[1])), output[1]);
                }
            }
        }
    }
    for profile in profiles {
        let plan = expected_catalogues(&profile);
        assert_eq!(plan["primary"].as_array().unwrap().len(), 4);
        assert_eq!(plan["primary"][3]["availability"], "ReservedUnavailable");
    }
    let tagged = controls["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "T01")
        .unwrap();
    let encoded = serde_json::to_string(tagged)?;
    for bits in ["7ff8000000000042", "8000000000000000", "3ff0000000000000"] {
        assert!(encoded.contains(bits));
    }
    assert_ne!(
        rust_host("dispatch", 1),
        include_str!("basic_owned_sequence_item_at/Host.rs.txt")
    );
    assert!(rust_host("dispatch", 1).contains("call[\"invalid_limits\"]"));
    assert!(csharp_host(1).contains("var limits = call[\"invalid_limits\"]"));
    assert!(csharp_host(1).contains("roots = (ulong)limits.MaxRoots"));
    Ok(())
}
#[test]
fn basic_adjacent442_original_fence_and_raw_failure_continuation() -> TestResult<()> {
    let temp = TempDir::new("adjacent442-fence")?;
    let evidence = Evidence(temp.0.clone());
    evidence.write("controls.json", b"authored")?;
    let original = serde_json::to_value(hashes(artifact_files(&temp.0)?))?;
    evidence.write("new-output", b"allowed")?;
    verify_original_evidence(&temp.0, &original)?;
    fs::write(temp.0.join("controls.json"), b"mutated")?;
    assert_eq!(
        verify_original_evidence(&temp.0, &original)
            .unwrap_err()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
    let group: codegen_runtime::InstanceGroup = vec![(
        "bits".into(),
        Instance::Scalar(Value::Float(f64::from_bits(0x7ff8000000000042))),
    )]
    .into();
    let annotated = Instance::Group(group.with_xml_type_origin(
        ir::XmlTypeOrigin::ExplicitPadded {
            literal: " p:T ",
            resolved_identity: "{urn:442}T",
        },
    )?);
    let mut matches = Vec::new();
    for (i, value) in [
        Instance::MappedSequence(vec![annotated.clone()]),
        annotated,
        Instance::Scalar(Value::Int(7)),
    ]
    .into_iter()
    .enumerate()
    {
        let product: std::thread::Result<Result<Instance, codegen_runtime::RuntimeError>> =
            Ok(Ok(value));
        let observed = codec::retain_outcome(
            product,
            |suffix, bytes| evidence.write_io(&format!("{i}-{suffix}"), bytes),
            codec::raw_tree,
            |value| {
                let value = codec::encode(value.as_ref().unwrap());
                assert!(i != 1, "observer sentinel");
                json!({"matched":value==json!(["Int",7])})
            },
        )?;
        matches.push(observed.is_some_and(|v| v["matched"] == true));
    }
    assert_eq!(matches, [false, false, true]);
    let retained = fs::read_to_string(temp.0.join("0-COMPLETE-RAW-PRODUCT.json"))?;
    for required in [
        "MappedSequence",
        "7ff8000000000042",
        "ExplicitPadded",
        " p:T ",
        "{urn:442}T",
    ] {
        assert!(retained.contains(required));
    }
    assert!(temp.0.join("1-COMPLETE-OBSERVATION-FAILURE.txt").is_file());
    Ok(())
}

#[test]
fn basic_adjacent442_closed_projects_and_direct_programs_keep_literal_diagnostics() -> TestResult<()>
{
    let mut failures = Vec::new();
    for (id, project, expected) in closed_projects() {
        let before = mapping::project_file::encode_pretty(&project)?;
        let actual = codegen::lower_with_basic_owned_group_adjacent_by(&project);
        if !matches!(&actual,Err(error) if error.diagnostics()==expected) {
            failures.push(format!(
                "Project {id}: expected={expected:#?}; actual={actual:#?}"
            ));
        }
        if mapping::project_file::encode_pretty(&project)? != before {
            failures.push(format!("Project {id}: refusing a Project cannot mutate it"));
        }
    }
    let base = codegen::lower_with_basic_owned_group_adjacent_by(&projects::base())?;
    let direct = closed_programs(&base);
    assert_eq!(direct.len(), DIRECT_PROGRAMS);
    for (id, program, expected) in direct {
        let before = program.clone();
        let validation = codegen::validate_program_with_basic_owned_group_adjacent_by(&program);
        let emission = codegen::validate_basic_owned_group_adjacent_by_emission(&program);
        if validation != Err(expected.clone()) || emission != Err(expected.clone()) {
            failures.push(format!(
                "Program {id}: expected={expected:#?}; validation={validation:#?}; emission={emission:#?}"
            ));
        }
        if program != before {
            failures.push(format!("Program {id}: refusing a Program cannot mutate it"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    Ok(())
}

fn historical_manifest_is_original(bytes: &[u8]) -> bool {
    digest(bytes) == HISTORICAL_MANIFEST_SHA256
}
fn historical_inventory() -> TestResult<(
    Json,
    Vec<legacy_artifact_policy::ManifestRow>,
    Vec<legacy_artifact_policy::ManifestRow>,
)> {
    if !historical_manifest_is_original(HISTORICAL_MANIFEST) {
        return Err("historical ordinary-fixture manifest changed".into());
    }
    let manifest: Json = serde_json::from_slice(HISTORICAL_MANIFEST)?;
    let provenance = &manifest["provenance"];
    if manifest["version"] != 1
        || provenance["source_commit"] != "a70e111282638db2f63bc6cf208c934bf1663a2f"
        || provenance["lowering_api"] != "codegen::lower"
        || provenance["rust_api"] != "codegen_rust::emit"
        || provenance["csharp_api"] != "codegen_csharp::emit"
        || provenance["rust_package_name"] != "ferrule-generated-mapping"
        || provenance["rust_runtime_dependency"] != "Path"
        || provenance["bridge"]["reviewed_base_commit"]
            != "7769fce8009bc17be091980cb7de12f11f7722c4"
        || provenance["bridge"]["document_sha256"] != digest(HISTORICAL_BRIDGE)
    {
        return Err("legacy historical baseline provenance/options differ".into());
    }
    for (field, bytes) in [
        (
            "project_sha256",
            include_bytes!(
                "../../../../conformance/basic-owned-group-adjacent-by/legacy-project.json"
            )
            .as_slice(),
        ),
        (
            "input_sha256",
            include_bytes!(
                "../../../../conformance/basic-owned-group-adjacent-by/legacy-input.json"
            )
            .as_slice(),
        ),
        (
            "expected_output_sha256",
            include_bytes!(
                "../../../../conformance/basic-owned-group-adjacent-by/legacy-expected.json"
            )
            .as_slice(),
        ),
    ] {
        if provenance[field] != digest(bytes) {
            return Err("legacy immutable fixture differs from its original".into());
        }
    }
    let rust: Vec<legacy_artifact_policy::ManifestRow> =
        serde_json::from_value(manifest["rust"].clone())?;
    let cs: Vec<legacy_artifact_policy::ManifestRow> =
        serde_json::from_value(manifest["csharp"].clone())?;
    if rust.len() != 2
        || cs.len() != 78
        || rust[0] != legacy_artifact_policy::row("Cargo.toml", HISTORICAL_CARGO)
        || rust[1].0 != "src/lib.rs"
    {
        return Err("historical 2 Rust/78 C# pathset changed".into());
    }
    for rows in [&rust, &cs] {
        if rows.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err("historical artifact paths must be ordered and unique".into());
        }
        for (path, length, sha) in rows {
            codegen::ArtifactPath::new(path.as_str())?;
            if *length == 0 || sha.len() != 64 || !sha.as_bytes().iter().all(u8::is_ascii_hexdigit)
            {
                return Err("historical artifact length/digest is invalid".into());
            }
        }
    }
    Ok((manifest, rust, cs))
}
fn historical_cargo_manifest(runtime: &str) -> TestResult<Vec<u8>> {
    if HISTORICAL_CARGO_TEMPLATE
        .matches("@RUNTIME_PATH_JSON@")
        .count()
        != 1
        || !runtime.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err("one explicit printable runtime-path operand required".into());
    }
    Ok(HISTORICAL_CARGO_TEMPLATE
        .replace("@RUNTIME_PATH_JSON@", &serde_json::to_string(runtime)?)
        .into_bytes())
}
#[test]
fn basic_adjacent442_historical_manifest_and_template_are_pinned() -> TestResult<()> {
    assert!(historical_manifest_is_original(HISTORICAL_MANIFEST));
    let mut changed_manifest = HISTORICAL_MANIFEST.to_vec();
    changed_manifest.push(b'\n');
    assert!(!historical_manifest_is_original(&changed_manifest));
    let (manifest, _, _) = historical_inventory()?;
    let path = manifest["provenance"]["historical_runtime_path"]
        .as_str()
        .unwrap();
    assert_eq!(
        historical_cargo_manifest(path)?.as_slice(),
        HISTORICAL_CARGO
    );
    Ok(())
}
fn historical_artifacts(
    evidence: &Evidence,
    runtime: &Path,
) -> TestResult<(
    Vec<legacy_artifact_policy::ManifestRow>,
    Vec<legacy_artifact_policy::ManifestRow>,
    Vec<u8>,
)> {
    let (manifest, mut rust, cs) = historical_inventory()?;
    let historical_path = manifest["provenance"]["historical_runtime_path"]
        .as_str()
        .ok_or("historical runtime-path operand required")?;
    if historical_cargo_manifest(historical_path)?.as_slice() != HISTORICAL_CARGO {
        return Err("complete historical Cargo template differs".into());
    }
    evidence.write("legacy-baseline/manifest.json", HISTORICAL_MANIFEST)?;
    evidence.write(
        "legacy-baseline/provenance/GENERATOR-BRIDGE.md",
        HISTORICAL_BRIDGE,
    )?;
    evidence.write("legacy-baseline/Cargo.toml.historical", HISTORICAL_CARGO)?;
    evidence.write(
        "legacy-baseline/Cargo.toml.template",
        HISTORICAL_CARGO_TEMPLATE.as_bytes(),
    )?;
    // Only independently expected Cargo bytes instantiate the explicit operand.
    // No emitted actual or historical row is normalized.
    let cargo = historical_cargo_manifest(&runtime.display().to_string())?;
    rust[0] = legacy_artifact_policy::row("Cargo.toml", &cargo);
    evidence.write("legacy-baseline/Cargo.toml.expected", &cargo)?;
    evidence.json(
        "legacy-baseline/COMPLETE-EXPECTED-ARTIFACTS.json",
        &json!({"rust":rust,"csharp":cs}),
    )?;
    Ok((rust, cs, cargo))
}
fn freeze_runtime(evidence: &Evidence) -> TestResult<legacy_artifact_policy::SourceSnapshot> {
    // Runtime expectations are captured from the source tree before emission.
    let snapshot = legacy_artifact_policy::snapshot(&root()?)?;
    evidence.json(
        "legacy-runtime-sources.json",
        &snapshot
            .iter()
            .map(|(path, bytes)| legacy_artifact_policy::row(path, bytes))
            .collect::<Vec<_>>(),
    )?;
    Ok(snapshot)
}

#[test]
fn reviewed_json_runtime_additions_keep_the_frozen_legacy_artifacts() -> TestResult<()> {
    let temp = TempDir::new("reviewed-json-runtime-additions")?;
    let evidence = Evidence(temp.0.clone());
    let (_, _, historical) = historical_inventory()?;
    let sources = freeze_runtime(&evidence)?;
    let project = mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-adjacent-by/legacy-project.json"
    ))?;
    let program = codegen::lower(&project)?;
    let artifacts = codegen_csharp::emit(&program)?;
    evidence.artifacts("actual", &artifacts)?;
    let actual = artifact_manifest(&artifacts);
    let comparison = legacy_artifact_policy::compare(&historical, &actual, &sources);
    assert!(
        comparison.accepts(legacy_artifact_policy::Policy::CurrentRuntimeSources),
        "{comparison:?}"
    );
    assert!(!comparison.accepts(legacy_artifact_policy::Policy::ExactHistorical));
    assert!(legacy_artifact_policy::snapshot_unchanged(
        &root()?,
        &sources
    )?);
    assert!(historical_manifest_is_original(HISTORICAL_MANIFEST));
    Ok(())
}

fn prepare_legacy(
    evidence: &Evidence,
    runtime: &Path,
    snapshot: &legacy_artifact_policy::SourceSnapshot,
    baseline: &(
        Vec<legacy_artifact_policy::ManifestRow>,
        Vec<legacy_artifact_policy::ManifestRow>,
        Vec<u8>,
    ),
) -> TestResult<bool> {
    let original =
        include_str!("../../../../conformance/basic-owned-group-adjacent-by/legacy-project.json");
    evidence.write("legacy-original-project.json", original.as_bytes())?;
    let project = mapping::project_file::decode_str(original)?;
    let wire = mapping::project_file::encode_pretty(&project)?;
    evidence.write("legacy-roundtrip-project.json", wire.as_bytes())?;
    let roundtrip = mapping::project_file::decode_str(&wire)?;
    let wire_same = serde_json::to_value(&project)? == serde_json::to_value(&roundtrip)?
        && !wire.contains("owned_source");
    let lowered = codegen::lower(&project);
    evidence.debug("legacy-COMPLETE-LOWERING.txt", &lowered)?;
    let program = match lowered {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    let options = codegen_rust::Options {
        package_name: "ferrule-generated-mapping".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    evidence.debug("legacy-COMPLETE-RUST-OPTIONS.txt", &options)?;
    // Same-profile ordinary APIs are the historical conservation comparison.
    let rust = codegen_rust::emit(&program, &options);
    let cs = codegen_csharp::emit(&program);
    let new_rust = codegen_rust::emit_with_basic_owned_group_adjacent_by(&program, &options);
    let new_cs = codegen_csharp::emit_with_basic_owned_group_adjacent_by(&program);
    for (name, result) in [
        ("rust-legacy", &rust),
        ("rust-legacy-adjacent-profile", &new_rust),
    ] {
        match result {
            Ok(a) => evidence.artifacts(name, a)?,
            Err(e) => evidence.error(&format!("{name}-COMPLETE-ERROR.txt"), e)?,
        }
    }
    for (name, result) in [
        ("csharp-legacy", &cs),
        ("csharp-legacy-adjacent-profile", &new_cs),
    ] {
        match result {
            Ok(a) => evidence.artifacts(name, a)?,
            Err(e) => evidence.error(&format!("{name}-COMPLETE-ERROR.txt"), e)?,
        }
    }
    let rust_rows = rust.as_ref().ok().map(artifact_manifest);
    let cs_rows = cs.as_ref().ok().map(artifact_manifest);
    let comparison = cs_rows
        .as_ref()
        .map(|actual| legacy_artifact_policy::compare(&baseline.1, actual, snapshot));
    let unchanged = legacy_artifact_policy::snapshot_unchanged(&root()?, snapshot)?;
    let cs_accepted = unchanged
        && comparison
            .as_ref()
            .is_some_and(|c| c.accepts(legacy_artifact_policy::Policy::CurrentRuntimeSources));
    let rust_exact = rust_rows.as_ref() == Some(&baseline.0)
        && rust.as_ref().is_ok_and(|a| {
            a.files()
                .iter()
                .any(|f| f.path.as_str() == "Cargo.toml" && f.contents == baseline.2)
        });
    evidence.json("legacy-COMPLETE-ARTIFACT-COMPARISONS.json",&json!({"policy":"CurrentRuntimeSources","wire_roundtrip":wire_same,"rust_exact":rust_exact,
        "rust_actual":rust_rows,"csharp_actual":cs_rows,"csharp_accepted":cs_accepted,"runtime_sources_unchanged":unchanged,
        "paths_exact":comparison.as_ref().is_some_and(|c|c.paths_exact),"stable_files_exact":comparison.as_ref().is_some_and(|c|c.stable_files_exact),"runtime_sources_exact":comparison.as_ref().is_some_and(|c|c.runtime_sources_exact),
        "historical_exact":comparison.as_ref().is_some_and(|c|c.accepts(legacy_artifact_policy::Policy::ExactHistorical))}))?;
    for (name, bytes) in [
        (
            "legacy-input.json",
            include_bytes!(
                "../../../../conformance/basic-owned-group-adjacent-by/legacy-input.json"
            )
            .as_slice(),
        ),
        (
            "legacy-expected.json",
            include_bytes!(
                "../../../../conformance/basic-owned-group-adjacent-by/legacy-expected.json"
            )
            .as_slice(),
        ),
    ] {
        evidence.write(name, bytes)?;
        evidence.write(&format!("rust-host/src/{name}"), bytes)?;
    }
    evidence.write(
        "rust-host/src/legacy_control.rs",
        include_bytes!("basic_owned_sequence_item_at/legacy_control.rs.txt"),
    )?;
    evidence.write(
        "csharp-legacy-host/Host.csproj",
        host_project("legacy").as_bytes(),
    )?;
    evidence.write(
        "csharp-legacy-host/Program.cs",
        include_bytes!("basic_owned_sequence_item_at/LegacyHost.cs.txt"),
    )?;
    Ok(wire_same && rust_exact && cs_accepted && new_rust.is_ok() && new_cs.is_ok())
}
fn old_family_controls(
    project: &Project,
    program: &codegen::Program,
    evidence: &Evidence,
    runtime: &Path,
) -> TestResult<bool> {
    let expected = closed::grouping_error();
    let expected_diagnostic = match &expected {
        codegen::ProgramValidationError::OwnedSource { diagnostic } => {
            codegen::Diagnostic::OwnedSource {
                diagnostic: diagnostic.clone(),
            }
        }
        _ => unreachable!(),
    };
    evidence.debug(
        "old-families-COMPLETE-AUTHORED-EXPECTED.txt",
        &(&expected, &expected_diagnostic),
    )?;
    let lowers = closed::old_lower(project);
    evidence.debug("old-families-COMPLETE-LOWERING.txt", &lowers)?;
    let before = program.clone();
    let validators = [
        codegen::validate_program(program),
        codegen::validate_basic_owned_emission(program),
        codegen::validate_program_with_basic_owned_filters(program),
        codegen::validate_basic_owned_filter_emission(program),
        codegen::validate_program_with_basic_owned_sort(program),
        codegen::validate_basic_owned_sort_emission(program),
        codegen::validate_program_with_basic_owned_windows(program),
        codegen::validate_basic_owned_window_emission(program),
        codegen::validate_program_with_basic_owned_row_failures(program),
        codegen::validate_basic_owned_row_failure_emission(program),
        codegen::validate_program_with_basic_owned_group_by(program),
        codegen::validate_basic_owned_group_by_emission(program),
        codegen::validate_program_with_basic_owned_sequence_item_at(program),
        codegen::validate_basic_owned_sequence_item_at_emission(program),
    ];
    let plans = [
        codegen::legacy_named_input_admission_plan(program),
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(program),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sort(program),
        codegen::legacy_named_input_admission_plan_with_basic_owned_windows(program),
        codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(program),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(program),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(program),
    ];
    let options = codegen_rust::Options {
        package_name: "adjacent442-old-gate".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    let rust = [
        codegen_rust::emit(program, &options),
        codegen_rust::emit_with_basic_owned_sources(program, &options),
        codegen_rust::emit_with_basic_owned_filters(program, &options),
        codegen_rust::emit_with_basic_owned_sort(program, &options),
        codegen_rust::emit_with_basic_owned_windows(program, &options),
        codegen_rust::emit_with_basic_owned_row_failures(program, &options),
        codegen_rust::emit_with_basic_owned_group_by(program, &options),
        codegen_rust::emit_with_basic_owned_sequence_item_at(program, &options),
    ];
    let cs = [
        codegen_csharp::emit(program),
        codegen_csharp::emit_with_basic_owned_sources(program),
        codegen_csharp::emit_with_basic_owned_filters(program),
        codegen_csharp::emit_with_basic_owned_sort(program),
        codegen_csharp::emit_with_basic_owned_windows(program),
        codegen_csharp::emit_with_basic_owned_row_failures(program),
        codegen_csharp::emit_with_basic_owned_group_by(program),
        codegen_csharp::emit_with_basic_owned_sequence_item_at(program),
    ];
    evidence.debug("old-families-COMPLETE-COMMON.txt", &(&validators, &plans))?;
    evidence.debug("old-families-COMPLETE-EMITTERS.txt", &(&rust, &cs))?;
    Ok(lowers
        .iter()
        .all(|r| matches!(r,Err(e) if e.diagnostics()==std::slice::from_ref(&expected_diagnostic)))
        && validators
            .iter()
            .all(|r| matches!(r,Err(e) if *e==expected))
        && plans.iter().all(|r| matches!(r,Err(e) if *e==expected))
        && rust
            .iter()
            .all(|r| matches!(r,Err(codegen_rust::EmitError::InvalidProgram(e)) if *e==expected))
        && cs.iter().all(
            |r| matches!(r,Err(codegen_csharp::EmitError::ProgramValidation(e)) if *e==expected),
        )
        && *program == before)
}

fn old_by_item_at_control(evidence: &Evidence, runtime: &Path) -> TestResult<bool> {
    let mut project = projects::project("I02");
    project.root.children[0].group_adjacent_by = None;
    project.root.children[0].group_by = Some(1);
    let expected = codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 70,
                scope: Some(closed::route()),
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::GeneratedItem,
            origin: codegen::OwnedSourceCapabilityOrigin::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 70,
            },
        }),
    };
    evidence.debug(
        "old-By-ItemAt-COMPLETE-AUTHORED.txt",
        &(&project, &expected),
    )?;
    let lowered = codegen::lower_with_basic_owned_group_adjacent_by(&project);
    evidence.debug("old-By-ItemAt-COMPLETE-NEW-LOWERING.txt", &lowered)?;
    let program = match lowered {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    let options = codegen_rust::Options {
        package_name: "adjacent442-old-By-ItemAt".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    let old_lower = codegen::lower_with_basic_owned_group_by(&project);
    let validation = codegen::validate_program_with_basic_owned_group_by(&program);
    let emission = codegen::validate_basic_owned_group_by_emission(&program);
    let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(&program);
    let rust = codegen_rust::emit_with_basic_owned_group_by(&program, &options);
    let cs = codegen_csharp::emit_with_basic_owned_group_by(&program);
    evidence.debug(
        "old-By-ItemAt-COMPLETE-ACTUALS.txt",
        &(&old_lower, &validation, &emission, &plan, &rust, &cs),
    )?;
    let diagnostic = match &expected {
        codegen::ProgramValidationError::OwnedSource { diagnostic } => {
            codegen::Diagnostic::OwnedSource {
                diagnostic: diagnostic.clone(),
            }
        }
        _ => unreachable!(),
    };
    Ok(
        matches!(&old_lower,Err(e) if e.diagnostics()==std::slice::from_ref(&diagnostic))
            && validation == Err(expected.clone())
            && emission == Err(expected.clone())
            && matches!(&plan,Err(e) if *e==expected)
            && matches!(&rust,Err(codegen_rust::EmitError::InvalidProgram(e)) if *e==expected)
            && matches!(&cs,Err(codegen_csharp::EmitError::ProgramValidation(e)) if *e==expected),
    )
}

#[test]
#[ignore = "compiler-free preparation only; coordinator supplies a fresh absent evidence root"]
fn basic_adjacent442_prepare_public_projects_and_callers() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let fence = source_bindings()?;
    if fence.len() < 855 {
        return Err("incomplete source closure".into());
    }
    let (controls, projects, profiles) = corpus()?;
    // Independent entire v1 inputs/outcomes, inventory and catalogue oracles
    // precede all product operations. There is no actual-to-expected path.
    evidence.json("controls.json", &controls)?;
    let (legacy_controls, legacy_project) = legacy_native_controls()?;
    evidence.json(
        "legacy-native-COMPLETE-AUTHORED-CONTROLS.json",
        &legacy_controls,
    )?;
    evidence.debug(
        "legacy-native-COMPLETE-AUTHORED-PROJECT.txt",
        &legacy_project,
    )?;
    evidence.debug("ALL-COMPLETE-PUBLIC-PROJECTS.txt", &projects)?;
    evidence.json(
        "ALL-COMPLETE-AUTHORED-CATALOGUES.json",
        &profiles
            .iter()
            .map(|p| json!({"profile":p,"expected":expected_catalogues(p)}))
            .collect::<Vec<_>>(),
    )?;
    let closed = closed_projects();
    for (i, (id, project, expected)) in closed.iter().enumerate() {
        evidence.debug(
            &format!("closed/{i:02}-{id}-COMPLETE-AUTHORED.txt"),
            &(project, expected),
        )?;
    }
    for (i, p) in projects.iter().enumerate() {
        evidence.write(
            &format!("model{i}-project.json"),
            mapping::project_file::encode_pretty(p)?.as_bytes(),
        )?;
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let baseline = historical_artifacts(&evidence, &runtime)?;
    let runtime_sources = freeze_runtime(&evidence)?;
    let mut statuses = Vec::new();
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    let mut direct_checks = Vec::new();
    for (i, p) in projects.iter().enumerate() {
        let options = codegen_rust::Options {
            package_name: format!("adjacent442-model{i}"),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        };
        evidence.debug(&format!("model{i}-COMPLETE-RUST-OPTIONS.txt"), &options)?;
        let validation = engine::validate(p);
        evidence.debug(&format!("model{i}-COMPLETE-VALIDATION.txt"), &validation)?;
        let lowered = codegen::lower_with_basic_owned_group_adjacent_by(p);
        evidence.debug(&format!("model{i}-COMPLETE-LOWERING.txt"), &lowered)?;
        let program = match lowered {
            Ok(program) => program,
            Err(error) => {
                evidence.error(&format!("model{i}-COMPLETE-LOWERING-CHAIN.txt"), &error)?;
                statuses.push((i, false));
                continue;
            }
        };
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(&program);
        let valid = codegen::validate_program_with_basic_owned_group_adjacent_by(&program);
        let emission = codegen::validate_basic_owned_group_adjacent_by_emission(&program);
        evidence.debug(
            &format!("model{i}-COMPLETE-COMMON-ACTUALS.txt"),
            &(&plan, &valid, &emission),
        )?;
        let catalogue_matches = plan
            .as_ref()
            .is_ok_and(|plan| actual_catalogues(plan) == expected_catalogues(&profiles[i]));
        let rust = codegen_rust::emit_with_basic_owned_group_adjacent_by(&program, &options);
        let cs = codegen_csharp::emit_with_basic_owned_group_adjacent_by(&program);
        match &rust {
            Ok(a) => evidence.artifacts(&format!("rust-model{i}"), a)?,
            Err(e) => evidence.error(&format!("model{i}-COMPLETE-RUST-ERROR.txt"), e)?,
        }
        match &cs {
            Ok(a) => evidence.artifacts(&format!("csharp-model{i}"), a)?,
            Err(e) => evidence.error(&format!("model{i}-COMPLETE-CSHARP-ERROR.txt"), e)?,
        }
        statuses.push((
            i,
            validation.is_empty()
                && catalogue_matches
                && valid.is_ok()
                && emission.is_ok()
                && rust.is_ok()
                && cs.is_ok(),
        ));
        if i == 0 {
            direct_checks.push((
                "old-profiles",
                old_family_controls(p, &program, &evidence, &runtime)?,
            ));
            direct_checks.push((
                "old-By-ItemAt",
                old_by_item_at_control(&evidence, &runtime)?,
            ));
            let direct = closed_programs(&program);
            evidence.debug("closed/ALL-COMPLETE-DIRECT-PROGRAMS-AUTHORED.txt", &direct)?;
            if direct.len() != DIRECT_PROGRAMS {
                return Err("direct refusal inventory changed".into());
            }
            for (id, program, expected) in direct {
                let before = program.clone();
                let validation =
                    codegen::validate_program_with_basic_owned_group_adjacent_by(&program);
                let emission = codegen::validate_basic_owned_group_adjacent_by_emission(&program);
                let plan =
                    codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(
                        &program,
                    );
                let rust =
                    codegen_rust::emit_with_basic_owned_group_adjacent_by(&program, &options);
                let cs = codegen_csharp::emit_with_basic_owned_group_adjacent_by(&program);
                evidence.debug(
                    &format!("closed/{id}-COMPLETE-ACTUALS.txt"),
                    &(&validation, &emission, &plan, &rust, &cs, &program),
                )?;
                let matched = validation == Err(expected.clone())
                    && emission == Err(expected.clone())
                    && matches!(&plan,Err(e) if *e==expected)
                    && matches!(&rust,Err(codegen_rust::EmitError::InvalidProgram(e)) if *e==expected)
                    && matches!(&cs,Err(codegen_csharp::EmitError::ProgramValidation(e)) if *e==expected)
                    && program == before;
                direct_checks.push((id, matched));
            }
        }
        let count = controls["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["model"] == i)
            .map(|c| c["calls"].as_array().unwrap().len())
            .sum::<usize>();
        evidence.write(
            &format!("csharp-host{i}/Host.csproj"),
            host_project(&format!("model{i}")).as_bytes(),
        )?;
        evidence.write(
            &format!("csharp-host{i}/Program.cs"),
            csharp_host(count).as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{i}={{package=\"adjacent442-model{i}\",path=\"../rust-model{i}\"}}\n"
        ));
        dispatch.push_str(&format!("{i}=>invoke!(model{i},selection,&source,&inputs,&execution,limits,&directory,&prefix,call,&before,&named),\n"));
    }
    let mut closed_checks = Vec::new();
    for (i, (id, p, expected)) in closed.iter().enumerate() {
        let actual = codegen::lower_with_basic_owned_group_adjacent_by(p);
        evidence.debug(&format!("closed/{i:02}-{id}-COMPLETE-ACTUAL.txt"), &actual)?;
        closed_checks.push((*id, matches!(&actual,Err(e) if e.diagnostics()==expected)));
    }
    let legacy = prepare_legacy(&evidence, &runtime, &runtime_sources, &baseline)?;
    evidence.debug(
        "ALL-COMPLETE-PREPARATION-COMPARISONS.txt",
        &(&statuses, &direct_checks, &closed_checks, legacy),
    )?;
    evidence.write(
        "rust-host/src/main.rs",
        rust_host(&dispatch, CORE_CALLS).as_bytes(),
    )?;
    evidence.write(
        "rust-host/src/codec.rs",
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    evidence.write("rust-host/Cargo.toml",format!("[package]\nname=\"adjacent442-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\nlegacy0={{package=\"ferrule-generated-mapping\",path=\"../rust-legacy\"}}\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;
    if statuses.len() != MODELS
        || statuses.iter().any(|(_, ok)| !*ok)
        || direct_checks.iter().any(|(_, ok)| !*ok)
        || closed_checks.iter().any(|(_, ok)| !*ok)
        || !legacy
    {
        return Err("adjacent442 compiler-free preparation differs; all originals retained; no compiler/native evaluator started".into());
    }
    if fence != source_bindings()?
        || !legacy_artifact_policy::snapshot_unchanged(&root()?, &runtime_sources)?
    {
        return Err("source closure changed during preparation".into());
    }
    evidence.json(SUCCESS, &binding(&evidence.0)?)?;
    Ok(())
}

fn legacy_native_controls() -> TestResult<(Json, Project)> {
    // This tiny adapter is confined to the immutable ordinary JSON fixture;
    // all owned scalar/float/origin data uses the borrowed lossless codec.
    fn tree(value: &Json) -> Instance {
        match value {
            Json::Object(fields) => Instance::Group(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), tree(value)))
                    .collect::<Vec<_>>()
                    .into(),
            ),
            Json::Array(items) => Instance::Repeated(items.iter().map(tree).collect()),
            Json::String(value) => Instance::Scalar(Value::String(value.clone())),
            Json::Number(value) => Instance::Scalar(Value::Int(value.as_i64().unwrap())),
            Json::Bool(value) => Instance::Scalar(Value::Bool(*value)),
            Json::Null => Instance::Scalar(Value::Null),
        }
    }
    let source = tree(&serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-adjacent-by/legacy-input.json"
    ))?);
    let expected = tree(&serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-adjacent-by/legacy-expected.json"
    ))?);
    let project = mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-adjacent-by/legacy-project.json"
    ))?;
    let controls = json!({"version":1,"cases":[{"id":"L01-ordinary-adjacent","model":0,"source":codec::encode(&source),"inputs":[],"calls":[{"selection":"All","expected":cases::expected(expected.clone())},{"selection":"Primary","expected":cases::expected(expected)}]}]});
    Ok((controls, project))
}
fn legacy_native(evidence: &Evidence) -> TestResult<Vec<Json>> {
    let (controls, project) = legacy_native_controls()?;
    if fs::read(
        evidence
            .0
            .join("legacy-native-COMPLETE-AUTHORED-CONTROLS.json"),
    )? != serde_json::to_vec_pretty(&controls)?
    {
        return Err("frozen ordinary legacy call inventory/oracles differ".into());
    }
    run_native(&controls, &[project], &evidence.0.join("legacy-control"))
}

#[test]
#[ignore = "requires unchanged successful preparation and coordinator serialized compiler/native lane grant"]
fn basic_adjacent442_qualify_native_and_public_consumers() -> TestResult<()> {
    let directory = PathBuf::from(
        std::env::var_os(ENVIRONMENT).ok_or("successful preparation directory required")?,
    );
    if !directory.is_absolute() || !directory.is_dir() {
        return Err("absolute existing successful preparation required".into());
    }
    let success: Json = serde_json::from_slice(&fs::read(directory.join(SUCCESS))?)?;
    if success != binding(&directory)? {
        return Err(
            "stale/altered source, commands, native binary, artifacts or independent recipes"
                .into(),
        );
    }
    let (controls, projects, _) = corpus()?;
    if fs::read(directory.join("controls.json"))? != serde_json::to_vec_pretty(&controls)? {
        return Err("frozen control bytes differ".into());
    }
    for (i, p) in projects.iter().enumerate() {
        if fs::read(directory.join(format!("model{i}-project.json")))?
            != mapping::project_file::encode_pretty(p)?.as_bytes()
        {
            return Err("genuine prepared Project bytes differ".into());
        }
    }
    let evidence = Evidence(directory.clone());
    let native = run_native(&controls, &projects, &evidence.0)?;
    let legacy = legacy_native(&evidence)?;
    evidence.json("ALL-COMPLETE-NATIVE-COMPARISONS.json", &native)?;
    evidence.json("ALL-COMPLETE-LEGACY-NATIVE-COMPARISONS.json", &legacy)?;
    // Every independent call is attempted after mismatches/observer failures.
    // Required evidence I/O remains fatal, never a swallowed comparison error.
    let nuget_cache = std::env::var_os("NUGET_PACKAGES")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|p| PathBuf::from(p).join(".nuget/packages"))
        })
        .filter(|p| p.is_dir())
        .and_then(|p| p.canonicalize().ok());
    let mut statuses = Vec::new();
    let mut rust = Command::new("cargo");
    rust.args(["run", "--quiet", "--jobs", "1", "--manifest-path"])
        .arg(directory.join("rust-host/Cargo.toml"))
        .arg("--")
        .arg(directory.join("controls.json"))
        .arg(directory.join("rust-actual"))
        .current_dir(&directory)
        .env("RUSTFLAGS", "-D warnings")
        .env("RUST_BACKTRACE", "1");
    let mut rust_sources = vec![
        PathBuf::from("rust-host"),
        PathBuf::from("rust-legacy"),
        PathBuf::from("controls.json"),
        PathBuf::from(SUCCESS),
    ];
    rust_sources.extend((0..MODELS).map(|i| PathBuf::from(format!("rust-model{i}"))));
    let output = host_policy::recorded_output_with_sources(
        &mut rust,
        &directory,
        "adjacent442-rust-host",
        &rust_sources,
    )?;
    statuses.push(("rust-host".to_owned(), output.status.success()));
    for index in 0..=MODELS {
        let legacy = index == MODELS;
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
        let mut sources = vec![
            PathBuf::from(&host),
            PathBuf::from(format!("csharp-{label}")),
            PathBuf::from(SUCCESS),
        ];
        sources.extend(if legacy {
            vec![
                PathBuf::from("legacy-input.json"),
                PathBuf::from("legacy-expected.json"),
            ]
        } else {
            vec![PathBuf::from("controls.json")]
        });
        let artifacts = directory.join(format!("dotnet-artifacts-{label}"));
        let mut build = dotnet_command(&directory);
        if let Some(cache) = &nuget_cache {
            build.env("NUGET_PACKAGES", cache);
        }
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
            .arg(directory.join(&host).join("Host.csproj"))
            .current_dir(&directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let built = host_policy::recorded_output_with_sources(
            &mut build,
            &directory,
            &format!("adjacent442-csharp-{label}-build"),
            &sources,
        )?;
        statuses.push((format!("csharp-{label}-build"), built.status.success()));
        if !built.status.success() {
            continue;
        }
        let binaries = hashes(artifact_files(&artifacts.join("bin"))?);
        evidence.json(&format!("csharp-{label}-PRE-RUN-BINARIES.json"), &binaries)?;
        let mut run = dotnet_command(&directory);
        if let Some(cache) = &nuget_cache {
            run.env("NUGET_PACKAGES", cache);
        }
        run.arg(artifacts.join("bin/Host/release/Host.dll"));
        if legacy {
            run.arg(directory.join("legacy-input.json"))
                .arg(directory.join("legacy-expected.json"));
        } else {
            run.arg(directory.join("controls.json"))
                .arg(index.to_string());
        }
        run.arg(directory.join(format!("csharp-{label}-actual")))
            .current_dir(&directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let output = host_policy::recorded_output_with_sources(
            &mut run,
            &directory,
            &format!("adjacent442-csharp-{label}-host"),
            &sources,
        )?;
        let after = hashes(artifact_files(&artifacts.join("bin"))?);
        evidence.json(&format!("csharp-{label}-POST-RUN-BINARIES.json"), &after)?;
        statuses.push((
            format!("csharp-{label}-host"),
            output.status.success() && binaries == after,
        ));
    }
    evidence.debug("ALL-GENERATED-EXECUTION-STATUSES.txt", &statuses)?;
    verify_original_evidence(&directory, &success["evidence"])?;
    if success["sources"] != serde_json::to_value(source_bindings()?)?
        || success["commands"] != command_runtime_binding()?
    {
        return Err(
            "source/command/native-binary closure changed during qualification; originals retained"
                .into(),
        );
    }
    if native.len() != CORE_CALLS
        || legacy.len() != LEGACY_CALLS
        || native.iter().chain(&legacy).any(|c| c["matched"] != true)
        || statuses.len() != 1 + 2 * (MODELS + 1)
        || statuses.iter().any(|(_, ok)| !*ok)
    {
        return Err(
            "adjacent442 same-Project qualification differs; complete originals retained".into(),
        );
    }
    Ok(())
}
