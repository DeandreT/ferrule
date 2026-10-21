//! #474 owned SequenceAggregate parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expected results.
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::fs;

#[path = "../../../../conformance/basic-owned-sequence-aggregate/cases.rs"]
mod cases;
#[path = "../../../../conformance/basic-owned-sequence-aggregate/closed.rs"]
mod closed;
#[path = "../../../../conformance/basic-owned-sequence-aggregate/projects.rs"]
mod projects;
// Reuse the #473 codec and native runner rather than a second implementation.
use super::basic_owned_sequence_item_at::{codec, run_native_with_input_after as run_native};

const MODELS: usize = 25;
const CALLS: usize = 4 * MODELS;
const CLOSED_PROJECTS: usize = 10;
const DIRECT_PROGRAMS: usize = 2;

struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
    native: bool,
}

fn corpus() -> TestResult<(Json, Vec<Project>)> {
    let declared: BTreeSet<_> = projects::PROFILES.iter().copied().collect();
    let cases = cases::cases();
    let ids: BTreeSet<_> = cases.iter().map(|case| case.id).collect();
    if declared.len() != MODELS || ids != declared || cases.len() != MODELS {
        return Err("profiles and cases must correspond one to one".into());
    }
    let mut models = Vec::new();
    let mut records = Vec::new();
    for (model, case) in cases.iter().enumerate() {
        if case.profile != case.id || !case.native || case.calls.len() != 4 {
            return Err(format!("case shape changed: {}", case.id).into());
        }
        models.push(projects::project(case.profile));
        records.push(json!({"id":case.id,"profile":case.profile,"model":model,
            "native":case.native,"source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,value)|json!([name,codec::encode(value)]))
                .collect::<Vec<_>>(),"calls":case.calls}));
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
fn all_optional(plan: &codegen::NamedInputAdmissionPlan<'_>) -> bool {
    let optional = |catalogue: &codegen::NamedInputCatalogue<'_>| {
        catalogue
            .entries
            .iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>()
            == [projects::NAMED, projects::EXACT, projects::UNUSED]
            && catalogue
                .entries
                .iter()
                .all(|entry| entry.availability == codegen::NamedInputAvailability::Optional)
    };
    optional(&plan.all_outputs)
        && optional(&plan.primary)
        && plan.named_targets.len() == 2
        && plan
            .named_targets
            .iter()
            .all(|target| optional(&target.catalogue))
}
fn file_text(artifacts: &codegen::ArtifactSet, path: &str) -> String {
    artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == path)
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap_or_default()
}

#[test]
fn basic_aggregate474_corpus_and_codecs_roundtrip() -> TestResult<()> {
    let (controls, models) = corpus()?;
    assert_eq!(models.len(), MODELS);
    let mut calls = 0;
    for case in controls["cases"].as_array().ok_or("cases")? {
        assert_eq!(
            codec::encode(&codec::decode(&case["source"])),
            case["source"]
        );
        for input in case["inputs"].as_array().ok_or("inputs")? {
            assert_eq!(codec::encode(&codec::decode(&input[1])), input[1]);
        }
        for call in case["calls"].as_array().ok_or("calls")? {
            calls += 1;
            if let Some(outputs) = call["expected"].get("Ok") {
                for output in outputs.as_array().ok_or("outputs")? {
                    assert_eq!(codec::encode(&codec::decode(&output[1])), output[1]);
                }
            }
        }
    }
    assert_eq!(calls, CALLS);
    Ok(())
}

#[test]
fn basic_aggregate474_profiles_lower_validate_and_emit() -> TestResult<()> {
    let (_, models) = corpus()?;
    let runtime = runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let profile = projects::PROFILES[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        let before = mapping::project_file::encode_pretty(project)?;
        let issues = engine::validate(project);
        assert!(issues.is_empty(), "{profile}: {issues:?}");
        let program =
            codegen::lower_with_basic_owned_sequence_aggregate(project).map_err(|e| context(&e))?;
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{profile}"
        );
        let original = program.clone();
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(
            &program,
        )
        .map_err(|e| context(&e))?;
        assert!(all_optional(&plan), "{profile}: {plan:?}");
        codegen::validate_program_with_basic_owned_sequence_aggregate(&program)
            .map_err(|e| context(&e))?;
        codegen::validate_basic_owned_sequence_aggregate_emission(&program)
            .map_err(|e| context(&e))?;
        let rust = codegen_rust::emit_with_basic_owned_sequence_aggregate(
            &program,
            &rust_options(&format!("aggregate474-model{index}"), &runtime),
        )
        .map_err(|e| context(&e))?;
        let csharp = codegen_csharp::emit_with_basic_owned_sequence_aggregate(&program)
            .map_err(|e| context(&e))?;
        assert!(
            file_text(&rust, "src/lib.rs").contains("generated_item_contexts(&generated_items)"),
            "{profile}"
        );
        assert!(
            file_text(&csharp, "GeneratedMapping.cs").contains("EnumerateGenerated("),
            "{profile}"
        );
        assert_eq!(program, original, "{profile}");
    }
    Ok(())
}

fn old_lowering(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    vec![
        codegen::lower(project),
        codegen::lower_with_basic_owned_filters(project),
        codegen::lower_with_basic_owned_sort(project),
        codegen::lower_with_basic_owned_windows(project),
        codegen::lower_with_basic_owned_row_failures(project),
        codegen::lower_with_basic_owned_group_by(project),
        codegen::lower_with_basic_owned_sequence_item_at(project),
        codegen::lower_with_basic_owned_group_adjacent_by(project),
        codegen::lower_with_basic_owned_group_starting_with(project),
        codegen::lower_with_basic_owned_group_ending_with(project),
        codegen::lower_with_basic_owned_failure_sources(project),
        codegen::lower_with_basic_owned_path_hierarchy(project),
        codegen::lower_with_basic_owned_adjacency_tree(project),
        codegen::lower_with_basic_owned_group_into_blocks(project),
        codegen::lower_with_basic_owned_recursive_collect(project),
        codegen::lower_with_basic_owned_sequence_exists(project),
        codegen::lower_with_basic_owned_recursive_filter(project),
        codegen::lower_with_basic_owned_lookup_find(project),
        codegen::lower_with_basic_owned_dynamic_field(project),
    ]
}
fn old_validators(p: &codegen::Program) -> Vec<Result<(), codegen::ProgramValidationError>> {
    vec![
        codegen::validate_program(p),
        codegen::validate_basic_owned_emission(p),
        codegen::validate_program_with_basic_owned_filters(p),
        codegen::validate_basic_owned_filter_emission(p),
        codegen::validate_program_with_basic_owned_sort(p),
        codegen::validate_basic_owned_sort_emission(p),
        codegen::validate_program_with_basic_owned_windows(p),
        codegen::validate_basic_owned_window_emission(p),
        codegen::validate_program_with_basic_owned_row_failures(p),
        codegen::validate_basic_owned_row_failure_emission(p),
        codegen::validate_program_with_basic_owned_group_by(p),
        codegen::validate_basic_owned_group_by_emission(p),
        codegen::validate_program_with_basic_owned_sequence_item_at(p),
        codegen::validate_basic_owned_sequence_item_at_emission(p),
        codegen::validate_program_with_basic_owned_group_adjacent_by(p),
        codegen::validate_basic_owned_group_adjacent_by_emission(p),
        codegen::validate_program_with_basic_owned_group_starting_with(p),
        codegen::validate_basic_owned_group_starting_with_emission(p),
        codegen::validate_program_with_basic_owned_group_ending_with(p),
        codegen::validate_basic_owned_group_ending_with_emission(p),
        codegen::validate_program_with_basic_owned_failure_sources(p),
        codegen::validate_basic_owned_failure_source_emission(p),
        codegen::validate_program_with_basic_owned_path_hierarchy(p),
        codegen::validate_basic_owned_path_hierarchy_emission(p),
        codegen::validate_program_with_basic_owned_adjacency_tree(p),
        codegen::validate_basic_owned_adjacency_tree_emission(p),
        codegen::validate_program_with_basic_owned_group_into_blocks(p),
        codegen::validate_basic_owned_group_into_blocks_emission(p),
        codegen::validate_program_with_basic_owned_recursive_collect(p),
        codegen::validate_basic_owned_recursive_collect_emission(p),
        codegen::validate_program_with_basic_owned_sequence_exists(p),
        codegen::validate_basic_owned_sequence_exists_emission(p),
        codegen::validate_program_with_basic_owned_recursive_filter(p),
        codegen::validate_basic_owned_recursive_filter_emission(p),
        codegen::validate_program_with_basic_owned_lookup_find(p),
        codegen::validate_basic_owned_lookup_find_emission(p),
        codegen::validate_program_with_basic_owned_dynamic_field(p),
        codegen::validate_basic_owned_dynamic_field_emission(p),
    ]
}
fn old_catalogues(
    p: &codegen::Program,
) -> Vec<Result<codegen::NamedInputAdmissionPlan<'_>, codegen::ProgramValidationError>> {
    vec![
        codegen::legacy_named_input_admission_plan(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_filters(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sort(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_windows(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_row_failures(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_by(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_adjacent_by(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(p),
        codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(p),
    ]
}
fn old_rust(
    p: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
        codegen_rust::emit(p, options),
        codegen_rust::emit_with_basic_owned_sources(p, options),
        codegen_rust::emit_with_basic_owned_filters(p, options),
        codegen_rust::emit_with_basic_owned_sort(p, options),
        codegen_rust::emit_with_basic_owned_windows(p, options),
        codegen_rust::emit_with_basic_owned_row_failures(p, options),
        codegen_rust::emit_with_basic_owned_group_by(p, options),
        codegen_rust::emit_with_basic_owned_sequence_item_at(p, options),
        codegen_rust::emit_with_basic_owned_group_adjacent_by(p, options),
        codegen_rust::emit_with_basic_owned_group_starting_with(p, options),
        codegen_rust::emit_with_basic_owned_group_ending_with(p, options),
        codegen_rust::emit_with_basic_owned_failure_sources(p, options),
        codegen_rust::emit_with_basic_owned_path_hierarchy(p, options),
        codegen_rust::emit_with_basic_owned_adjacency_tree(p, options),
        codegen_rust::emit_with_basic_owned_group_into_blocks(p, options),
        codegen_rust::emit_with_basic_owned_recursive_collect(p, options),
        codegen_rust::emit_with_basic_owned_sequence_exists(p, options),
        codegen_rust::emit_with_basic_owned_recursive_filter(p, options),
        codegen_rust::emit_with_basic_owned_lookup_find(p, options),
        codegen_rust::emit_with_basic_owned_dynamic_field(p, options),
    ]
}
fn old_csharp(
    p: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
        codegen_csharp::emit(p),
        codegen_csharp::emit_with_basic_owned_sources(p),
        codegen_csharp::emit_with_basic_owned_filters(p),
        codegen_csharp::emit_with_basic_owned_sort(p),
        codegen_csharp::emit_with_basic_owned_windows(p),
        codegen_csharp::emit_with_basic_owned_row_failures(p),
        codegen_csharp::emit_with_basic_owned_group_by(p),
        codegen_csharp::emit_with_basic_owned_sequence_item_at(p),
        codegen_csharp::emit_with_basic_owned_group_adjacent_by(p),
        codegen_csharp::emit_with_basic_owned_group_starting_with(p),
        codegen_csharp::emit_with_basic_owned_group_ending_with(p),
        codegen_csharp::emit_with_basic_owned_failure_sources(p),
        codegen_csharp::emit_with_basic_owned_path_hierarchy(p),
        codegen_csharp::emit_with_basic_owned_adjacency_tree(p),
        codegen_csharp::emit_with_basic_owned_group_into_blocks(p),
        codegen_csharp::emit_with_basic_owned_recursive_collect(p),
        codegen_csharp::emit_with_basic_owned_sequence_exists(p),
        codegen_csharp::emit_with_basic_owned_recursive_filter(p),
        codegen_csharp::emit_with_basic_owned_lookup_find(p),
        codegen_csharp::emit_with_basic_owned_dynamic_field(p),
    ]
}

#[test]
fn basic_aggregate474_closed_projects_programs_and_old_profiles_refuse() -> TestResult<()> {
    let projects = closed::projects();
    assert_eq!(projects.len(), CLOSED_PROJECTS);
    for (id, project, expected) in &projects {
        let before = mapping::project_file::encode_pretty(project)?;
        let actual = codegen::lower_with_basic_owned_sequence_aggregate(project);
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

    let base =
        codegen::lower_with_basic_owned_sequence_aggregate(&projects::project("S01-colliding"))?;
    let programs = closed::programs(&base);
    assert_eq!(programs.len(), DIRECT_PROGRAMS);
    let options = rust_options("aggregate474-direct-refusal", &runtime_path()?);
    for (id, program, expected) in &programs {
        let before = program.clone();
        assert_eq!(
            codegen::validate_program_with_basic_owned_sequence_aggregate(program).as_ref(),
            Err(expected),
            "{id} validator"
        );
        assert_eq!(
            codegen::validate_basic_owned_sequence_aggregate_emission(program).as_ref(),
            Err(expected),
            "{id} emission validator"
        );
        assert!(
            matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(program),
                Err(error) if error == *expected),
            "{id} catalogue"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_sequence_aggregate(program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_sequence_aggregate(program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
            "{id} C#"
        );
        assert_eq!(program, &before, "{id}");
    }

    // Every other generation profile, including Exists, RecursiveFilter,
    // LookupFind and DynamicField, keeps refusing owned SequenceAggregate.
    let s01 = projects::project("S01-colliding");
    let expected = closed::closed_producer(
        1,
        codegen::OwnedSourceDiagnosticRequest::Field {
            owner: codegen::OwnedSourceDiagnosticOwner::Primary,
            path: projects::path(&["Code"]),
            frame: Some(vec![]),
        },
    );
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = &expected else {
        unreachable!("authored refusal");
    };
    let diagnostic = codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    };
    let lowering = old_lowering(&s01);
    assert_eq!(lowering.len(), 19);
    for result in lowering {
        assert!(
            matches!(&result, Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic)),
            "{result:?}"
        );
    }
    let validators = old_validators(&base);
    assert_eq!(validators.len(), 38);
    for result in validators {
        assert_eq!(result.as_ref(), Err(&expected));
    }
    let catalogues = old_catalogues(&base);
    assert_eq!(catalogues.len(), 19);
    assert!(
        catalogues
            .iter()
            .all(|r| matches!(r, Err(e) if *e == expected))
    );
    let options = rust_options("aggregate474-old-gates", &runtime_path()?);
    let rust = old_rust(&base, &options);
    assert_eq!(rust.len(), 20);
    assert!(
        rust.iter().all(
            |r| matches!(r, Err(codegen_rust::EmitError::InvalidProgram(e)) if *e == expected)
        )
    );
    let csharp = old_csharp(&base);
    assert_eq!(csharp.len(), 20);
    assert!(csharp.iter().all(
        |r| matches!(r, Err(codegen_csharp::EmitError::ProgramValidation(e)) if *e == expected)
    ));
    Ok(())
}

#[test]
fn basic_aggregate474_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let temp = TempDir::new("aggregate474_native")?;
    let native = run_native(&controls, &models, &temp.0.join("native"))?;
    assert_eq!(native.len(), CALLS);
    let differing: Vec<_> = native.iter().filter(|row| row["matched"] != true).collect();
    assert!(differing.is_empty(), "{differing:#?}");
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
fn rust_host(dispatch: &str) -> TestResult<String> {
    let template = include_str!("basic_owned_group_starting_with/Host.rs.txt")
        .replace("itemat473", "aggregate474");
    let template = replace_once(template, "@DISPATCH@", dispatch)?;
    replace_once(template, "@CORE_CALLS@", &CALLS.to_string())
}
/// This corpus has no ordinary saved-project control inside the host.
const NO_LEGACY_CONTROL: &str = "use serde_json::Value as Json;\nuse std::path::Path;\npub fn run(_directory:&Path,_contract:&Json)->Result<Vec<Json>,Box<dyn std::error::Error>> {Ok(Vec::new())}\n";
fn csharp_host(calls: usize) -> TestResult<String> {
    let template = include_str!("basic_owned_group_starting_with/Host.cs.txt")
        .replace("itemat473", "aggregate474")
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
    label: &str,
    engine: &str,
    program: &Path,
    assembly: Option<&Path>,
    output: &Path,
) -> TestResult<PathBuf> {
    let path = root.join(format!("run-contracts/{label}.json"));
    let mut contract = json!({"version":2,"engine":engine,"run_id":format!("aggregate474-{label}"),
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
fn model_calls(controls: &Json, model: usize) -> usize {
    controls["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|case| case["model"] == model)
        .map(|case| case["calls"].as_array().map_or(0, Vec::len))
        .sum()
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_aggregate474_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let runtime = runtime_path()?;
    let temp = TempDir::new("aggregate474_hosts")?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(&controls)?,
    )?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in models.iter().enumerate() {
        let program = codegen::lower_with_basic_owned_sequence_aggregate(project)?;
        let options = rust_options(&format!("aggregate474-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_sequence_aggregate(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_sequence_aggregate(&program)?,
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
            "model{index}={{package=\"aggregate474-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!("{index}=>invoke!(model{index},selection,&source,&inputs,&execution,limits,&directory,&prefix,call,&before,&named,&identity,&raw_before),\n"));
    }
    write_file(
        &root.join("rust-host/src/main.rs"),
        rust_host(&dispatch)?.as_bytes(),
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
    write_file(&root.join("rust-host/Cargo.toml"),format!("[package]\nname=\"aggregate474-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",serde_json::to_string(&runtime.display().to_string())?).as_bytes())?;

    let mut failures = Vec::new();
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
        let executable = target
            .join("debug/aggregate474-public-host")
            .canonicalize()?;
        let output = root.join("rust-actual");
        let contract = host_contract(&root, "rust", "rust", &executable, None, &output)?;
        let run = Command::new(&executable)
            .arg(root.join("controls.json"))
            .arg(&output)
            .arg(&contract)
            .output()?;
        if let Err(error) = host_result(&output, CALLS, &run) {
            failures.push(format!("Rust host: {error}"));
        }
    } else {
        failures.push(format!(
            "Rust host build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }

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
    let label = format!("model{index}");
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
        .arg(root.join(format!("csharp-host{index}/Host.csproj")))
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .output()
        .map_err(|error| format!("C# {label} build: {error}"))?;
    if !build.status.success() {
        return Err(failed("build", &build));
    }
    let assembly = artifacts.join("bin/Host/release/Host.dll");
    let output = root.join(format!("csharp-{label}-actual"));
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
        .arg(root.join("controls.json"))
        .arg(index.to_string())
        .arg(&output)
        .arg(&contract)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .output()
        .map_err(|error| format!("C# {label} run: {error}"))?;
    host_result(&output, model_calls(controls, index), &run)
        .map_err(|error| format!("C# {label}: {error}"))
}
