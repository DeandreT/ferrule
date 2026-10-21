//! #443 owned `group_starting_with` parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expected results.
use super::*;
use serde_json::{Value as Json, json};
use std::fs;
// These modules are the conformance corpus. Keep their bodies byte-identical to
// the authored files when formatting this harness.
#[rustfmt::skip]
#[allow(clippy::possible_missing_else, clippy::too_many_arguments)]
#[path = "../../../../conformance/basic-owned-group-starting-with/cases.rs"]
mod cases;
#[rustfmt::skip]
#[allow(clippy::possible_missing_else)]
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
// Reuse the #473 codec and native runner rather than a second implementation.
use super::basic_owned_sequence_item_at::{codec, run_native_with_input_after as run_native};
const MODELS: usize = 105;
const CASES: usize = 109;
const CORE_CALLS: usize = 381;
const LEGACY_CALLS: usize = 2;
const CLOSED_PROJECTS: usize = 65;
const DIRECT_PROGRAMS: usize = 61;
const OLD_SURFACES: usize = 50;
struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
    native: bool,
}

// This selector list is independent of the lowered Programs and catalogues.
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
    use std::collections::BTreeSet;
    let authored: BTreeMap<&str, usize> =
        include_str!("basic_owned_group_starting_with/MODEL-SELECTOR-INVENTORY.tsv")
            .lines()
            .skip(1)
            .map(|line| {
                let fields: Vec<_> = line.split('\t').collect();
                (fields[0], fields[1].parse().expect("authored case count"))
            })
            .collect();
    let declared: BTreeSet<_> = projects::PROFILES.iter().copied().collect();
    if authored.len() != MODELS
        || declared.len() != MODELS
        || projects::PROFILES.len() != MODELS
        || authored.keys().copied().collect::<BTreeSet<_>>() != declared
    {
        return Err("105 unique profile keys differ from the selector inventory".into());
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
                return Err(
                    "E22 must keep absent inputs, an unknown target and invalid limits".into(),
                );
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
            return Err(format!("selectors changed: {}", case.id).into());
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
                    return Err(format!("named targets differ: {profile}").into());
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
        return Err("105 profiles/109 cases/381 calls or selector distribution changed".into());
    }
    Ok((
        json!({"version":1,"cases":records,"models":profiles}),
        models,
        profiles,
    ))
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
fn host_project(model: &str) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
fn replace_once(source: String, needle: &str, replacement: &str) -> TestResult<String> {
    if source.matches(needle).count() != 1 {
        return Err(format!("host template seam changed: {needle}").into());
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
fn legacy_project() -> TestResult<Project> {
    Ok(mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-starting-with/legacy-project.json"
    ))?)
}
fn legacy_native_controls() -> TestResult<(Json, Project)> {
    let source: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-starting-with/legacy-input.tree.json"
    ))?;
    let expected: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-group-starting-with/legacy-expected.tree.json"
    ))?;
    // Both All and Primary compare against the authored tree, not ordinary output.
    let expected = json!({"Ok":[[null,expected]]});
    Ok((
        json!({"version":1,"cases":[{"id":"L01-ordinary-starting","model":0,"native":true,
        "source":source,"inputs":[],"calls":[{"selection":"All","expected":expected},
        {"selection":"Primary","expected":expected}]}]}),
        legacy_project()?,
    ))
}
fn closed_projects() -> Vec<(String, Project, Vec<codegen::Diagnostic>)> {
    closed::projects()
        .into_iter()
        .chain(closed_specialized::projects())
        .map(|(id, p, e)| (id.into(), p, e))
        .chain(closed_matrix::projects())
        .collect()
}
fn closed_programs(
    base: &codegen::Program,
    specialized: &codegen::Program,
) -> Vec<(String, codegen::Program, codegen::ProgramValidationError)> {
    closed::programs(base)
        .into_iter()
        .chain(closed_specialized::programs(specialized))
        .map(|(id, p, e)| (id.into(), p, e))
        .chain(closed_matrix::programs(base))
        .collect()
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

#[test]
fn basic_starting443_corpus_codecs_and_host_templates() -> TestResult<()> {
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
    assert_eq!(
        (0..MODELS)
            .map(|model| model_calls(&controls, model))
            .sum::<usize>(),
        CORE_CALLS
    );
    assert!(rust_host("dispatch", CORE_CALLS)?.contains("call[\"invalid_limits\"]"));
    assert!(csharp_host(1)?.contains("roots = (ulong)limits.MaxRoots"));
    Ok(())
}

#[test]
fn basic_starting443_public_profiles_lower_validate_and_emit() -> TestResult<()> {
    let (_, models, profiles) = corpus()?;
    let runtime = runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let profile = &profiles[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        let before = mapping::project_file::encode_pretty(project)?;
        let issues = engine::validate(project);
        assert!(issues.is_empty(), "{profile}: {issues:?}");
        let program = codegen::lower_with_basic_owned_group_starting_with(project)
            .map_err(|error| context(&error))?;
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{profile}"
        );
        let original = program.clone();
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
            &program,
        )
        .map_err(|error| context(&error))?;
        assert_eq!(
            actual_catalogues(&plan),
            catalogues::expected(profile),
            "{profile}"
        );
        codegen::validate_program_with_basic_owned_group_starting_with(&program)
            .map_err(|error| context(&error))?;
        codegen::validate_basic_owned_group_starting_with_emission(&program)
            .map_err(|error| context(&error))?;
        codegen_rust::emit_with_basic_owned_group_starting_with(
            &program,
            &rust_options(&format!("starting443-model{index}"), &runtime),
        )
        .map_err(|error| context(&error))?;
        codegen_csharp::emit_with_basic_owned_group_starting_with(&program)
            .map_err(|error| context(&error))?;
        assert_eq!(program, original, "{profile}");
    }
    Ok(())
}

#[test]
fn basic_starting443_closed_projects_programs_and_old_profiles_refuse() -> TestResult<()> {
    let projects = closed_projects();
    assert_eq!(projects.len(), CLOSED_PROJECTS);
    assert_eq!(
        projects
            .iter()
            .map(|(id, _, _)| id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        CLOSED_PROJECTS
    );
    for (id, project, expected) in &projects {
        let before = mapping::project_file::encode_pretty(project)?;
        let actual = codegen::lower_with_basic_owned_group_starting_with(project);
        assert!(
            matches!(&actual, Err(error) if error.diagnostics() == expected.as_slice()),
            "{id}: {actual:?}"
        );
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{id}"
        );
    }

    let base = codegen::lower_with_basic_owned_group_starting_with(&projects::base())?;
    let specialized =
        codegen::lower_with_basic_owned_group_starting_with(&projects::refusal_base())?;
    let programs = closed_programs(&base, &specialized);
    assert_eq!(programs.len(), DIRECT_PROGRAMS);
    assert_eq!(
        programs
            .iter()
            .map(|(id, _, _)| id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        DIRECT_PROGRAMS
    );
    let options = rust_options("starting443-direct-refusal", &runtime_path()?);
    for (id, program, expected) in &programs {
        let before = program.clone();
        assert!(
            matches!(codegen::validate_program_with_basic_owned_group_starting_with(program),
                Err(error) if error == *expected),
            "{id} validator"
        );
        assert!(
            matches!(codegen::validate_basic_owned_group_starting_with_emission(program),
                Err(error) if error == *expected),
            "{id} emission validator"
        );
        assert!(
            matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(program),
                Err(error) if error == *expected),
            "{id} catalogue"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_group_starting_with(program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_group_starting_with(program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
            "{id} C#"
        );
        assert_eq!(program, &before, "{id}");
    }

    // Every earlier generation profile keeps refusing owned Starting.
    let s01 = projects::project("S01");
    let program = codegen::lower_with_basic_owned_group_starting_with(&s01)?;
    let expected = closed::grouping_error();
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = &expected else {
        unreachable!("authored Grouping refusal");
    };
    let diagnostic = codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    };
    let options = rust_options("starting443-old-gates", &runtime_path()?);
    let products = old_profiles::products(&s01, &program, &options);
    assert_eq!(
        [
            products.lowering.len(),
            products.validators.len(),
            products.catalogues.len(),
            products.rust.len(),
            products.csharp.len()
        ],
        [8, 16, 8, 9, 9]
    );
    assert_eq!(8 + 16 + 8 + 9 + 9, OLD_SURFACES);
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
fn basic_starting443_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models, _) = corpus()?;
    let (legacy, project) = legacy_native_controls()?;
    let temp = TempDir::new("starting443_native")?;
    let native = run_native(&controls, &models, &temp.0.join("native"))?;
    let ordinary = run_native(&legacy, &[project], &temp.0.join("legacy"))?;
    assert_eq!(native.len(), CORE_CALLS);
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
fn basic_starting443_legacy_saved_project_roundtrips_and_emits() -> TestResult<()> {
    let original =
        include_str!("../../../../conformance/basic-owned-group-starting-with/legacy-project.json");
    let project = legacy_project()?;
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert_eq!(wire, original);
    assert!(!wire.contains("owned_source"));
    let program = codegen::lower(&project)?;
    let options = rust_options("ferrule-generated-mapping", &runtime_path()?);
    codegen_rust::emit(&program, &options)?;
    codegen_csharp::emit(&program)?;
    codegen_rust::emit_with_basic_owned_group_starting_with(&program, &options)?;
    codegen_csharp::emit_with_basic_owned_group_starting_with(&program)?;
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
    let mut contract = json!({"version":2,"engine":engine,"run_id":format!("starting443-{label}"),
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
#[ignore = "builds and runs one generated Rust host and 106 generated C# hosts; requires cargo and the .NET 10 SDK"]
fn basic_starting443_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models, _) = corpus()?;
    let (legacy_controls, legacy) = legacy_native_controls()?;
    let runtime = runtime_path()?;
    let temp = TempDir::new("starting443_hosts")?;
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
        let program = codegen::lower_with_basic_owned_group_starting_with(project)?;
        let options = rust_options(&format!("starting443-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_group_starting_with(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_group_starting_with(&program)?,
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
            "model{index}={{package=\"starting443-model{index}\",path=\"../rust-model{index}\"}}\n"
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
                .join("../../conformance/basic-owned-group-starting-with")
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
        include_bytes!("basic_owned_group_starting_with/legacy_control.rs.txt"),
    )?;
    write_file(
        &root.join("csharp-legacy-host/Host.csproj"),
        host_project("legacy").as_bytes(),
    )?;
    write_file(
        &root.join("csharp-legacy-host/Program.cs"),
        format!(
            "{}\n{}",
            include_str!("basic_owned_group_starting_with/LegacyHost.cs.txt"),
            include_str!("basic_owned_group_starting_with/HostProof.cs.txt")
        )
        .as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/main.rs"),
        rust_host(&dispatch, CORE_CALLS)?.as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/codec.rs"),
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    write_file(
        &root.join("rust-host/src/proof.rs"),
        include_bytes!("basic_owned_group_starting_with/HostProof.rs.txt"),
    )?;
    write_file(&root.join("rust-host/Cargo.toml"),format!("[package]\nname=\"starting443-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\nlegacy0={{package=\"ferrule-generated-mapping\",path=\"../rust-legacy\"}}\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;

    let mut failures = Vec::new();

    // One Rust host links every model and also runs the ordinary saved project.
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
        let executable = target
            .join("debug/starting443-public-host")
            .canonicalize()?;
        let output = root.join("rust-actual");
        let contract = host_contract(&root, "rust", "rust", &executable, None, &output)?;
        let run = Command::new(&executable)
            .arg(root.join("controls.json"))
            .arg(&output)
            .arg(&contract)
            .output()?;
        if let Err(error) = host_result(&output, CORE_CALLS + LEGACY_CALLS, &run) {
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
                    if index > MODELS {
                        break;
                    }
                    if let Err(error) = run_csharp_host(&root, &dotnet, &controls, index) {
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
/// Build and run the C# host for one model, or the ordinary saved project when
/// `index == MODELS`.
fn run_csharp_host(
    root: &Path,
    dotnet: &Path,
    controls: &Json,
    index: usize,
) -> Result<(), String> {
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
        closed_projects()
            .into_iter()
            .map(|(id, project, _)| (format!("closed/{id}"), project)),
    ));
    let base = codegen::lower_with_basic_owned_group_starting_with(&projects::base())?;
    let specialized =
        codegen::lower_with_basic_owned_group_starting_with(&projects::refusal_base())?;
    corpus.extend(presets::programs(
        closed_programs(&base, &specialized)
            .into_iter()
            .map(|(id, program, _)| (format!("direct/{id}"), program)),
    ));
    Ok(corpus)
}
