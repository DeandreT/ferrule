//! #472 owned SequenceExists parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expected results.
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

#[path = "../../../../conformance/basic-owned-sequence-exists/cases.rs"]
mod cases;
#[path = "../../../../conformance/basic-owned-sequence-exists/closed.rs"]
mod closed;
#[path = "../../../../conformance/basic-owned-sequence-exists/projects.rs"]
mod projects;
// Reuse the #473 codec; the shared execution bindings launch every executor.
use super::basic_owned_sequence_item_at::codec;
use super::execution_bindings::{self as bindings, Emitters, runtime_path, rust_options};

const MODELS: usize = 22;
const CALLS: usize = 4 * MODELS;
const CLOSED_PROJECTS: usize = 9;
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
fn basic_exists472_corpus_and_codecs_roundtrip() -> TestResult<()> {
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
fn basic_exists472_profiles_lower_validate_and_emit() -> TestResult<()> {
    let (_, models) = corpus()?;
    let runtime = runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let profile = projects::PROFILES[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        let before = mapping::project_file::encode_pretty(project)?;
        let issues = engine::validate(project);
        assert!(issues.is_empty(), "{profile}: {issues:?}");
        let program =
            codegen::lower_with_basic_owned_sequence_exists(project).map_err(|e| context(&e))?;
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{profile}"
        );
        let original = program.clone();
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(&program)
                .map_err(|e| context(&e))?;
        assert!(all_optional(&plan), "{profile}: {plan:?}");
        codegen::validate_program_with_basic_owned_sequence_exists(&program)
            .map_err(|e| context(&e))?;
        codegen::validate_basic_owned_sequence_exists_emission(&program)
            .map_err(|e| context(&e))?;
        let rust = codegen_rust::emit_with_basic_owned_sequence_exists(
            &program,
            &rust_options(&format!("exists472-model{index}"), &runtime),
        )
        .map_err(|e| context(&e))?;
        let csharp = codegen_csharp::emit_with_basic_owned_sequence_exists(&program)
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
    ]
}

#[test]
fn basic_exists472_closed_projects_programs_and_old_profiles_refuse() -> TestResult<()> {
    let projects = closed::projects();
    assert_eq!(projects.len(), CLOSED_PROJECTS);
    for (id, project, expected) in &projects {
        let before = mapping::project_file::encode_pretty(project)?;
        let actual = codegen::lower_with_basic_owned_sequence_exists(project);
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
        codegen::lower_with_basic_owned_sequence_exists(&projects::project("S01-colliding"))?;
    let programs = closed::programs(&base);
    assert_eq!(programs.len(), DIRECT_PROGRAMS);
    let options = rust_options("exists472-direct-refusal", &runtime_path()?);
    for (id, program, expected) in &programs {
        let before = program.clone();
        assert_eq!(
            codegen::validate_program_with_basic_owned_sequence_exists(program).as_ref(),
            Err(expected),
            "{id} validator"
        );
        assert_eq!(
            codegen::validate_basic_owned_sequence_exists_emission(program).as_ref(),
            Err(expected),
            "{id} emission validator"
        );
        assert!(
            matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(program),
                Err(error) if error == *expected),
            "{id} catalogue"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_sequence_exists(program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_sequence_exists(program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
            "{id} C#"
        );
        assert_eq!(program, &before, "{id}");
    }

    // Every earlier generation profile keeps refusing owned SequenceExists.
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
    assert_eq!(lowering.len(), 15);
    for result in lowering {
        assert!(
            matches!(&result, Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic)),
            "{result:?}"
        );
    }
    let validators = old_validators(&base);
    assert_eq!(validators.len(), 30);
    for result in validators {
        assert_eq!(result.as_ref(), Err(&expected));
    }
    let catalogues = old_catalogues(&base);
    assert_eq!(catalogues.len(), 15);
    assert!(
        catalogues
            .iter()
            .all(|r| matches!(r, Err(e) if *e == expected))
    );
    let options = rust_options("exists472-old-gates", &runtime_path()?);
    let rust = old_rust(&base, &options);
    assert_eq!(rust.len(), 16);
    assert!(
        rust.iter().all(
            |r| matches!(r, Err(codegen_rust::EmitError::InvalidProgram(e)) if *e == expected)
        )
    );
    let csharp = old_csharp(&base);
    assert_eq!(csharp.len(), 16);
    assert!(csharp.iter().all(
        |r| matches!(r, Err(codegen_csharp::EmitError::ProgramValidation(e)) if *e == expected)
    ));
    Ok(())
}

#[test]
fn basic_exists472_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    assert_eq!(bindings::total_calls(&controls), CALLS);
    bindings::check_interpreter("exists472", &controls, &models)
}

const EMITTERS: Emitters = Emitters {
    rust: codegen_rust::emit_with_basic_owned_sequence_exists,
    csharp: codegen_csharp::emit_with_basic_owned_sequence_exists,
};

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_exists472_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    assert_eq!(bindings::total_calls(&controls), CALLS);
    let programs = models
        .iter()
        .map(codegen::lower_with_basic_owned_sequence_exists)
        .collect::<Result<Vec<_>, _>>()?;
    bindings::check_generated_hosts("exists472", &controls, &programs, EMITTERS)
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
    let base =
        codegen::lower_with_basic_owned_sequence_exists(&projects::project("S01-colliding"))?;
    corpus.extend(presets::programs(
        closed::programs(&base)
            .into_iter()
            .map(|(id, program, _)| (format!("direct/{id}"), program)),
    ));
    Ok(corpus)
}
