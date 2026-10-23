//! #451 owned document-set paths: the interpreter, generated Rust and
//! generated C# run the portable conformance cases against their expected
//! results through the shared execution bindings.
use super::*;
use serde_json::Value as Json;

#[path = "../../../../conformance/basic-owned-document-path/projects.rs"]
mod projects;
use super::execution_bindings::{self as bindings, Emitters};
use super::owned_capability_presets::{self as presets, Corpus};

const TAG: &str = "docpath451";
const CASES: &str = include_str!("../../../../conformance/basic-owned-document-path/cases.json");
const CALLS: usize = 20;
const LOWER: bindings::Lower = codegen::lower_with_basic_owned_document_paths;
const EMITTERS: Emitters = Emitters {
    rust: codegen_rust::emit_with_basic_owned_document_paths,
    csharp: codegen_csharp::emit_with_basic_owned_document_paths,
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

fn document_path(owner: mapping::SourceOwner) -> Node {
    Node::OwnedSourceDocumentPath(mapping::OwnedSourceDocumentPath { owner })
}

fn named() -> mapping::SourceOwner {
    mapping::SourceOwner::Named {
        name: projects::NAMED.into(),
    }
}

fn capability_refusal(
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
fn basic_docpath451_cases_cover_every_model() -> TestResult<()> {
    let controls = controls()?;
    let ids = bindings::check_case_recipes(&controls)?;
    assert_eq!(ids.len() * 2, CALLS);
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
fn basic_docpath451_projects_lower_validate_and_emit() -> TestResult<()> {
    bindings::check_emission(TAG, &projects::MODELS, &models(), LOWER, EMITTERS)
}

#[test]
fn basic_docpath451_interpreter_matches_expected() -> TestResult<()> {
    bindings::check_interpreter(TAG, &controls()?, &models())
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_docpath451_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let programs = models().iter().map(LOWER).collect::<Result<Vec<_>, _>>()?;
    bindings::check_generated_hosts(TAG, &controls()?, &programs, EMITTERS)
}

/// The owned form is strict and additive: legacy projects keep their bytes,
/// and a reader that predates the kind, or a body with extra keys, refuses it.
#[test]
fn basic_docpath451_wire_form_is_strict_and_additive() -> TestResult<()> {
    let project = projects::project("nested");
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert!(wire.contains("\"owned_source_document_path\""));
    let decoded = mapping::project_file::decode_str(&wire)?;
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, wire);

    let mut legacy = project.clone();
    legacy.graph.nodes.insert(1, Node::SourceDocumentPath);
    legacy.graph.nodes.insert(2, Node::SourceDocumentPath);
    let legacy_wire = mapping::project_file::encode_pretty(&legacy)?;
    assert!(!legacy_wire.contains("owned_source_document_path"));
    assert_eq!(
        mapping::project_file::encode_pretty(&mapping::project_file::decode_str(&legacy_wire)?)?,
        legacy_wire
    );

    let node = serde_json::to_value(document_path(named()))?;
    assert_eq!(
        node,
        serde_json::json!({"kind": "owned_source_document_path", "owner": {"kind": "named", "name": "Ref"}})
    );
    for refused in [
        serde_json::json!({"kind": "owned_source_document_path", "owner": {"kind": "primary"}, "collection": []}),
        serde_json::json!({"kind": "owned_source_document_path"}),
        serde_json::json!({"kind": "owned_source_document_path", "owner": {"kind": "named", "name": ""}}),
    ] {
        assert!(
            serde_json::from_value::<Node>(refused.clone()).is_err(),
            "{refused}"
        );
    }
    Ok(())
}

/// Every other preset, and the ordinary emitters, keep the owned document
/// path and its document-set walk closed.
#[test]
fn basic_docpath451_other_presets_keep_the_document_path_closed() -> TestResult<()> {
    let project = projects::project("root");
    let program = codegen::lower(&project)?;
    for preset in codegen::OwnedPreset::ALL {
        let result = codegen::validate_basic_owned_emission_with_capabilities(
            &program,
            preset.capabilities(),
        );
        if preset == codegen::OwnedPreset::DocumentPaths {
            result?;
        } else {
            let error = result.expect_err("closed outside the document-path preset");
            assert_eq!(
                capability_refusal(&error),
                codegen::OwnedSourceCapabilityReason::Document,
                "{preset:?}"
            );
        }
    }
    let options = bindings::rust_options(&format!("{TAG}-ordinary"), &bindings::runtime_path()?);
    assert!(codegen_rust::emit(&program, &options).is_err());
    assert!(codegen_csharp::emit(&program).is_err());
    Ok(())
}

/// A dynamic named owner has no document-set member location: the
/// interpreter and generation refuse the read before any input is acquired.
#[test]
fn basic_docpath451_dynamic_owner_route_stays_refused() {
    let mut project = projects::project("root");
    project.root.bindings[0].node = 2;
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("loaded.xml".into()),
        },
    );
    project.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(9, vec![]));
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
    let error = LOWER(&project).expect_err("dynamic owner refused");
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

/// Grouped scopes keep refusing a document-set endpoint in the interpreter.
#[test]
fn basic_docpath451_grouped_document_set_scope_stays_refused() -> TestResult<()> {
    let mut project = projects::project("sorted");
    let rows = &mut project.root.children[0];
    rows.sort_by = None;
    rows.group_by = Some(3);
    let controls = controls()?;
    let source =
        super::basic_owned_sequence_item_at::codec::decode(&controls["cases"][0]["source"]);
    let actual = engine::run_with_sources(&project, &source, vec![]);
    assert!(
        matches!(
            actual,
            Err(engine::EngineError::UnsupportedOwnedContext {
                context: engine::OwnedCapabilityContext::Document,
                ..
            })
        ),
        "{actual:?}"
    );
    Ok(())
}
