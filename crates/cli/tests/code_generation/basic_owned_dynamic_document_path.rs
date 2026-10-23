//! #994 owned document paths of dynamically loaded documents: the
//! interpreter, generated Rust and generated C# run the portable conformance
//! cases with a fresh recording loader per call, comparing outcomes,
//! attempted callbacks and physical reads.
use super::*;
use serde_json::{Value as Json, json};

#[path = "../../../../conformance/basic-owned-dynamic-document-path/projects.rs"]
mod projects;
use super::basic_owned_dynamic_loading::{
    LoaderEmitters, check_generated_hosts, check_interpreter,
};
use super::basic_owned_sequence_item_at::codec;
use super::owned_capability_presets::{self as presets, Corpus};

const TAG: &str = "dyndocpath994";
const CASES: &str =
    include_str!("../../../../conformance/basic-owned-dynamic-document-path/cases.json");
const CALLS: usize = 22;
const LOWER: fn(&Project) -> Result<codegen::Program, codegen::LowerError> =
    codegen::lower_with_basic_owned_dynamic_document_paths;
const EMITTERS: LoaderEmitters = LoaderEmitters {
    rust: codegen_rust::emit_with_basic_owned_dynamic_document_paths,
    csharp: codegen_csharp::emit_with_basic_owned_dynamic_document_paths,
};

fn models() -> Vec<Project> {
    projects::MODELS
        .iter()
        .map(|model| projects::project(model))
        .collect()
}

/// The portable cases with each model name resolved to its host index.
fn controls() -> TestResult<Json> {
    let mut controls: Json = serde_json::from_str(CASES)?;
    if controls["version"] != 1 {
        return Err("unknown cases version".into());
    }
    for case in controls["cases"].as_array_mut().ok_or("cases")? {
        let model = projects::MODELS
            .iter()
            .position(|model| case["model"] == *model)
            .ok_or_else(|| format!("{}: unknown model", case["id"]))?;
        case["model"] = json!(model);
    }
    Ok(controls)
}

pub(super) fn capability_corpus() -> Corpus {
    presets::projects(
        projects::MODELS
            .iter()
            .map(|model| (*model, projects::project(model))),
    )
}

fn round_trips(tree: &Json) -> bool {
    codec::encode(&codec::decode(tree)) == *tree
}

fn capability_reason(
    error: &codegen::ProgramValidationError,
) -> codegen::OwnedSourceCapabilityReason {
    match error {
        codegen::ProgramValidationError::OwnedSource { diagnostic } => match diagnostic.as_ref() {
            codegen::OwnedSourceDiagnostic::Capability { reason, .. } => *reason,
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn basic_dyndocpath994_cases_cover_every_model() -> TestResult<()> {
    let controls = controls()?;
    for document in controls["documents"].as_array().ok_or("documents")? {
        assert!(round_trips(&document[2]), "{document}");
    }
    let mut ids = std::collections::BTreeSet::new();
    let cases = controls["cases"].as_array().ok_or("cases")?;
    for case in cases {
        assert!(
            ids.insert(case["id"].as_str().ok_or("id")?),
            "{}",
            case["id"]
        );
        assert!(round_trips(&case["source"]), "{}", case["id"]);
        for input in case["inputs"].as_array().ok_or("inputs")? {
            assert_eq!(input[0], projects::CATALOG, "{}", case["id"]);
            assert!(round_trips(&input[1]), "{}", case["id"]);
        }
        let calls = case["calls"].as_array().ok_or("calls")?;
        assert_eq!(calls.len(), 2, "{}", case["id"]);
        for call in calls {
            assert!(call["calls"].is_array() && call["reads"].is_array());
            match (call["expected"].get("Ok"), call["expected"].get("Err")) {
                (Some(outputs), None) => {
                    for output in outputs.as_array().ok_or("outputs")? {
                        assert!(round_trips(&output[1]), "{}", case["id"]);
                    }
                }
                (None, Some(error)) => assert!(error["kind"].is_string()),
                _ => panic!("{}: one expected outcome per call", case["id"]),
            }
        }
    }
    for model in 0..projects::MODELS.len() {
        assert!(cases.iter().any(|case| case["model"] == model), "{model}");
    }
    let calls: usize = cases
        .iter()
        .map(|case| case["calls"].as_array().map_or(0, Vec::len))
        .sum();
    assert_eq!(calls, CALLS);
    Ok(())
}

#[test]
fn basic_dyndocpath994_projects_lower_validate_and_emit() -> TestResult<()> {
    let runtime = super::execution_bindings::runtime_path()?;
    for (index, project) in models().iter().enumerate() {
        let model = projects::MODELS[index];
        let before = mapping::project_file::encode_pretty(project)?;
        assert!(
            engine::validate(project).is_empty(),
            "{model}: {:?}",
            engine::validate(project)
        );
        engine::check_source_ownership_capability(project, None)
            .map_err(|error| format!("{model}: {error}"))?;
        let program = LOWER(project).map_err(|error| format!("{model}: {error}"))?;
        codegen::validate_basic_owned_dynamic_document_path_emission(&program)
            .map_err(|error| format!("{model}: {error}"))?;
        let options =
            super::execution_bindings::rust_options(&format!("{TAG}-model{index}"), &runtime);
        (EMITTERS.rust)(&program, &options).map_err(|error| format!("{model}: {error}"))?;
        (EMITTERS.csharp)(&program).map_err(|error| format!("{model}: {error}"))?;
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{model}"
        );
    }
    Ok(())
}

#[test]
fn basic_dyndocpath994_interpreter_matches_expected() -> TestResult<()> {
    check_interpreter(&controls()?, &models(), CALLS)
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_dyndocpath994_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let programs = models().iter().map(LOWER).collect::<Result<Vec<_>, _>>()?;
    check_generated_hosts(TAG, &controls()?, &programs, EMITTERS, CALLS)
}

/// A loaded owner's document path is admitted only with both dynamic loading
/// and loaded document paths; every other preset keeps the dynamic-owner
/// refusal, while the document-set preset still reads static owners.
#[test]
fn basic_dyndocpath994_other_presets_keep_the_loaded_read_closed() -> TestResult<()> {
    let program = LOWER(&projects::project("outside"))?;
    for preset in codegen::OwnedPreset::ALL {
        let result = codegen::validate_basic_owned_emission_with_capabilities(
            &program,
            preset.capabilities(),
        );
        if preset == codegen::OwnedPreset::DynamicDocumentPaths {
            result?;
        } else {
            let error = result.expect_err("closed outside the loaded document-path preset");
            assert_eq!(
                capability_reason(&error),
                codegen::OwnedSourceCapabilityReason::DynamicOwner,
                "{preset:?}"
            );
        }
    }
    Ok(())
}

/// A legacy dynamic source has no owned loading scope: the interpreter and
/// the new preset both refuse a document-path read naming it.
#[test]
fn basic_dyndocpath994_legacy_dynamic_owner_stays_refused() {
    let mut project = projects::project("outside");
    // Parts would otherwise be an owned driver over the legacy source.
    project.extra_sources.truncate(2);
    for node in [2, 4, 8] {
        project.graph.nodes.remove(&node);
    }
    project.extra_sources[1].dynamic_path =
        Some(mapping::DynamicSourcePath::legacy(1, vec!["Files".into()]));
    let actual = engine::run_with_sources(&project, &Instance::Group(vec![].into()), vec![]);
    assert!(
        matches!(
            actual,
            Err(engine::EngineError::UnsupportedOwnedContext {
                context: engine::OwnedCapabilityContext::DynamicOwner,
                ..
            })
        ),
        "{actual:?}"
    );
    let error = LOWER(&project).expect_err("legacy dynamic owner refused");
    assert!(
        error.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            codegen::Diagnostic::OwnedSource { diagnostic }
                if matches!(diagnostic.as_ref(), codegen::OwnedSourceDiagnostic::Capability {
                    reason: codegen::OwnedSourceCapabilityReason::DynamicOwner,
                    ..
                })
        )),
        "{error:?}"
    );
}
