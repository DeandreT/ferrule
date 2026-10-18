//! #819: independently authored genuine Projects for the explicit #441 profile.
//! No compiler, native evaluator or generated consumer runs during preparation.
use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest as _, Sha256};
use std::{fs, io::Write as _};

#[path = "../../../../conformance/basic-owned-group-by/cases.rs"]
mod cases;
#[path = "../../../../conformance/basic-owned-group-by/closed.rs"]
mod closed;
#[path = "basic_owned_group_by/codec.rs"]
mod codec;
#[path = "basic_owned_group_by/native.rs"]
mod native;
#[path = "../../../../conformance/basic-owned-group-by/projects.rs"]
mod projects;

const MODELS: usize = 45;
const LEGACY_CALLS: usize = 2;
const ENVIRONMENT: &str = "FERRULE_BASIC_GROUP441_PREPARE_DIR";
const SUCCESS: &str = "SUCCESSFUL-COMPILER-FREE-PREPARATION.json";
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
            return Err("fresh absolute preparation path with existing parent required".into());
        }
        fs::create_dir(&path)?;
        host_policy::preflight_environment(&path)?;
        eprintln!("GROUP441_ORIGINALS={}", path.display());
        Ok(Self(path))
    }
    fn write(&self, name: &str, bytes: &[u8]) -> TestResult<()> {
        self.write_io(name, bytes)?;
        Ok(())
    }
    fn write_io(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.0.join(name);
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| io::Error::other("evidence parent"))?,
        )?;
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
    fn error(&self, name: &str, error: &(dyn std::error::Error + 'static)) -> TestResult<()> {
        let mut original = format!("Debug={error:#?}\nDisplay={error}\n");
        let mut source = error.source();
        while let Some(error) = source {
            original.push_str(&format!("CauseDebug={error:#?}\nCauseDisplay={error}\n"));
            source = error.source();
        }
        original.push_str("CAUSE_CHAIN_TERMINATED=None\n");
        self.write(name, original.as_bytes())
    }
    fn artifacts(&self, name: &str, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
        let mut index = Vec::new();
        for file in artifacts.files() {
            self.write(&format!("{name}/{}", file.path.as_str()), &file.contents)?;
            index.push((
                file.path.as_str(),
                file.contents.len(),
                format!("{:x}", Sha256::digest(&file.contents)),
            ));
        }
        self.write(
            &format!("{name}-COMPLETE-ARTIFACT-INDEX.json"),
            &serde_json::to_vec_pretty(&index)?,
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
fn hashes(files: ArtifactFiles) -> Vec<(String, usize, String)> {
    files
        .into_iter()
        .map(|(path, bytes)| (path, bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
        .collect()
}
fn source_bindings() -> TestResult<Vec<(String, usize, String)>> {
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
        "conformance/basic-owned-group-by",
        "crates/cli/tests/code_generation/basic_owned_group_by",
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
        "crates/cli/tests/code_generation/basic_owned_group_by.rs",
    ] {
        files.push((relative.into(), fs::read(root.join(relative))?));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(hashes(files))
}
fn corpus() -> TestResult<(Json, Vec<Project>, Vec<String>)> {
    let cases = cases::cases();
    let mut profiles = Vec::<String>::new();
    let mut projects = Vec::new();
    let mut controls = Vec::new();
    for case in &cases {
        let model = match profiles.iter().position(|profile| profile == case.profile) {
            Some(index) => index,
            None => {
                profiles.push(case.profile.into());
                projects.push(projects::project(case.profile));
                profiles.len() - 1
            }
        };
        controls.push(json!({"id":case.id,"profile":case.profile,"model":model,"native":case.native,"source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,instance)|json!([name,codec::encode(instance)])).collect::<Vec<_>>(),"calls":case.calls}));
    }
    let calls = cases.iter().map(|case| case.calls.len()).sum::<usize>();
    if cases.len() != cases::CASE_COUNT
        || calls != cases::CALL_COUNT
        || projects.len() != MODELS
        || closed::projects().len() != closed::COUNT
    {
        return Err(format!(
            "authored inventory changed: cases={} calls={calls} models={} closed={}",
            cases.len(),
            projects.len(),
            closed::projects().len()
        )
        .into());
    }
    Ok((
        json!({"version":1,"cases":controls,"models":profiles}),
        projects,
        profiles,
    ))
}
fn binding(directory: &Path) -> TestResult<Json> {
    let files = artifact_files(directory)?
        .into_iter()
        .filter(|(path, _)| path != SUCCESS)
        .collect();
    Ok(
        json!({"version":1,"result":"success","cases":cases::CASE_COUNT,"core_calls_per_engine":cases::CALL_COUNT,"legacy_calls_per_engine":LEGACY_CALLS,
        "models":MODELS,"closed_projects":closed::COUNT,"sources":source_bindings()?,"evidence":hashes(files),
        "runtime":Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime").canonicalize()?}),
    )
}
fn verify_original_evidence(directory: &Path, original: &Json) -> TestResult<()> {
    let files: Vec<(String, usize, String)> = serde_json::from_value(original.clone())?;
    for (relative, length, digest) in files {
        let bytes = fs::read(directory.join(&relative))?;
        if bytes.len() != length || format!("{:x}", Sha256::digest(&bytes)) != digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("original prepared evidence changed: {relative}"),
            )
            .into());
        }
    }
    Ok(())
}
fn expected_catalogues(profile: &str) -> Json {
    fn catalogue(names: &[&str], required: &[&str]) -> Json {
        json!(
            names
                .iter()
                .enumerate()
                .map(
                    |(index, name)| json!({"declaration_index":index,"name":name,
            "availability":if required.contains(name) {"RequiredLegacy"} else {"Optional"}})
                )
                .collect::<Vec<_>>()
        )
    }
    if profile.starts_with("root-") || profile == "scalar-values" {
        return json!({"all_outputs":[],"primary":[],"named_targets":[]});
    }
    if profile.starts_with("legacy-named-repeated-root") {
        let values = catalogue(&["Alias"], &["Alias"]);
        return json!({"all_outputs":values,"primary":values,"named_targets":[{"target_index":0,"name":projects::AUDIT,"catalogue":values}]});
    }
    let names = ["Rows", projects::EXACT, "RequiredLegacy", "Unused"];
    let optional = catalogue(&names, &[]);
    let required = catalogue(&names, &["RequiredLegacy"]);
    let legacy_rows = matches!(
        profile,
        "legacy-key-only" | "legacy-post-only" | "legacy-island"
    );
    json!({"all_outputs":if legacy_rows {catalogue(&names,&["Rows","RequiredLegacy"])} else {required.clone()},
        "primary":if legacy_rows {catalogue(&names,&["Rows"])} else {optional.clone()},
        "named_targets":[{"target_index":0,"name":projects::AUDIT,"catalogue":optional},
        {"target_index":1,"name":"legacy","catalogue":required}]})
}
fn actual_catalogues(plan: &codegen::NamedInputAdmissionPlan<'_>) -> Json {
    fn catalogue(value: &codegen::NamedInputCatalogue<'_>) -> Json {
        json!(
            value
                .entries
                .iter()
                .map(
                    |entry| json!({"declaration_index":entry.declaration_index,"name":entry.name,
            "availability":format!("{:?}",entry.availability)})
                )
                .collect::<Vec<_>>()
        )
    }
    json!({"all_outputs":catalogue(&plan.all_outputs),"primary":catalogue(&plan.primary),"named_targets":plan.named_targets.iter().map(|target|
        json!({"target_index":target.target_index,"name":target.name,"catalogue":catalogue(&target.catalogue)})).collect::<Vec<_>>()})
}
fn host_project(model: usize) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
#[test]
fn basic_owned_group441_authored_inventory_and_lossless_recipes() -> TestResult<()> {
    let (controls, _, _) = corpus()?;
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
    Ok(())
}
#[test]
fn basic_owned_group441_original_file_fence_rejects_mutation_and_allows_outputs() -> TestResult<()>
{
    let directory = TempDir::new("group441_original_file_fence")?;
    let evidence = Evidence(directory.0.clone());
    evidence.write("controls.json", b"control A")?;
    evidence.write("rust-host/src/main.rs", b"caller A")?;
    let original = serde_json::to_value(hashes(artifact_files(&directory.0)?))?;
    verify_original_evidence(&directory.0, &original)?;
    evidence.write("rust-host/Cargo.lock", b"new legitimate build output")?;
    evidence.write("actual/outcome.json", b"new retained outcome")?;
    verify_original_evidence(&directory.0, &original)?;
    for (path, replacement, restored) in [
        (
            "controls.json",
            b"control B".as_slice(),
            b"control A".as_slice(),
        ),
        (
            "rust-host/src/main.rs",
            b"caller B".as_slice(),
            b"caller A".as_slice(),
        ),
    ] {
        fs::write(directory.0.join(path), replacement)?;
        let error = verify_original_evidence(&directory.0, &original).unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(directory.0.join(path), restored)?;
    }
    fs::remove_file(directory.0.join("rust-host/src/main.rs"))?;
    let error = verify_original_evidence(&directory.0, &original).unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::NotFound
    );
    Ok(())
}
#[test]
fn basic_owned_group441_raw_outcomes_survive_observation_failure_and_continue() -> TestResult<()> {
    let directory = TempDir::new("group441_observation_retention")?;
    let evidence = Evidence(directory.0.clone());
    let literal = "\t p:Retained \r\n";
    let identity = "{urn:group441:capture}Retained";
    let fields: codegen_runtime::InstanceGroup = vec![
        (
            "payload".into(),
            Instance::Scalar(Value::Float(f64::from_bits(0x7ff8_0000_0000_0041))),
        ),
        (
            "sentinel".into(),
            Instance::Scalar(Value::String("comparison sentinel".into())),
        ),
    ]
    .into();
    let annotated = Instance::Group(fields.with_xml_type_origin(
        ir::XmlTypeOrigin::ExplicitPadded {
            literal,
            resolved_identity: identity,
        },
    )?);
    let mapped = Instance::MappedSequence(vec![annotated.clone()]);
    let documents = Instance::DocumentSet(vec![
        ir::DocumentMember::new_source("retained.xml", "original.xml", mapped.clone()).unwrap(),
        ir::DocumentMember::new("portable-only.xml", mapped.clone()).unwrap(),
    ]);
    let mut checks = Vec::new();
    for (ordinal, (value, comparison_panics)) in [
        (mapped, false),
        (Instance::Scalar(Value::String("continued".into())), false),
        (documents, false),
        (Instance::Scalar(Value::String("continued".into())), false),
        (annotated, true),
        (Instance::Scalar(Value::String("continued".into())), false),
    ]
    .into_iter()
    .enumerate()
    {
        let actual: std::thread::Result<Result<Instance, codegen_runtime::RuntimeError>> =
            Ok(Ok(value));
        let observed = codec::retain_outcome(
            actual,
            |suffix, bytes| evidence.write_io(&format!("{ordinal}-{suffix}"), bytes),
            codec::raw_tree,
            |product| {
                let encoded = codec::encode(product.as_ref().unwrap());
                assert!(!comparison_panics, "comparison sentinel panic");
                json!({"matched":encoded==json!(["String","continued"]),"actual":encoded})
            },
        )?;
        checks.push(observed.is_some_and(|record| record["matched"] == true));
    }
    assert_eq!(checks, [false, true, false, true, false, true]);
    for (ordinal, marker) in [
        (0, "MappedSequence"),
        (2, "DocumentSet"),
        (4, "comparison sentinel"),
    ] {
        let raw = fs::read_to_string(
            directory
                .0
                .join(format!("{ordinal}-COMPLETE-RAW-PRODUCT.txt")),
        )?;
        assert!(raw.contains(marker));
        assert!(
            directory
                .0
                .join(format!("{ordinal}-COMPLETE-OBSERVATION-FAILURE.txt"))
                .is_file()
        );
    }
    fn retained_group(group: &Json, literal: &str, identity: &str) {
        assert_eq!(group[0], "Group");
        assert_eq!(group[1][0][0], "payload");
        assert_eq!(group[1][0][1][0], "Float");
        assert_eq!(group[1][0][1][1], "7ff8000000000041");
        assert_eq!(group[2]["kind"], "ExplicitPadded");
        assert_eq!(
            group[2]["identity"].as_str().unwrap().as_bytes(),
            identity.as_bytes()
        );
        assert_eq!(
            group[2]["literal"].as_str().unwrap().as_bytes(),
            literal.as_bytes()
        );
    }
    let mut captured = Vec::new();
    for ordinal in [0, 2, 4] {
        let bytes = fs::read(
            directory
                .0
                .join(format!("{ordinal}-COMPLETE-RAW-PRODUCT.json")),
        )?;
        assert!(bytes.windows(16).any(|bytes| bytes == b"7ff8000000000041"));
        captured.push(serde_json::from_slice::<Json>(&bytes)?);
    }
    assert_eq!(captured[0]["Ok"][0], "MappedSequence");
    retained_group(&captured[0]["Ok"][1][0], literal, identity);
    assert_eq!(captured[1]["Ok"][0], "DocumentSet");
    let documents = &captured[1]["Ok"][1];
    assert_eq!(documents[0]["path"], "retained.xml");
    assert_eq!(documents[0]["source_path"], "original.xml");
    assert!(
        documents[0]["member_debug"]
            .as_str()
            .unwrap()
            .contains("resolved_source_path: Some(")
    );
    assert_eq!(documents[1]["path"], "portable-only.xml");
    assert_eq!(documents[1]["source_path"], "portable-only.xml");
    assert!(
        documents[1]["member_debug"]
            .as_str()
            .unwrap()
            .contains("resolved_source_path: None")
    );
    for document in documents.as_array().unwrap() {
        assert_eq!(document["value"][0], "MappedSequence");
        retained_group(&document["value"][1][0], literal, identity);
    }
    retained_group(&captured[2]["Ok"], literal, identity);
    let denied: TestResult<Option<Json>> = codec::retain_outcome(
        Ok(Ok::<_, codegen_runtime::RuntimeError>(Instance::Scalar(
            Value::Null,
        ))),
        |suffix, _| {
            if suffix == "COMPLETE-RAW-PRODUCT.json" {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "required capture denied",
                ))
            } else {
                Ok(())
            }
        },
        codec::raw_tree,
        |_| panic!("observer must not run without raw evidence"),
    );
    assert_eq!(
        denied
            .unwrap_err()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    Ok(())
}
#[test]
#[ignore = "compiler-free public preparation only; coordinator supplies a fresh evidence directory"]
fn prepare_basic_owned_group441_public_projects_and_callers() -> TestResult<()> {
    let evidence = Evidence::new()?;
    let source_fence = source_bindings()?;
    let (controls, projects, profiles) = corpus()?;
    // Complete independent inputs/expected metadata precede all product calls.
    evidence.write("controls.json", &serde_json::to_vec_pretty(&controls)?)?;
    evidence.debug("ALL-COMPLETE-PUBLIC-PROJECTS.txt", &projects)?;
    evidence.write(
        "ALL-COMPLETE-AUTHORED-CATALOGUES.json",
        &serde_json::to_vec_pretty(
            &profiles
                .iter()
                .map(|profile| json!({"profile":profile,"expected":expected_catalogues(profile)}))
                .collect::<Vec<_>>(),
        )?,
    )?;
    let closed = closed::projects();
    for (ordinal, (id, project, expected)) in closed.iter().enumerate() {
        evidence.debug(
            &format!("closed/{ordinal:02}-{id}-COMPLETE-AUTHORED.txt"),
            &(project, expected),
        )?;
    }
    for (index, project) in projects.iter().enumerate() {
        evidence.write(
            &format!("model{index}-project.json"),
            mapping::project_file::encode_pretty(project)?.as_bytes(),
        )?;
    }
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut statuses = Vec::new();
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in projects.iter().enumerate() {
        let options = codegen_rust::Options {
            package_name: format!("group441-model{index}"),
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
        let lowered = codegen::lower_with_basic_owned_group_by(project);
        evidence.debug(&format!("model{index}-COMPLETE-LOWERING.txt"), &lowered)?;
        let program = match lowered {
            Ok(program) => program,
            Err(error) => {
                evidence.error(&format!("model{index}-COMPLETE-LOWERING-CHAIN.txt"), &error)?;
                statuses.push((index, false));
                continue;
            }
        };
        let admission =
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(&program);
        evidence.debug(
            &format!("model{index}-COMPLETE-CATALOGUE-ACTUAL.txt"),
            &admission,
        )?;
        let admission_matches = admission
            .as_ref()
            .is_ok_and(|plan| actual_catalogues(plan) == expected_catalogues(&profiles[index]));
        let rust = codegen_rust::emit_with_basic_owned_group_by(&program, &options);
        let csharp = codegen_csharp::emit_with_basic_owned_group_by(&program);
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
        statuses.push((
            index,
            validation.is_empty() && admission_matches && rust.is_ok() && csharp.is_ok(),
        ));
        let count = controls["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["model"] == index)
            .map(|case| case["calls"].as_array().unwrap().len())
            .sum::<usize>();
        evidence.write(
            &format!("csharp-host{index}/Host.csproj"),
            host_project(index).as_bytes(),
        )?;
        evidence.write(
            &format!("csharp-host{index}/Program.cs"),
            include_str!("basic_owned_group_by/Host.cs.txt")
                .replace("@MODEL_CALLS@", &count.to_string())
                .as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"group441-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "{index}=>invoke!(model{index},selection,&source,&inputs,&execution,limits,&directory,&prefix,call,&before,&named),\n"
        ));
    }
    let mut closed_checks = Vec::new();
    for (ordinal, (id, project, expected)) in closed.iter().enumerate() {
        let actual = codegen::lower_with_basic_owned_group_by(project);
        evidence.debug(
            &format!("closed/{ordinal:02}-{id}-COMPLETE-ACTUAL.txt"),
            &actual,
        )?;
        closed_checks.push((
            *id,
            matches!(&actual,Err(error) if error.diagnostics()==expected),
        ));
    }
    evidence.debug("ALL-COMPLETE-CLOSED-COMPARISONS.txt", &closed_checks)?;
    // Old-family gates and legacy saved-project controls are prepared separately.
    let legacy = prepare_legacy(&evidence, &runtime)?;
    let old = old_family_controls(&projects[0], &evidence, &runtime)?;
    evidence.debug(
        "ALL-COMPLETE-PREPARATION-OUTCOMES.txt",
        &(&profiles, &statuses),
    )?;
    evidence.write(
        "rust-host/src/main.rs",
        include_str!("basic_owned_group_by/Host.rs.txt")
            .replace("@DISPATCH@", &dispatch)
            .as_bytes(),
    )?;
    evidence.write(
        "rust-host/src/codec.rs",
        include_bytes!("basic_owned_group_by/codec.rs"),
    )?;
    evidence.write(
        "rust-host/src/legacy_control.rs",
        include_bytes!("basic_owned_group_by/legacy_control.rs.txt"),
    )?;
    for (name, bytes) in [
        (
            "legacy-input.json",
            include_bytes!("../../../../conformance/basic-owned-group-by/legacy-input.json")
                .as_slice(),
        ),
        (
            "legacy-expected.json",
            include_bytes!("../../../../conformance/basic-owned-group-by/legacy-expected.json")
                .as_slice(),
        ),
    ] {
        evidence.write(&format!("rust-host/src/{name}"), bytes)?;
    }
    evidence.write("rust-host/Cargo.toml",format!("[package]\nname=\"group441-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\nlegacy0={{package=\"group441-legacy\",path=\"../rust-legacy\"}}\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;
    if statuses.len() != MODELS
        || statuses.iter().any(|(_, matched)| !*matched)
        || closed_checks.iter().any(|(_, matched)| !*matched)
        || !legacy
        || !old
    {
        return Err("group441 public preparation differs; originals retained; no compiler or native evaluator started".into());
    }
    if source_fence != source_bindings()? {
        return Err("sources changed during preparation; originals retained".into());
    }
    evidence.write(SUCCESS, &serde_json::to_vec_pretty(&binding(&evidence.0)?)?)?;
    Ok(())
}
fn prepare_legacy(evidence: &Evidence, runtime: &Path) -> TestResult<bool> {
    let original = include_str!("../../../../conformance/basic-owned-group-by/legacy-project.json");
    evidence.write("legacy-original-project.json", original.as_bytes())?;
    let project = mapping::project_file::decode_str(original)?;
    let encoded = mapping::project_file::encode_pretty(&project)?;
    evidence.write("legacy-roundtrip-project.json", encoded.as_bytes())?;
    let decoded = mapping::project_file::decode_str(&encoded)?;
    let same = serde_json::to_value(&project)? == serde_json::to_value(&decoded)?
        && !encoded.contains("owned_source");
    let lowered = codegen::lower(&project);
    evidence.debug("legacy-COMPLETE-LOWERING.txt", &lowered)?;
    let program = match lowered {
        Ok(program) => program,
        Err(_) => return Ok(false),
    };
    let options = codegen_rust::Options {
        package_name: "group441-legacy".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    let rust = codegen_rust::emit(&program, &options);
    let csharp = codegen_csharp::emit(&program);
    match &rust {
        Ok(artifacts) => evidence.artifacts("rust-legacy", artifacts)?,
        Err(error) => evidence.error("legacy-COMPLETE-RUST-ERROR.txt", error)?,
    }
    match &csharp {
        Ok(artifacts) => evidence.artifacts("csharp-legacy", artifacts)?,
        Err(error) => evidence.error("legacy-COMPLETE-CSHARP-ERROR.txt", error)?,
    }
    evidence.write(
        "csharp-legacy-host/Host.csproj",
        host_project(0)
            .replace("../csharp-model0/", "../csharp-legacy/")
            .as_bytes(),
    )?;
    evidence.write(
        "csharp-legacy-host/Program.cs",
        include_bytes!("basic_owned_group_by/LegacyHost.cs.txt"),
    )?;
    evidence.write(
        "legacy-input.json",
        include_bytes!("../../../../conformance/basic-owned-group-by/legacy-input.json"),
    )?;
    evidence.write(
        "legacy-expected.json",
        include_bytes!("../../../../conformance/basic-owned-group-by/legacy-expected.json"),
    )?;
    Ok(same && rust.is_ok() && csharp.is_ok())
}
fn old_family_controls(project: &Project, evidence: &Evidence, runtime: &Path) -> TestResult<bool> {
    let program = codegen::lower_with_basic_owned_group_by(project)?;
    let options = codegen_rust::Options {
        package_name: "group441-old-gate".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    };
    let expected = codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(closed::scope_error(
            codegen::OwnedSourceCapabilityReason::Grouping,
        )),
    };
    evidence.debug("old-families-COMPLETE-AUTHORED-EXPECTED.txt", &expected)?;
    let rust = [
        codegen_rust::emit(&program, &options),
        codegen_rust::emit_with_basic_owned_sources(&program, &options),
        codegen_rust::emit_with_basic_owned_filters(&program, &options),
        codegen_rust::emit_with_basic_owned_sort(&program, &options),
        codegen_rust::emit_with_basic_owned_windows(&program, &options),
        codegen_rust::emit_with_basic_owned_row_failures(&program, &options),
    ];
    let csharp = [
        codegen_csharp::emit(&program),
        codegen_csharp::emit_with_basic_owned_sources(&program),
        codegen_csharp::emit_with_basic_owned_filters(&program),
        codegen_csharp::emit_with_basic_owned_sort(&program),
        codegen_csharp::emit_with_basic_owned_windows(&program),
        codegen_csharp::emit_with_basic_owned_row_failures(&program),
    ];
    evidence.debug("old-families-COMPLETE-RUST-ACTUALS.txt", &rust)?;
    evidence.debug("old-families-COMPLETE-CSHARP-ACTUALS.txt", &csharp)?;
    Ok(rust.iter().all(|result|matches!(result,Err(codegen_rust::EmitError::InvalidProgram(error)) if *error==expected))&&
        csharp.iter().all(|result|matches!(result,Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error==expected)))
}
fn legacy_native(evidence: &Evidence) -> TestResult<Vec<Json>> {
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
        "../../../../conformance/basic-owned-group-by/legacy-input.json"
    ))?);
    let expected = tree(&serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-by/legacy-expected.json"
    ))?);
    let project = mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-by/legacy-project.json"
    ))?;
    let context = engine::ExecutionContext::new(Path::new("group441.mapping"));
    let mut checks = Vec::new();
    evidence.debug(
        "native/legacy-COMPLETE-AUTHORED-EXPECTED.txt",
        &(&project, &source, &expected),
    )?;
    #[derive(Debug)]
    enum Product {
        All(engine::ExecutionOutputs),
        Selected(engine::SelectedTargetOutput),
    }
    for selection in ["All", "Primary"] {
        let actual = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if selection == "All" {
                engine::run_outputs_with_sources_and_context(&project, &source, vec![], &context)
                    .map(Product::All)
            } else {
                engine::run_selected_target_with_sources_and_context(
                    &project,
                    &source,
                    vec![],
                    &context,
                    engine::TargetSelection::Primary,
                )
                .map(Product::Selected)
            }
        }));
        let observed = codec::retain_outcome(
            actual,
            |suffix, bytes| {
                evidence.write_io(&format!("native/legacy-{selection}-{suffix}"), bytes)
            },
            |product| match product {
                Product::All(output) => {
                    json!({"variant":"All","primary":codec::raw_tree(&output.primary),
                    "extras":output.extras.iter().map(|output|json!({"name":output.name,"instance":codec::raw_tree(&output.instance)})).collect::<Vec<_>>()})
                }
                Product::Selected(engine::SelectedTargetOutput::Primary(instance)) => {
                    json!({"variant":"Primary","instance":codec::raw_tree(instance)})
                }
                Product::Selected(engine::SelectedTargetOutput::Named(output)) => {
                    json!({"variant":"Named","name":output.name,"instance":codec::raw_tree(&output.instance)})
                }
            },
            |product| {
                let matched = match product {
                    Ok(Product::All(output)) => {
                        output.primary == expected && output.extras.is_empty()
                    }
                    Ok(Product::Selected(engine::SelectedTargetOutput::Primary(tree))) => {
                        *tree == expected
                    }
                    _ => false,
                };
                json!({"matched":matched})
            },
        )?;
        checks.push(json!({"case":"legacy-saved","selection":selection,
            "matched":observed.is_some_and(|record|record["matched"]==true)}));
    }
    Ok(checks)
}
#[test]
#[ignore = "requires successful unchanged preparation and explicit coordinator serialized compiler/native lane grant"]
fn qualify_basic_owned_group441_native_and_public_consumers() -> TestResult<()> {
    let directory = PathBuf::from(
        std::env::var_os(ENVIRONMENT).ok_or("successful preparation directory required")?,
    );
    if !directory.is_absolute() || !directory.is_dir() {
        return Err("absolute existing preparation directory required".into());
    }
    let success: Json = serde_json::from_slice(&fs::read(directory.join(SUCCESS))?)?;
    if success != binding(&directory)? {
        return Err("stale or altered source/artifact/recipe preparation binding".into());
    }
    let (controls, projects, _) = corpus()?;
    let nuget_cache = std::env::var_os("NUGET_PACKAGES")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".nuget/packages"))
        })
        .filter(|path| path.is_dir())
        .and_then(|path| path.canonicalize().ok());
    if fs::read(directory.join("controls.json"))? != serde_json::to_vec_pretty(&controls)? {
        return Err("frozen controls differ".into());
    }
    for (index, project) in projects.iter().enumerate() {
        if fs::read(directory.join(format!("model{index}-project.json")))?
            != mapping::project_file::encode_pretty(project)?.as_bytes()
        {
            return Err("genuine prepared Project differs".into());
        }
    }
    let evidence = Evidence(directory.clone());
    // This is the only native-execution stage. A mismatch never supplies an
    // expected tree and does not suppress the other languages' original results.
    let native = native::run(&controls, &projects, &evidence)?;
    let legacy = legacy_native(&evidence)?;
    evidence.write(
        "ALL-COMPLETE-NATIVE-COMPARISONS.json",
        &serde_json::to_vec_pretty(&native)?,
    )?;
    evidence.write(
        "ALL-COMPLETE-LEGACY-NATIVE-COMPARISONS.json",
        &serde_json::to_vec_pretty(&legacy)?,
    )?;
    let mut statuses = Vec::new();
    let mut rust = Command::new("cargo");
    rust.args(["run", "--quiet", "--jobs", "1", "--manifest-path"])
        .arg(directory.join("rust-host/Cargo.toml"))
        .arg("--")
        .arg(directory.join("controls.json"))
        .arg(directory.join("rust-actual"))
        .current_dir(&directory)
        .env("RUSTFLAGS", "-D warnings");
    rust.env("RUST_BACKTRACE", "1");
    let mut rust_sources = vec![
        PathBuf::from("rust-host"),
        PathBuf::from("rust-legacy"),
        PathBuf::from("controls.json"),
        PathBuf::from(SUCCESS),
    ];
    rust_sources.extend((0..MODELS).map(|index| PathBuf::from(format!("rust-model{index}"))));
    let output = host_policy::recorded_output_with_sources(
        &mut rust,
        &directory,
        "group441-rust-host",
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
            .arg(directory.join(host).join("Host.csproj"))
            .current_dir(&directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let built = host_policy::recorded_output_with_sources(
            &mut build,
            &directory,
            &format!("group441-csharp-{label}-build"),
            &sources,
        )?;
        statuses.push((format!("csharp-{label}-build"), built.status.success()));
        if !built.status.success() {
            continue;
        }
        // Bind the actual executable, generated dependency, and execution
        // descriptors; source snapshots alone do not identify built DLLs.
        let binaries = hashes(artifact_files(&artifacts.join("bin"))?);
        evidence.write(
            &format!("csharp-{label}-PRE-RUN-BINARIES.json"),
            &serde_json::to_vec_pretty(&binaries)?,
        )?;
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
            &format!("group441-csharp-{label}-host"),
            &sources,
        )?;
        let after = hashes(artifact_files(&artifacts.join("bin"))?);
        evidence.write(
            &format!("csharp-{label}-POST-RUN-BINARIES.json"),
            &serde_json::to_vec_pretty(&after)?,
        )?;
        statuses.push((
            format!("csharp-{label}-host"),
            output.status.success() && binaries == after,
        ));
    }
    evidence.debug("ALL-GENERATED-EXECUTION-STATUSES.txt", &statuses)?;
    verify_original_evidence(&directory, &success["evidence"])?;
    if success["sources"] != serde_json::to_value(source_bindings()?)? {
        return Err("sources changed during qualification; originals retained".into());
    }
    if native.len() != cases::CALL_COUNT
        || legacy.len() != LEGACY_CALLS
        || native
            .iter()
            .chain(&legacy)
            .any(|check| check["matched"] != true)
        || statuses.len() != 1 + 2 * (MODELS + 1)
        || statuses.iter().any(|(_, matched)| !*matched)
    {
        return Err(
            "group441 same-Project qualification differs; complete originals retained".into(),
        );
    }
    Ok(())
}
