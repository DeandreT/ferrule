//! #670 owned sorting: the interpreter, generated Rust and generated C# run
//! the portable conformance cases against their expected results through the
//! shared execution bindings.
use super::*;
use serde_json::Value as Json;

#[path = "../../../../conformance/basic-owned-sort/projects.rs"]
mod projects;
use super::execution_bindings::{self as bindings, Emitters};
use super::owned_capability_presets::{self as presets, Corpus};

const TAG: &str = "sort670";
const CASES: &str = include_str!("../../../../conformance/basic-owned-sort/cases.json");
const CALLS: usize = 16;
const LOWER: bindings::Lower = codegen::lower_with_basic_owned_sort;
const EMITTERS: Emitters = Emitters {
    rust: codegen_rust::emit_with_basic_owned_sort,
    csharp: codegen_csharp::emit_with_basic_owned_sort,
};

fn models() -> Vec<Project> {
    projects::MODELS
        .iter()
        .map(|model| projects::project(model))
        .collect()
}

fn controls() -> TestResult<Json> {
    bindings::controls(CASES, &projects::MODELS)
}

pub(super) fn capability_corpus() -> Corpus {
    presets::projects(
        projects::MODELS
            .iter()
            .map(|model| (*model, projects::project(model))),
    )
}

#[test]
fn basic_sort670_cases_cover_every_model() -> TestResult<()> {
    let controls = controls()?;
    let ids = bindings::check_case_recipes(&controls)?;
    let expected: Vec<_> = (1..=CALLS).map(|case| format!("S{case:02}")).collect();
    assert_eq!(ids, expected);
    let cases = controls["cases"].as_array().ok_or("cases")?;
    for model in 0..projects::MODELS.len() {
        assert!(cases.iter().any(|case| case["model"] == model), "{model}");
    }
    for case in cases {
        for input in case["inputs"].as_array().ok_or("inputs")? {
            assert_eq!(input[0], projects::NAMED, "{}", case["id"]);
        }
    }
    assert_eq!(bindings::total_calls(&controls), CALLS);
    Ok(())
}

/// The S15 refusal compares whole typed values; its wrappers must also expose
/// the owned cause through `Error::source`.
#[test]
fn basic_sort670_unavailable_key_source_exposes_its_cause() {
    use std::error::Error as _;
    let cause = engine::OwnedSourceError::SourceUnavailable {
        owner: mapping::SourceOwner::Named {
            name: projects::NAMED.into(),
        },
    };
    let native = engine::EngineError::OwnedSourceAccess {
        node: Some(u32::MAX),
        scope: None,
        source: cause.clone(),
    };
    let source = native.source().and_then(|error| error.downcast_ref());
    assert_eq!(source, Some(&cause));

    let cause = || codegen_runtime::OwnedSourceError::SourceUnavailable {
        name: projects::NAMED.into(),
    };
    let generated = codegen_runtime::RuntimeError::OwnedSourceAtNode {
        node: u32::MAX,
        source: cause(),
    };
    let source = generated.source().and_then(|error| error.downcast_ref());
    assert_eq!(source, Some(&cause()));
}

#[test]
fn basic_sort670_projects_lower_validate_and_emit() -> TestResult<()> {
    bindings::check_emission(TAG, &projects::MODELS, &models(), LOWER, EMITTERS)
}

#[test]
fn basic_sort670_interpreter_matches_expected() -> TestResult<()> {
    bindings::check_interpreter(TAG, &controls()?, &models())
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_sort670_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let programs = models().iter().map(LOWER).collect::<Result<Vec<_>, _>>()?;
    bindings::check_generated_hosts(TAG, &controls()?, &programs, EMITTERS)
}
