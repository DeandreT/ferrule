//! #443 prospective public-Project three-engine campaign. SOURCE ONLY / UNRUN.
//! Production companions and independent candidate-delta acceptance are prerequisites.
//! Preparation performs no mapping; execution is a distinct coordinator-owned lane.
use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest as _, Sha256};
use std::{fs, io::Write as _};
// These modules are byte-bound independent corpus inputs. Keep their mounted
// bodies identical to the archived originals when formatting this harness.
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/cases.rs"]
mod cases;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/projects.rs"]
mod projects;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/catalogues.rs"]
mod catalogues;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/closed.rs"]
mod closed;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/closed_specialized.rs"]
mod closed_specialized;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/closed_matrix.rs"]
mod closed_matrix;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-group-starting-with/old_profiles.rs"]
mod old_profiles;
// Borrow the qualified #473 implementations. No second codec/native algorithm.
use super::basic_owned_sequence_item_at::{codec, run_native_with_input_after as run_native};
const ENVIRONMENT: &str = "FERRULE_BASIC_STARTING443_PREPARE_DIR";
const SUCCESS: &str = "SUCCESSFUL-COMPILER-FREE-PREPARATION.json";
const MODELS: usize = 105;
const CASES: usize = 109;
const CORE_CALLS: usize = 381;
const LEGACY_CALLS: usize = 2;
const CLOSED_PROJECTS: usize = 65;
const DIRECT_PROGRAMS: usize = 61;
const OLD_SURFACES: usize = 50;
const _: () = assert!(CORE_CALLS + LEGACY_CALLS == 383);
const _: () = assert!(3 * (CORE_CALLS + LEGACY_CALLS) == 1149);
struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
    native: bool,
}
include!("basic_owned_group_starting_with/mechanics.rs");
include!("basic_owned_group_starting_with/historical.rs");
include!("basic_owned_group_starting_with/refusals.rs");
include!("basic_owned_group_starting_with/preparation.rs");
include!("basic_owned_group_starting_with/execution.rs");
include!("basic_owned_group_starting_with/host_evidence.rs");
include!("basic_owned_group_starting_with/routes.rs");
include!("basic_owned_group_starting_with/prepared_inputs.rs");
include!("basic_owned_group_starting_with/prepared_input_controls.rs");
include!("basic_owned_group_starting_with/review_controls.rs");

// This independent selector list is not computed from candidate Programs,
// catalogue output, a target count assumption or cases::standalone().
const STANDALONE_PROFILES: &[&str] = &[
    "S23-boundary",
    "S23-post",
    "S23-ordinary",
    "X07-P",
    "X07-N",
    "X08-P",
    "X08-N",
    "X08-Alias",
    "X09-P-true",
    "X09-P-false",
    "X09-N-true",
    "X09-N-false",
    "X09-Alias-true",
    "X09-Alias-false",
    "X10-ordinary",
    "X11",
    "X14-scalar",
    "X14-members",
    "E17",
    "E18-window",
    "E18-post",
    "E18-reached",
    "I07-producer",
    "I07-empty",
    "I07-null",
];
fn corpus() -> TestResult<(Json, Vec<Project>, Vec<String>)> {
    use std::collections::{BTreeMap, BTreeSet};
    let authored: BTreeMap<&str, usize> =
        include_str!("basic_owned_group_starting_with/MODEL-SELECTOR-INVENTORY.tsv")
            .lines()
            .skip(1)
            .map(|line| {
                let fields: Vec<_> = line.split('\t').collect();
                (fields[0], fields[1].parse().expect("frozen source count"))
            })
            .collect();
    let declared: BTreeSet<_> = projects::PROFILES.iter().copied().collect();
    if authored.len() != MODELS
        || declared.len() != MODELS
        || projects::PROFILES.len() != MODELS
        || authored.keys().copied().collect::<BTreeSet<_>>() != declared
    {
        return Err("105 exact unique profile keys differ from frozen inventory".into());
    }
    let cases = cases::cases();
    let mut ids = BTreeSet::new();
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut profiles = Vec::<String>::new();
    let mut models = Vec::new();
    let mut records = Vec::new();
    let mut arities = BTreeMap::<usize, usize>::new();
    for case in &cases {
        if !ids.insert(case.id) || !case.native {
            return Err("duplicate ID or skipped native case".into());
        }
        let profile = match case.id {
            "S20-empty" => "S20",
            "X10-grouped-shadow" => "X08-P",
            "C01-absent-optional" | "E22-target-before-input-and-policy" => "S01",
            id => id,
        };
        if case.profile != profile || !declared.contains(profile) {
            return Err(format!("case/profile binding differs: {}", case.id).into());
        }
        let selections: Vec<_> = case
            .calls
            .iter()
            .map(|call| call["selection"].as_str())
            .collect();
        let expected: Vec<_> = if case.id == "E22-target-before-input-and-policy" {
            if !case.inputs.is_empty()
                || case.calls
                    != vec![json!({"selection":" audit ","invalid_limits":true,
                "expected":{"Err":{"kind":"UnknownTarget","name":" audit "}}})]
            {
                return Err("E22 must retain absent inputs, exact unknown target and invalid five-field policy".into());
            }
            vec![Some(" audit ")]
        } else if STANDALONE_PROFILES.contains(&profile) {
            vec![Some("All"), Some("Primary")]
        } else {
            vec![Some("All"), Some("Primary"), Some("audit"), Some("unicode")]
        };
        if selections != expected
            || (case.id != "E22-target-before-input-and-policy"
                && case
                    .calls
                    .iter()
                    .any(|call| call.get("invalid_limits").is_some()))
        {
            return Err(format!("exact selectors changed: {}", case.id).into());
        }
        *arities.entry(case.calls.len()).or_default() += 1;
        *counts.entry(profile).or_default() += 1;
        let model = match profiles.iter().position(|p| p == profile) {
            Some(i) => i,
            None => {
                let project = projects::project(profile);
                let names: Vec<_> = project
                    .extra_targets
                    .iter()
                    .map(|target| target.name.as_str())
                    .collect();
                let expected_names = if STANDALONE_PROFILES.contains(&profile) {
                    vec![]
                } else {
                    vec!["audit", "unicode"]
                };
                if names != expected_names {
                    return Err(format!("actual named targets differ: {profile}").into());
                }
                profiles.push(profile.into());
                models.push(project);
                profiles.len() - 1
            }
        };
        records.push(json!({"id":case.id,"profile":case.profile,"model":model,"native":case.native,
            "source":codec::encode(&case.source),"inputs":case.inputs.iter()
                .map(|(name,value)|json!([name,codec::encode(value)])).collect::<Vec<_>>(),"calls":case.calls}));
    }
    let expected_ids: BTreeSet<_> = declared
        .into_iter()
        .chain([
            "S20-empty",
            "X10-grouped-shadow",
            "C01-absent-optional",
            "E22-target-before-input-and-policy",
        ])
        .collect();
    let calls: usize = cases.iter().map(|case| case.calls.len()).sum();
    if ids != expected_ids
        || counts != authored
        || models.len() != MODELS
        || cases.len() != CASES
        || calls != CORE_CALLS
        || arities != BTreeMap::from([(1, 1), (2, 26), (4, 82)])
    {
        return Err(
            "literal 105 profiles/109 cases/381 calls or selector distribution changed".into(),
        );
    }
    Ok((
        json!({"version":1,"cases":records,"models":profiles,"case_count":CASES,
        "core_calls_per_engine":CORE_CALLS,"legacy_calls_per_engine":LEGACY_CALLS,
        "calls_per_engine":383,"prospective_three_engine_calls":1149}),
        models,
        profiles,
    ))
}
fn binding(directory: &Path) -> TestResult<Json> {
    let files = artifact_files(directory)?
        .into_iter()
        .filter(|(path, _)| path != SUCCESS)
        .collect();
    let (controls, _, _) = corpus()?;
    Ok(
        json!({"version":1,"result":"success","cases":controls["case_count"],
        "core_calls_per_engine":CORE_CALLS,"legacy_calls_per_engine":LEGACY_CALLS,
        "calls_per_engine":383,"prospective_three_engine_calls":1149,
        "models":MODELS,"closed_projects":CLOSED_PROJECTS,"direct_programs":DIRECT_PROGRAMS,
        "old_surfaces":OLD_SURFACES,"sources":source_bindings()?,"evidence":hashes(files),
        "commands":command_runtime_binding()?,"runtime":runtime_path()?}),
    )
}
fn runtime_path() -> TestResult<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?)
}
fn verify_original_evidence(directory: &Path, rows: &Json) -> TestResult<()> {
    let rows: Vec<legacy_artifact_policy::ManifestRow> = serde_json::from_value(rows.clone())?;
    for (relative, length, sha) in rows {
        codegen::ArtifactPath::new(relative.as_str())?;
        let bytes = fs::read(directory.join(&relative))?;
        if bytes.len() != length || digest(&bytes) != sha {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("prepared original changed: {relative}"),
            )
            .into());
        }
    }
    Ok(())
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
fn host_project(model: &str) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
fn replace_once(source: String, needle: &str, replacement: &str) -> TestResult<String> {
    if source.matches(needle).count() != 1 {
        return Err("reviewed #442 host seam drift".into());
    }
    Ok(source.replace(needle, replacement))
}
fn rust_host(dispatch: &str, calls: usize) -> TestResult<String> {
    let template = include_str!("basic_owned_group_starting_with/Host.rs.txt")
        .replace("itemat473", "starting443");
    let template = replace_once(template, "@DISPATCH@", dispatch)?;
    let template = replace_once(template, "@CORE_CALLS@", &calls.to_string())?;
    replace_once(
        template,
        "let selection=call[\"selection\"].as_str().unwrap();let prefix=",
        "let limits=if call[\"invalid_limits\"]==true {OwnedSourceLimits {max_roots:0,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0}} else {limits};\n            let selection=call[\"selection\"].as_str().unwrap();let prefix=",
    )
}
fn csharp_host(calls: usize) -> TestResult<String> {
    let template = include_str!("basic_owned_group_starting_with/Host.cs.txt")
        .replace("itemat473", "starting443")
        + "\n"
        + include_str!("basic_owned_group_starting_with/HostProof.cs.txt");
    let template = replace_once(template, "@MODEL_CALLS@", &calls.to_string())?;
    let template = replace_once(
        template,
        "var limits = new FerruleOwnedSourceLimits",
        "var defaultLimits = new FerruleOwnedSourceLimits",
    )?;
    let template = replace_once(
        template,
        "var call = callNode!;",
        "var call = callNode!; var limits = call[\"invalid_limits\"]?.GetValue<bool>() == true ? new FerruleOwnedSourceLimits(0, 0, 0, 0, 0) : defaultLimits;",
    )?;
    replace_once(
        template,
        "limits = new { roots = 16, frames = 32, path = 64, bytes = 16777216, walk = 512 }",
        "limits = new { roots = (ulong)limits.MaxRoots, frames = (ulong)limits.MaxActiveFrames, path = (ulong)limits.MaxPathSegments, bytes = (ulong)limits.MaxMetadataBytes, walk = (ulong)limits.MaxWalkContexts }",
    )
}

/// The maintained raw-first observer owns original result/error/panic capture.
/// A success clone is only for later API calls; no original product is mutated.
/// Observation failures become false; required evidence I/O propagates unchanged.
fn recorded<T, E>(
    evidence: &Evidence,
    label: &str,
    invoke: impl FnOnce() -> Result<T, E>,
    capture: impl FnOnce(&T) -> Json,
    observe: impl FnOnce(&Result<T, E>) -> bool,
) -> TestResult<(Option<T>, bool)>
where
    T: std::fmt::Debug + Clone,
    E: std::error::Error + 'static,
{
    let mut success = None;
    let invocation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(invoke));
    if let Ok(Err(error)) = &invocation {
        evidence.error(&format!("{label}-COMPLETE-ERROR-CHAIN.txt"), error)?;
    }
    let observed = codec::retain_outcome(
        invocation,
        |suffix, bytes| evidence.write_io(&format!("{label}-{suffix}"), bytes),
        capture,
        |product| {
            success = product.as_ref().ok().cloned();
            json!({"matched":observe(product)})
        },
    )?;
    Ok((success, observed.is_some_and(|row| row["matched"] == true)))
}
fn artifact_capture(artifacts: &codegen::ArtifactSet) -> Json {
    // Full bytes, not a digest-only or Debug-only substitute for unexpected success.
    json!(artifacts.files().iter().map(|file| json!({"path":file.path.as_str(),
        "bytes":file.contents,"row":legacy_artifact_policy::row(file.path.as_str(),&file.contents)})).collect::<Vec<_>>())
}
fn debug_capture<T: std::fmt::Debug>(value: &T) -> Json {
    json!({"complete_debug":format!("{value:#?}")})
}
fn input_after(
    evidence: &Evidence,
    label: &str,
    program: &codegen::Program,
    before: &codegen::Program,
) -> TestResult<bool> {
    evidence.debug(&format!("{label}-COMPLETE-INPUT-AFTER.txt"), program)?;
    Ok(program == before)
}

fn legacy_native_controls() -> TestResult<(Json, Project)> {
    let source: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-starting-with/legacy-input.tree.json"
    ))?;
    let expected: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-starting-with/legacy-expected.tree.json"
    ))?;
    let project = mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-starting-with/legacy-project.json"
    ))?;
    // Both All/Primary read the independent lossless literal, not ordinary output.
    let expected = json!({"Ok":[[null,expected]]});
    Ok((
        json!({"version":1,"cases":[{"id":"L01-ordinary-starting","model":0,"native":true,
        "source":source,"inputs":[],"calls":[{"selection":"All","expected":expected},
        {"selection":"Primary","expected":expected}]}]}),
        project,
    ))
}
fn legacy_native(evidence: &Evidence) -> TestResult<Vec<Json>> {
    let (controls, project) = legacy_native_controls()?;
    if fs::read(
        evidence
            .0
            .join("legacy-native-COMPLETE-AUTHORED-CONTROLS.json"),
    )? != serde_json::to_vec_pretty(&controls)?
    {
        return Err("frozen ordinary controls differ".into());
    }
    run_native(&controls, &[project], &evidence.0.join("legacy-control"))
}

#[test]
fn basic_starting443_literal_inventory_and_host_seams() -> TestResult<()> {
    let (controls, models, _) = corpus()?;
    assert_eq!(models.len(), MODELS);
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
    assert!(rust_host("dispatch", CORE_CALLS)?.contains("call[\"invalid_limits\"]"));
    assert!(csharp_host(1)?.contains("roots = (ulong)limits.MaxRoots"));
    let mut selectors = BTreeMap::from([
        ("RUSTC", None),
        ("RUSTC_WRAPPER", None),
        ("RUSTC_WORKSPACE_WRAPPER", None),
    ]);
    reject_compiler_overrides(&selectors)?;
    for name in ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        for value in ["", "rustc", "/external/compiler-or-wrapper"] {
            selectors.insert(name, Some(value.into()));
            assert!(reject_compiler_overrides(&selectors).is_err());
        }
        selectors.insert(name, None);
    }
    Ok(())
}

#[test]
fn basic_starting443_raw_first_continuation_and_fatal_evidence_io() -> TestResult<()> {
    let temp = TempDir::new("starting443-raw-first")?;
    let evidence = Evidence(temp.0.clone());
    let raw = Instance::MappedSequence(vec![Instance::Scalar(Value::Float(f64::from_bits(
        0x7ff8000000000042,
    )))]);
    let (_, mismatch) = recorded(
        &evidence,
        "mismatch",
        || Ok::<_, io::Error>(raw),
        codec::raw_tree,
        |_| false,
    )?;
    let (_, observer) = recorded(
        &evidence,
        "observer",
        || Ok::<_, io::Error>(Instance::Scalar(Value::Int(1))),
        codec::raw_tree,
        |_| panic!("authored observation failure"),
    )?;
    let (_, invocation) = recorded::<Instance, io::Error>(
        &evidence,
        "invocation",
        || panic!("authored invocation failure"),
        codec::raw_tree,
        |_| true,
    )?;
    let (_, later) = recorded(
        &evidence,
        "later",
        || Ok::<_, io::Error>(Instance::Scalar(Value::Int(2))),
        codec::raw_tree,
        |actual| matches!(actual, Ok(Instance::Scalar(Value::Int(2)))),
    )?;
    assert_eq!(
        [mismatch, observer, invocation, later],
        [false, false, false, true]
    );
    let retained = fs::read_to_string(temp.0.join("mismatch-COMPLETE-RAW-PRODUCT.json"))?;
    assert!(retained.contains("MappedSequence") && retained.contains("7ff8000000000042"));
    assert!(
        temp.0
            .join("observer-COMPLETE-OBSERVATION-FAILURE.txt")
            .is_file()
    );
    assert!(
        temp.0
            .join("invocation-COMPLETE-INVOCATION-PANIC.txt")
            .is_file()
    );
    let observed = std::cell::Cell::new(false);
    let failure = recorded(
        &evidence,
        "later",
        || Ok::<_, io::Error>(Instance::Scalar(Value::Int(3))),
        codec::raw_tree,
        |_| {
            observed.set(true);
            true
        },
    );
    assert_eq!(
        failure
            .unwrap_err()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(
        !observed.get(),
        "required raw evidence failure must never become a comparison result"
    );
    Ok(())
}
