//! #469 owned XML serialization parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expected results.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic as OwnedDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceStructureReason as Structure,
    ProgramValidationError,
};
use mapping::{SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};
use serde_json::{Value as Json, json};
use std::fs;

#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-xml-serialize/projects.rs"]
mod projects;

// The composed constructions of the earlier companions. Their own harnesses
// keep private copies, so this file loads the authored models independently.
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-path-hierarchy/projects.rs"]
mod hierarchy_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-adjacency-tree/projects.rs"]
mod adjacency_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-recursive-collect/projects.rs"]
mod recursive_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-recursive-filter/projects.rs"]
mod filter_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-lookup-find/projects.rs"]
mod search_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-dynamic-field/projects.rs"]
mod dynamic_projects;

const CASES: usize = 19;
const CALLS: usize = 35;

fn corpus() -> TestResult<Json> {
    let corpus: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-xml-serialize/cases.json"
    ))?;
    let cases = corpus["cases"].as_array().ok_or("cases")?;
    let mut ids = std::collections::BTreeSet::new();
    let mut calls = 0;
    for case in cases {
        let model = case["model"].as_str().ok_or("model")?;
        if !ids.insert(case["id"].as_str().ok_or("id")?) || !projects::MODELS.contains(&model) {
            return Err(format!("duplicate case or unknown model: {case}").into());
        }
        calls += case["calls"].as_array().ok_or("calls")?.len();
    }
    if cases.len() != CASES || calls != CALLS {
        return Err(format!("{} cases and {calls} calls changed", cases.len()).into());
    }
    Ok(corpus)
}

/// Host controls refer to models by their index in `projects::MODELS`.
fn controls(corpus: &Json) -> Json {
    let mut controls = corpus.clone();
    for case in controls["cases"].as_array_mut().into_iter().flatten() {
        let model = projects::MODELS
            .iter()
            .position(|model| case["model"] == *model)
            .expect("checked corpus model");
        case["model"] = json!(model);
    }
    controls
}

fn decode(value: &Json) -> Instance {
    match value {
        Json::Null => Instance::Scalar(Value::Null),
        Json::Bool(value) => Instance::Scalar(Value::Bool(*value)),
        Json::Number(value) => Instance::Scalar(Value::Int(value.as_i64().expect("integer"))),
        Json::String(value) => Instance::Scalar(Value::String(value.clone())),
        Json::Array(items) => Instance::Repeated(items.iter().map(decode).collect()),
        Json::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), decode(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}

fn encode(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(Value::Null) => Json::Null,
        Instance::Scalar(Value::String(value)) => json!(value),
        Instance::Repeated(items) => Json::Array(items.iter().map(encode).collect()),
        Instance::Group(fields) => Json::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), encode(value)))
                .collect(),
        ),
        other => json!({ "unexpected": format!("{other:?}") }),
    }
}

fn owner_name(owner: &SourceOwner) -> Json {
    match owner {
        SourceOwner::Primary => Json::Null,
        SourceOwner::Named { name } => json!(name),
    }
}

fn engine_error(error: &engine::EngineError) -> Json {
    use engine::EngineError as E;
    use engine::OwnedSourceError as O;
    match error {
        E::XmlSerialization { node, .. } => json!({"kind": "XmlSerialization", "node": node}),
        E::UnknownTarget { name } => json!({"kind": "UnknownTarget", "name": name}),
        E::MappingFailure { rule, message } => {
            json!({"kind": "MappingFailure", "rule": rule, "message": message})
        }
        E::OwnedSourceAccess { node, source, .. } => match source {
            O::SourceUnavailable { owner } => {
                json!({"kind": "SourceUnavailable", "owner": owner_name(owner), "node": node})
            }
            O::MissingOwnedElement { owner, path } => json!({"kind": "MissingElement",
                "owner": owner_name(owner), "path": path, "node": node}),
            O::InactiveSourceFrame { owner, collection } => json!({"kind": "InactiveSourceFrame",
                "owner": owner_name(owner), "collection": collection, "node": node}),
            O::UniteratedOwnedRepetition { path } => {
                json!({"kind": "UniteratedRepetition", "path": path, "node": node})
            }
            other => json!({"kind": "Unexpected", "debug": format!("{other:?}")}),
        },
        other => json!({"kind": "Unexpected", "debug": format!("{other:?}")}),
    }
}

fn native(project: &Project, case: &Json, selection: &str) -> Json {
    let source = decode(&case["source"]);
    let inputs = case["inputs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|input| {
            (
                input[0].as_str().unwrap_or_default().into(),
                decode(&input[1]),
            )
        })
        .collect::<Vec<_>>();
    let execution = engine::ExecutionContext::new(Path::new("xml-serialize469.mapping"));
    if selection == "All" {
        return match engine::run_outputs_with_sources_and_context(
            project, &source, inputs, &execution,
        ) {
            Ok(outputs) => {
                let mut trees = vec![json!([null, encode(&outputs.primary)])];
                trees.extend(
                    outputs
                        .extras
                        .iter()
                        .map(|named| json!([named.name, encode(&named.instance)])),
                );
                json!({ "Ok": trees })
            }
            Err(error) => json!({ "Err": engine_error(&error) }),
        };
    }
    let target = if selection == "Primary" {
        engine::TargetSelection::Primary
    } else {
        engine::TargetSelection::Named(selection)
    };
    match engine::run_selected_target_with_sources_and_context(
        project, &source, inputs, &execution, target,
    ) {
        Ok(engine::SelectedTargetOutput::Primary(instance)) => {
            json!({ "Ok": [[null, encode(&instance)]] })
        }
        Ok(engine::SelectedTargetOutput::Named(named)) => {
            json!({ "Ok": [[named.name, encode(&named.instance)]] })
        }
        Err(error) => json!({ "Err": engine_error(&error) }),
    }
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

fn root(index: Option<(usize, &str)>, children: &[usize]) -> SourceScopeLocation {
    SourceScopeLocation {
        target: match index {
            None => SourceScopeTarget::Primary,
            Some((index, name)) => SourceScopeTarget::Named {
                index,
                name: name.into(),
            },
        },
        path: children
            .iter()
            .copied()
            .map(SourceScopeStep::Child)
            .collect(),
    }
}

fn diagnostic_owner(owner: Option<&str>) -> Owner {
    match owner {
        None => Owner::Primary,
        Some(name) => Owner::Named { name: name.into() },
    }
}

fn element(owner: Option<&str>, path: &[&str], frame: Option<&[&str]>) -> Request {
    Request::XmlElement {
        owner: diagnostic_owner(owner),
        path: path.iter().map(|part| (*part).into()).collect(),
        frame: frame.map(|frame| frame.iter().map(|part| (*part).into()).collect()),
    }
}

/// A closed context refusing one owned serializer at its reached expression.
fn refused(
    node: u32,
    scope: Option<SourceScopeLocation>,
    request: Request,
    reason: Capability,
    origin: Origin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedDiagnostic::Capability {
            location: Location::Expression {
                namespace: Namespace::Main,
                node,
                scope,
            },
            request: Some(request),
            reason,
            origin,
        }),
    }
}

fn xml_operator(node: u32, scope: SourceScopeLocation, request: Request) -> ProgramValidationError {
    refused(
        node,
        Some(scope),
        request,
        Capability::XmlOperator,
        Origin::Expression {
            namespace: Namespace::Main,
            node,
        },
    )
}

fn structure(node: u32, request: Request, reason: Structure) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedDiagnostic::Structure {
            location: Location::Expression {
                namespace: Namespace::Main,
                node,
                scope: None,
            },
            request,
            reason,
        }),
    }
}

/// The first owned serializer an older profile reaches. Profiles without
/// owned By grouping refuse the grouped model's scope first.
fn older_refusal(model: &str, grouping: bool) -> ProgramValidationError {
    const DETAIL: &[&str] = &["Orders", "Order", "Detail"];
    const ORDER: &[&str] = &["Orders", "Order"];
    match model {
        "X01-endpoints" | "X05-failure-walk" => {
            xml_operator(1, root(None, &[]), element(None, &["Header"], None))
        }
        "X02-nested" => xml_operator(10, root(None, &[0]), element(None, DETAIL, Some(ORDER))),
        "X03-frames" => xml_operator(30, root(None, &[0]), element(None, &["Header"], Some(&[]))),
        "X04-grouped" if grouping => {
            xml_operator(41, root(None, &[0]), element(None, DETAIL, None))
        }
        "X04-grouped" => ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedDiagnostic::Capability {
                location: Location::Iteration {
                    scope: root(None, &[0]),
                },
                request: Some(Request::Iteration {
                    owner: Owner::Primary,
                    path: ORDER.iter().map(|part| (*part).into()).collect(),
                }),
                reason: Capability::Grouping,
                origin: Origin::Scope {
                    scope: root(None, &[0]),
                },
            }),
        },
        _ => unreachable!("authored model"),
    }
}

fn diagnostic(error: &ProgramValidationError) -> codegen::Diagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        unreachable!("owned refusal");
    };
    codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    }
}

/// Every complete new-profile surface for one directly constructed Program.
fn assert_new_profile_refuses(
    program: &codegen::Program,
    expected: &ProgramValidationError,
    label: &str,
) -> TestResult<()> {
    let before = program.clone();
    let options = rust_options("xml-serialize469-refusal", &runtime_path()?);
    assert_eq!(
        codegen::validate_program_with_basic_owned_xml_serialize(program).as_ref(),
        Err(expected),
        "{label} validator"
    );
    assert_eq!(
        codegen::validate_basic_owned_xml_serialize_emission(program).as_ref(),
        Err(expected),
        "{label} emission validator"
    );
    assert!(
        matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(program),
            Err(error) if error == *expected),
        "{label} catalogue"
    );
    assert!(
        matches!(codegen_rust::emit_with_basic_owned_xml_serialize(program, &options),
            Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
        "{label} Rust"
    );
    assert!(
        matches!(codegen_csharp::emit_with_basic_owned_xml_serialize(program),
            Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
        "{label} C#"
    );
    assert_eq!(program, &before, "{label}");
    Ok(())
}

#[test]
fn basic_xml_serialize469_interpreter_matches_expected() -> TestResult<()> {
    let corpus = corpus()?;
    let mut differing = Vec::new();
    for case in corpus["cases"].as_array().ok_or("cases")? {
        let project = projects::project(case["model"].as_str().ok_or("model")?);
        assert!(engine::validate(&project).is_empty(), "{}", case["id"]);
        for call in case["calls"].as_array().ok_or("calls")? {
            let selection = call["selection"].as_str().ok_or("selection")?;
            let actual = native(&project, case, selection);
            if actual != call["expected"] {
                differing.push(json!({"case": case["id"], "selection": selection,
                    "actual": actual, "expected": call["expected"]}));
            }
        }
    }
    assert!(differing.is_empty(), "{differing:#?}");
    Ok(())
}

#[test]
fn basic_xml_serialize469_profile_lowers_validates_and_emits() -> TestResult<()> {
    let runtime = runtime_path()?;
    for (index, model) in projects::MODELS.iter().enumerate() {
        let project = projects::project(model);
        let before = mapping::project_file::encode_pretty(&project)?;
        assert!(before.contains("\"owned_xml_serialize\""), "{model}");
        let decoded = mapping::project_file::decode_str(&before)?;
        assert_eq!(mapping::project_file::encode_pretty(&decoded)?, before);
        let program = codegen::lower_with_basic_owned_xml_serialize(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        assert_eq!(mapping::project_file::encode_pretty(&project)?, before);
        let original = program.clone();
        codegen::validate_program_with_basic_owned_xml_serialize(&program)?;
        codegen::validate_basic_owned_xml_serialize_emission(&program)?;
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(&program)?;
        // Owned endpoints are never eagerly required legacy aliases.
        assert!(
            plan.all_outputs
                .entries
                .iter()
                .all(|entry| entry.availability == codegen::NamedInputAvailability::Optional),
            "{model}"
        );
        let rust = codegen_rust::emit_with_basic_owned_xml_serialize(
            &program,
            &rust_options(&format!("xml-serialize469-model{index}"), &runtime),
        )?;
        let csharp = codegen_csharp::emit_with_basic_owned_xml_serialize(&program)?;
        let text = |artifacts: &codegen::ArtifactSet| {
            artifacts
                .files()
                .iter()
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        assert!(text(&rust).contains(".owned_xml_instance("), "{model}");
        assert!(
            text(&csharp).contains("context.ResolveOwnedXmlInstance("),
            "{model}"
        );
        assert_eq!(program, original, "{model}");
    }
    Ok(())
}

#[test]
fn basic_xml_serialize469_older_profiles_and_default_validation_refuse() -> TestResult<()> {
    let options = rust_options("xml-serialize469-old", &runtime_path()?);
    for model in projects::MODELS {
        let project = projects::project(model);
        // (lowering, whether the profile admits owned By grouping)
        let lowered = [
            (codegen::lower(&project), false),
            (codegen::lower_with_basic_owned_filters(&project), false),
            (codegen::lower_with_basic_owned_sort(&project), false),
            (codegen::lower_with_basic_owned_windows(&project), false),
            (
                codegen::lower_with_basic_owned_row_failures(&project),
                false,
            ),
            (codegen::lower_with_basic_owned_group_by(&project), true),
            (
                codegen::lower_with_basic_owned_group_adjacent_by(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_group_starting_with(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_group_ending_with(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_failure_sources(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_sequence_item_at(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_path_hierarchy(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_adjacency_tree(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_group_into_blocks(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_recursive_collect(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_sequence_exists(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_sequence_aggregate(&project),
                true,
            ),
            (
                codegen::lower_with_basic_owned_recursive_filter(&project),
                true,
            ),
            (codegen::lower_with_basic_owned_lookup_find(&project), true),
            (
                codegen::lower_with_basic_owned_dynamic_field(&project),
                true,
            ),
        ];
        for (index, (result, grouping)) in lowered.iter().enumerate() {
            let expected = diagnostic(&older_refusal(model, *grouping));
            assert!(
                matches!(result, Err(error) if error.diagnostics() == std::slice::from_ref(&expected)),
                "{model} lowering {index}: {result:?}"
            );
        }
        // Directly constructed Programs keep the same refusal on every older surface.
        let program = codegen::lower_with_basic_owned_xml_serialize(&project)?;
        let closed = older_refusal(model, false);
        let grouped = older_refusal(model, true);
        let validators = [
            (codegen::validate_program(&program), &closed),
            (codegen::validate_owned_emission(&program), &closed),
            (
                codegen::validate_program_with_basic_owned_filters(&program),
                &closed,
            ),
            (
                codegen::validate_program_with_basic_owned_sort(&program),
                &closed,
            ),
            (
                codegen::validate_program_with_basic_owned_windows(&program),
                &closed,
            ),
            (
                codegen::validate_program_with_basic_owned_row_failures(&program),
                &closed,
            ),
            (codegen::validate_basic_owned_emission(&program), &closed),
            (
                codegen::validate_program_with_basic_owned_group_by(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_group_adjacent_by(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_group_starting_with(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_group_starting_with_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_group_ending_with(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_group_ending_with_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_sequence_item_at(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_failure_sources(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_failure_source_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_path_hierarchy(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_path_hierarchy_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_adjacency_tree(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_adjacency_tree_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_group_into_blocks(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_group_into_blocks_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_recursive_collect(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_recursive_collect_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_sequence_exists(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_sequence_exists_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_sequence_aggregate(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_sequence_aggregate_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_recursive_filter(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_recursive_filter_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_lookup_find(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_lookup_find_emission(&program),
                &grouped,
            ),
            (
                codegen::validate_program_with_basic_owned_dynamic_field(&program),
                &grouped,
            ),
            (
                codegen::validate_basic_owned_dynamic_field_emission(&program),
                &grouped,
            ),
        ];
        for (index, (result, expected)) in validators.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(*expected), "{model} validator {index}");
        }
        let catalogues = [
            (
                codegen::legacy_named_input_admission_plan(&program).map(|_| ()),
                &closed,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(
                    &program,
                )
                .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(&program)
                    .map(|_| ()),
                &grouped,
            ),
            (
                codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(&program)
                    .map(|_| ()),
                &grouped,
            ),
        ];
        for (index, (result, expected)) in catalogues.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(*expected), "{model} catalogue {index}");
        }
        let rust = [
            (codegen_rust::emit(&program, &options), &closed),
            (
                codegen_rust::emit_with_basic_owned_sources(&program, &options),
                &closed,
            ),
            (
                codegen_rust::emit_with_basic_owned_windows(&program, &options),
                &closed,
            ),
            (
                codegen_rust::emit_with_basic_owned_group_ending_with(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_failure_sources(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_path_hierarchy(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_adjacency_tree(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_group_into_blocks(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_recursive_collect(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_sequence_exists(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_sequence_aggregate(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_recursive_filter(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_lookup_find(&program, &options),
                &grouped,
            ),
            (
                codegen_rust::emit_with_basic_owned_dynamic_field(&program, &options),
                &grouped,
            ),
        ];
        for (index, (result, expected)) in rust.iter().enumerate() {
            assert!(
                matches!(result, Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
                "{model} Rust {index}: {result:?}"
            );
        }
        let csharp = [
            (codegen_csharp::emit(&program), &closed),
            (
                codegen_csharp::emit_with_basic_owned_sources(&program),
                &closed,
            ),
            (
                codegen_csharp::emit_with_basic_owned_windows(&program),
                &closed,
            ),
            (
                codegen_csharp::emit_with_basic_owned_group_ending_with(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_failure_sources(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_path_hierarchy(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_adjacency_tree(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_group_into_blocks(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_recursive_collect(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_sequence_exists(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_sequence_aggregate(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_recursive_filter(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_lookup_find(&program),
                &grouped,
            ),
            (
                codegen_csharp::emit_with_basic_owned_dynamic_field(&program),
                &grouped,
            ),
        ];
        for (index, (result, expected)) in csharp.iter().enumerate() {
            assert!(
                matches!(result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
                "{model} C# {index}: {result:?}"
            );
        }
    }
    Ok(())
}

/// The companion composes the latest companion chain: every owned
/// path-hierarchy, adjacency-tree, recursive-collection, recursive-filter,
/// Lookup/CollectionFind and dynamic source-field model lowers, validates and
/// emits with it.
#[test]
fn basic_xml_serialize469_profile_composes_earlier_owned_companions() -> TestResult<()> {
    let runtime = runtime_path()?;
    let models = hierarchy_projects::MODELS
        .iter()
        .map(|model| (*model, hierarchy_projects::project(model)))
        .chain(
            adjacency_projects::MODELS
                .iter()
                .map(|model| (*model, adjacency_projects::project(model))),
        )
        .chain(
            recursive_projects::PROFILES
                .iter()
                .map(|model| (*model, recursive_projects::project(model))),
        )
        .chain(
            filter_projects::PROFILES
                .iter()
                .map(|model| (*model, filter_projects::project(model))),
        )
        .chain(
            search_projects::PROFILES
                .iter()
                .map(|model| (*model, search_projects::project(model))),
        )
        .chain(
            dynamic_projects::PROFILES
                .iter()
                .map(|model| (*model, dynamic_projects::project(model))),
        );
    for (model, project) in models {
        let program = codegen::lower_with_basic_owned_xml_serialize(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        codegen::validate_basic_owned_xml_serialize_emission(&program)?;
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(&program)?;
        codegen_rust::emit_with_basic_owned_xml_serialize(
            &program,
            &rust_options("xml-serialize469-composed", &runtime),
        )?;
        codegen_csharp::emit_with_basic_owned_xml_serialize(&program)?;
    }
    Ok(())
}

#[test]
fn basic_xml_serialize469_direct_programs_refuse_unsupported_ownership() -> TestResult<()> {
    let base = codegen::lower_with_basic_owned_xml_serialize(&projects::project("X01-endpoints"))?;
    let with = |change: &dyn Fn(&mut mapping::OwnedXmlSerialize)| {
        let mut program = base.clone();
        let node = program
            .expressions
            .iter_mut()
            .find(|node| node.id == 1)
            .expect("primary serializer");
        let codegen::Expression::OwnedXmlSerialize(serializer) = &mut node.expression else {
            unreachable!("authored owned serializer");
        };
        change(serializer);
        program
    };
    let path = |segments: &[&str]| segments.iter().map(|part| (*part).into()).collect();
    let cases = [
        (
            "unknown owner",
            with(&|serializer| serializer.source.owner = projects::owner(Some("missing"))),
            structure(
                1,
                element(Some("missing"), &["Header"], None),
                Structure::UnknownOwner,
            ),
        ),
        (
            "empty owner",
            with(&|serializer| serializer.source.owner = projects::owner(Some(""))),
            structure(
                1,
                element(Some(""), &["Header"], None),
                Structure::InvalidNamedOwner,
            ),
        ),
        (
            "missing path",
            with(&|serializer| serializer.source.path = path(&["Orders", "Nope"])),
            structure(
                1,
                element(None, &["Orders", "Nope"], None),
                Structure::MissingOwnedPath { segment: 1 },
            ),
        ),
        (
            "frame outside path",
            with(&|serializer| serializer.frame = Some(path(&["Orders", "Order"]))),
            structure(
                1,
                element(None, &["Header"], Some(&["Orders", "Order"])),
                Structure::IncompatibleFramePath,
            ),
        ),
        (
            "frame not a collection",
            with(&|serializer| serializer.frame = Some(path(&["Header"]))),
            structure(
                1,
                element(None, &["Header"], Some(&["Header"])),
                Structure::InvalidOwnedCollection {
                    found: codegen::OwnedSourceSchemaShape::Group { repeating: false },
                },
            ),
        ),
        (
            "schema of another element",
            with(&|serializer| *serializer.schema = projects::detail_schema()),
            ProgramValidationError::InvalidXmlSerializeSource {
                node: 1,
                path: path(&["Header"]),
                schema: "Detail".into(),
            },
        ),
        (
            "scalar element for a group schema",
            with(&|serializer| serializer.source.path = path(&["Single"])),
            ProgramValidationError::InvalidXmlSerializeSource {
                node: 1,
                path: path(&["Single"]),
                schema: "Header".into(),
            },
        ),
        (
            "repeating schema",
            with(&|serializer| *serializer.schema = projects::header_schema().repeating()),
            ProgramValidationError::RepeatingXmlSerializeSchema {
                node: 1,
                schema: "Header".into(),
            },
        ),
        (
            "empty namespace",
            with(&|serializer| serializer.namespace = Some(String::new())),
            ProgramValidationError::EmptyXmlSerializeNamespace { node: 1 },
        ),
    ];
    for (label, program, expected) in &cases {
        assert_new_profile_refuses(program, expected, label)?;
        // The default validator applies the same structural checks first.
        assert_eq!(
            codegen::validate_program(program).as_ref(),
            Err(expected),
            "{label}"
        );
    }
    Ok(())
}

/// A failure-rule message, generated item and isolated function body never
/// admit the owned serializer, in the interpreter or in generation.
#[test]
fn basic_xml_serialize469_closed_contexts_refuse_before_emission() -> TestResult<()> {
    let lowered_error = |project: &Project, expected: &ProgramValidationError| {
        let lowered = codegen::lower_with_basic_owned_xml_serialize(project);
        assert!(
            matches!(&lowered, Err(error)
                if error.diagnostics() == std::slice::from_ref(&diagnostic(expected))),
            "{lowered:?}"
        );
    };
    let source = decode(&json!({"Header": {"id": "p"}, "Orders": {"Order": [{"Label": "a"}]}}));

    // A dynamic named owner never supplies an exact owned element.
    let mut dynamic = projects::project("X01-endpoints");
    dynamic.graph.nodes.insert(
        90,
        Node::Const {
            value: Value::String("audit.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 90,
        iteration: vec!["Orders".into(), "Order".into()],
    });
    lowered_error(
        &dynamic,
        &refused(
            2,
            Some(root(Some((0, "audit")), &[])),
            element(Some("audit"), &["Header"], None),
            Capability::DynamicOwner,
            Origin::DynamicSource {
                index: 0,
                name: "audit".into(),
            },
        ),
    );

    // A generated item context closes the owned read inside it.
    let mut generated = projects::project("X01-endpoints");
    generated.extra_targets.clear();
    generated.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Item", vec![SchemaNode::scalar("Xml", ScalarType::String)])
                .repeating(),
        ],
    );
    generated.root = Scope {
        children: vec![Scope {
            target_field: "Item".into(),
            iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
                input: 91,
                delimiter: 92,
                item: 93,
            }),
            bindings: vec![Binding {
                target_field: "Xml".into(),
                node: 1,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    generated.graph.nodes.extend([
        (
            91,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            92,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            93,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    assert!(engine::validate(&generated).is_empty());
    lowered_error(
        &generated,
        &refused(
            1,
            Some(root(None, &[0])),
            element(None, &["Header"], None),
            Capability::GeneratedItem,
            Origin::Scope {
                scope: root(None, &[0]),
            },
        ),
    );
    assert!(matches!(
        engine::run_with_sources(&generated, &source, vec![]),
        Err(engine::EngineError::UnsupportedOwnedContext {
            context: engine::OwnedCapabilityContext::GeneratedItem,
            ..
        })
    ));

    // An owned failure walk's message stays within its scalar subset.
    let mut failure = projects::project("X05-failure-walk");
    failure.failure_rules[0].message = Some(1);
    assert!(engine::validate(&failure).is_empty());
    // Generation refuses the whole owned walk whose message leaves that subset.
    lowered_error(
        &failure,
        &ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedDiagnostic::Capability {
                location: Location::FailureIteration { index: 0 },
                request: Some(Request::Iteration {
                    owner: diagnostic_owner(Some("audit")),
                    path: vec!["Orders".into(), "Order".into()],
                }),
                reason: Capability::FailureRule,
                origin: Origin::Failure { index: 0 },
            }),
        },
    );
    assert!(matches!(
        engine::run_with_sources(&failure, &source, vec![]),
        Err(engine::EngineError::UnsupportedOwnedContext {
            node: Some(1),
            context: engine::OwnedCapabilityContext::FailureRule,
            ..
        })
    ));

    // An isolated function body never reads an endpoint.
    let mut function = projects::project("X05-failure-walk");
    function.failure_rules.clear();
    let id = FunctionId::new(1);
    function.user_functions.insert(
        id,
        UserFunction {
            library: "user".into(),
            name: "serialize".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(
                    5,
                    projects::serializer(
                        None,
                        &["Header"],
                        None,
                        projects::header_schema(),
                        (false, false, None),
                    ),
                )]
                .into(),
            },
            output: 5,
        },
    );
    function.graph.nodes.insert(
        1,
        Node::UserFunctionCall {
            function: id,
            args: vec![],
        },
    );
    assert!(matches!(
        engine::run_with_sources(&function, &source, vec![]),
        Err(engine::EngineError::UnsupportedSourceOwnership {
            node: 5,
            function: Some(_),
            kind: "owned_xml_serialize",
        })
    ));
    assert!(codegen::lower_with_basic_owned_xml_serialize(&function).is_err());
    Ok(())
}

#[test]
fn basic_xml_serialize469_legacy_serializer_and_errors_are_unchanged() -> TestResult<()> {
    let legacy = projects::legacy();
    let wire = mapping::project_file::encode_pretty(&legacy)?;
    assert!(wire.contains("\"xml_serialize\"") && !wire.contains("owned"));
    let decoded = mapping::project_file::decode_str(&wire)?;
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, wire);
    // Ordinary generation keeps the legacy route.
    let program = codegen::lower(&legacy)?;
    codegen_rust::emit(
        &program,
        &rust_options("xml-serialize469-legacy", &runtime_path()?),
    )?;
    codegen_csharp::emit(&program)?;
    // The owned companion keeps the same legacy refusal as earlier companions.
    let owned = codegen::lower_with_basic_owned_xml_serialize(&legacy);
    let dynamic = codegen::lower_with_basic_owned_dynamic_field(&legacy);
    assert!(owned.is_err());
    assert_eq!(
        owned.err().map(|error| error.diagnostics().to_vec()),
        dynamic.err().map(|error| error.diagnostics().to_vec())
    );
    let header = json!({"Header": {"id": "p", "Note": "primary"}});
    assert_eq!(
        encode(&engine::run(&legacy, &decode(&header))?),
        json!({"Primary": "<Header id=\"p\">\n  <Note>primary</Note>\n</Header>"})
    );
    // Legacy outward fallback still repairs a missing primary element with
    // the root of the named endpoint whose name matches the path.
    let alias = decode(&json!({"id": "h", "Note": "alias root"}));
    assert_eq!(
        encode(&engine::run_with_sources(
            &legacy,
            &decode(&json!({})),
            vec![("Header".into(), alias)]
        )?),
        json!({"Primary": "<Header id=\"h\">\n  <Note>alias root</Note>\n</Header>"})
    );
    Ok(())
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        fs::write(path, &file.contents)?;
    }
    Ok(())
}

fn host_rows(output: &Path, expected: usize, run: &Output) -> Result<(), String> {
    let rows: Vec<Json> = fs::read(output.join("ALL-COMPLETE-COMPARISONS.json"))
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .unwrap_or_default();
    let differing = rows
        .iter()
        .filter(|row| row["matched"] != true)
        .collect::<Vec<_>>();
    if run.status.success() && rows.len() == expected && differing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{}: status {:?}, {} rows of {expected}, differing {differing:#?}\n{}{}",
        output.display(),
        run.status,
        rows.len(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    ))
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_xml_serialize469_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let corpus = corpus()?;
    let controls = controls(&corpus);
    let runtime = runtime_path()?;
    let temp = TempDir::new("xml_serialize469_hosts")?;
    let root = temp.0.canonicalize()?;
    fs::write(
        root.join("controls.json"),
        serde_json::to_vec_pretty(&controls)?,
    )?;
    let calls = |model: usize| {
        controls["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|case| case["model"] == model)
            .map(|case| case["calls"].as_array().map_or(0, Vec::len))
            .sum::<usize>()
    };

    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, model) in projects::MODELS.iter().enumerate() {
        let program = codegen::lower_with_basic_owned_xml_serialize(&projects::project(model))?;
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_xml_serialize(
                &program,
                &rust_options(&format!("xml-serialize469-model{index}"), &runtime),
            )?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_xml_serialize(&program)?,
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"xml-serialize469-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "                {index} => invoke!(model{index}, selection, &source, &inputs, &context, limits),\n"
        ));
    }
    let host = include_str!("basic_owned_xml_serialize/Host.rs.txt");
    if host.matches("@DISPATCH@").count() != 1 {
        return Err("Rust host dispatch seam changed".into());
    }
    fs::create_dir_all(root.join("rust-host/src"))?;
    fs::write(
        root.join("rust-host/src/main.rs"),
        host.replace("@DISPATCH@", dispatch.trim_end()),
    )?;
    fs::write(
        root.join("rust-host/Cargo.toml"),
        format!(
            "[package]\nname=\"xml-serialize469-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime.display().to_string())?
        ),
    )?;

    let mut failures = Vec::new();
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
        let output = root.join("rust-actual");
        let run = Command::new(target.join("debug/xml-serialize469-host"))
            .arg(root.join("controls.json"))
            .arg(&output)
            .output()?;
        if let Err(error) = host_rows(&output, CALLS, &run) {
            failures.push(format!("Rust host: {error}"));
        }
    } else {
        failures.push(format!(
            "Rust host build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }

    // Each C# model has its own Ferrule.Generated assembly and therefore its own host.
    for index in 0..projects::MODELS.len() {
        let host = root.join(format!("csharp-host{index}"));
        fs::create_dir_all(&host)?;
        fs::write(
            host.join("Host.csproj"),
            format!(
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{index}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
            ),
        )?;
        fs::write(
            host.join("Program.cs"),
            include_str!("basic_owned_xml_serialize/Host.cs.txt"),
        )?;
        let artifacts = root.join(format!("dotnet-artifacts-{index}"));
        let build = dotnet_command(&root)
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
            .arg(host.join("Host.csproj"))
            .isolated_output()?;
        if !build.status.success() {
            failures.push(format!(
                "C# model{index} build failed:\n{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            ));
            continue;
        }
        let output = root.join(format!("csharp-actual{index}"));
        let run = dotnet_command(&root)
            .arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(root.join("controls.json"))
            .arg(index.to_string())
            .arg(&output)
            .isolated_output()?;
        if let Err(error) = host_rows(&output, calls(index), &run) {
            failures.push(format!("C# model{index}: {error}"));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n\n").into());
    }
    Ok(())
}
