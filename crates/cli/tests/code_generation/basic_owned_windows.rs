//! #704 owned windows: the interpreter, generated Rust and generated C# run
//! the portable conformance cases against their expected results through the
//! shared execution bindings.
use super::*;
use serde_json::Value as Json;

#[path = "../../../../conformance/basic-owned-windows/projects.rs"]
mod projects;
use super::execution_bindings::{self as bindings, Emitters};
use super::owned_capability_presets::{self as presets, Corpus};

const TAG: &str = "windows704";
const CASES: &str = include_str!("../../../../conformance/basic-owned-windows/cases.json");
const CALLS: usize = 12;
const LOWER: bindings::Lower = codegen::lower_with_basic_owned_windows;
const EMITTERS: Emitters = Emitters {
    rust: codegen_rust::emit_with_basic_owned_windows,
    csharp: codegen_csharp::emit_with_basic_owned_windows,
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
fn basic_windows704_cases_cover_every_model() -> TestResult<()> {
    let controls = controls()?;
    let ids = bindings::check_case_recipes(&controls)?;
    let expected: Vec<_> = (1..=CALLS).map(|case| format!("W{case:02}")).collect();
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

#[test]
fn basic_windows704_projects_lower_validate_and_emit() -> TestResult<()> {
    bindings::check_emission(TAG, &projects::MODELS, &models(), LOWER, EMITTERS)
}

#[test]
fn basic_windows704_interpreter_matches_expected() -> TestResult<()> {
    bindings::check_interpreter(TAG, &controls()?, &models())
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_windows704_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let programs = models().iter().map(LOWER).collect::<Result<Vec<_>, _>>()?;
    bindings::check_generated_hosts(TAG, &controls()?, &programs, EMITTERS)
}
