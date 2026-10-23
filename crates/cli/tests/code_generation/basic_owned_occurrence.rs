//! #640 owned occurrence positions: the interpreter, generated Rust and
//! generated C# run the portable conformance cases against their expected
//! results through the shared execution bindings.
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::error::Error as _;

#[path = "../../../../conformance/basic-owned-occurrence/projects.rs"]
mod projects;
use super::basic_owned_sequence_item_at::codec;
use super::execution_bindings::{self as bindings, Emitters};

const TAG: &str = "occurrence640";
const ENDPOINTS: [&str; 3] = ["mixed", "empty", "singleton"];
const SELECTIONS: [&str; 2] = ["All", "Primary"];
const CALLS: usize = 12;
const EMITTERS: Emitters = Emitters {
    rust: codegen_rust::emit_with_basic_owned_sources,
    csharp: codegen_csharp::emit_with_basic_owned_sources,
};

fn models() -> Vec<Project> {
    projects::PROGRAMS
        .iter()
        .map(|program| projects::project(program))
        .collect()
}

pub(super) fn capability_corpus() -> super::owned_capability_presets::Corpus {
    super::owned_capability_presets::projects(
        projects::PROGRAMS
            .iter()
            .map(|program| (*program, projects::project(program))),
    )
}

/// The portable cases with each model name resolved to its host index.
fn controls() -> TestResult<Json> {
    let mut cases: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-occurrence/cases.json"
    ))?;
    if cases["version"] != 1 {
        return Err("unknown cases version".into());
    }
    for case in cases["cases"].as_array_mut().ok_or("cases")? {
        let model = projects::PROGRAMS
            .iter()
            .position(|program| case["model"] == *program)
            .ok_or("case names an unknown model")?;
        case["model"] = json!(model);
    }
    Ok(cases)
}

#[test]
fn basic_occurrence640_cases_cover_every_program_endpoint_and_selection() -> TestResult<()> {
    let controls = controls()?;
    let cases = controls["cases"].as_array().ok_or("cases")?;
    let mut ids = BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().ok_or("id")?;
        assert!(ids.insert(id), "duplicate case {id}");
        let model = case["model"].as_u64().ok_or("model")? as usize;
        let (program, endpoint) = id.split_once('-').ok_or("case id")?;
        assert_eq!(projects::PROGRAMS[model], program, "{id}");
        assert!(ENDPOINTS.contains(&endpoint), "{id}");
        assert_eq!(
            codec::encode(&codec::decode(&case["source"])),
            case["source"]
        );
        let inputs = case["inputs"].as_array().ok_or("inputs")?;
        assert_eq!(inputs.len(), usize::from(program == "named"), "{id}");
        for input in inputs {
            assert_eq!(input[0], projects::NAMED);
            assert_eq!(codec::encode(&codec::decode(&input[1])), input[1]);
        }
        let calls = case["calls"].as_array().ok_or("calls")?;
        let selections: Vec<_> = calls.iter().map(|call| &call["selection"]).collect();
        assert_eq!(selections, SELECTIONS, "{id}");
        for call in calls {
            match (call["expected"].get("Ok"), call["expected"].get("Err")) {
                (Some(outputs), None) => {
                    for output in outputs.as_array().ok_or("outputs")? {
                        assert_eq!(codec::encode(&codec::decode(&output[1])), output[1]);
                    }
                }
                (None, Some(error)) => assert_eq!(error["cause"], "InactiveSourceFrame"),
                _ => panic!("{id}: one expected outcome per call"),
            }
        }
    }
    assert_eq!(ids.len(), projects::PROGRAMS.len() * ENDPOINTS.len());
    assert_eq!(bindings::total_calls(&controls), CALLS);
    Ok(())
}

#[test]
fn basic_occurrence640_projects_lower_validate_and_emit() -> TestResult<()> {
    let runtime = bindings::runtime_path()?;
    for (index, project) in models().iter().enumerate() {
        let program_name = projects::PROGRAMS[index];
        let before = mapping::project_file::encode_pretty(project)?;
        let issues = engine::validate(project);
        assert!(issues.is_empty(), "{program_name}: {issues:?}");
        let program = codegen::lower(project)?;
        let options = bindings::rust_options(&format!("{TAG}-model{index}"), &runtime);
        (EMITTERS.rust)(&program, &options)?;
        (EMITTERS.csharp)(&program)?;
        assert_eq!(
            mapping::project_file::encode_pretty(project)?,
            before,
            "{program_name}"
        );
    }
    Ok(())
}

/// The inactive-frame refusals compare whole typed values; their wrappers
/// must also expose the owned cause through `Error::source`.
#[test]
fn basic_occurrence640_inactive_frame_errors_expose_their_cause() {
    for program in projects::PROGRAMS {
        let (native_owner, runtime_owner) = if program == "primary" {
            (
                mapping::SourceOwner::Primary,
                codegen_runtime::OwnedSourceIdentity::Primary,
            )
        } else {
            (
                mapping::SourceOwner::Named {
                    name: projects::NAMED.into(),
                },
                codegen_runtime::OwnedSourceIdentity::Named(projects::NAMED.into()),
            )
        };
        let cause = engine::OwnedSourceError::InactiveSourceFrame {
            owner: native_owner,
            collection: Some(vec![]),
        };
        let native = engine::EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: cause.clone(),
        };
        let source = native.source().and_then(|error| error.downcast_ref());
        assert_eq!(source, Some(&cause), "{program}");

        let cause = || codegen_runtime::OwnedSourceError::InactiveSourceFrame {
            owner: runtime_owner.clone(),
            collection: Some(vec![]),
        };
        let generated = codegen_runtime::RuntimeError::OwnedSourceAtNode {
            node: 2,
            source: cause(),
        };
        let source = generated.source().and_then(|error| error.downcast_ref());
        assert_eq!(source, Some(&cause()), "{program}");
    }
}

#[test]
fn basic_occurrence640_interpreter_matches_expected() -> TestResult<()> {
    bindings::check_interpreter(TAG, &controls()?, &models())
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_occurrence640_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let programs = models()
        .iter()
        .map(codegen::lower)
        .collect::<Result<Vec<_>, _>>()?;
    bindings::check_generated_hosts(TAG, &controls()?, &programs, EMITTERS)
}
