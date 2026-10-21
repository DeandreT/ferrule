//! #468 owned XML mixed-content parity: the interpreter, generated Rust and
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
#[path = "../../../../conformance/basic-owned-xml-mixed-content/projects.rs"]
mod projects;

// Every model of the companions this one composes. Their own harnesses keep
// private copies, so this file loads the authored models independently.
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-xml-serialize/projects.rs"]
mod serialize_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-dynamic-field/projects.rs"]
mod dynamic_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-lookup-find/projects.rs"]
mod search_projects;

const CASES: usize = 16;
const CALLS: usize = 27;

/// Mixed groups are authored compactly: a `~` member lists the ordered text
/// nodes (strings) and elements (`{name: text}`). Like the XML readers, each
/// element retains the next same-named field occurrence as its value, or its
/// text when the group has no such field, and the stream follows the fields.
fn expand(value: &Json) -> Json {
    match value {
        Json::Array(items) => Json::Array(items.iter().map(expand).collect()),
        Json::Object(fields) => {
            let mut output = serde_json::Map::new();
            for (name, value) in fields {
                if name != "~" {
                    output.insert(name.clone(), expand(value));
                }
            }
            if let Some(Json::Array(stream)) = fields.get("~") {
                let mut seen = std::collections::BTreeMap::<&str, usize>::new();
                let items = stream
                    .iter()
                    .map(|item| match item {
                        Json::String(text) => json!({
                            (ir::XML_NODE_NAME_FIELD): "",
                            (ir::XML_TEXT_FIELD): text,
                            (ir::XML_MIXED_CONTENT_VALUE_FIELD): text,
                        }),
                        Json::Object(element) => {
                            let (name, text) = element.iter().next().expect("one element");
                            let index = seen.entry(name).or_default();
                            let value = match output.get(name) {
                                Some(Json::Array(occurrences)) => occurrences.get(*index).cloned(),
                                Some(single) => Some(single.clone()),
                                None => None,
                            };
                            *index += 1;
                            json!({
                                (ir::XML_NODE_NAME_FIELD): name,
                                (ir::XML_TEXT_FIELD): text,
                                (ir::XML_MIXED_CONTENT_VALUE_FIELD): value.unwrap_or_else(|| text.clone()),
                            })
                        }
                        other => panic!("closed mixed stream item {other}"),
                    })
                    .collect();
                output.insert(ir::XML_MIXED_CONTENT_FIELD.into(), Json::Array(items));
            }
            Json::Object(output)
        }
        other => other.clone(),
    }
}

fn corpus() -> TestResult<Json> {
    let corpus: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-xml-mixed-content/cases.json"
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
    Ok(expand(&corpus))
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
        E::MappingException { node, message } => {
            json!({"kind": "MappingException", "node": node, "message": message})
        }
        E::UnknownTarget { name } => json!({"kind": "UnknownTarget", "name": name}),
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
    let execution = engine::ExecutionContext::new(Path::new("xml-mixed468.mapping"));
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

fn group_read(owner: Option<&str>, path: &[&str], frame: Option<&[&str]>) -> Request {
    Request::XmlElement {
        owner: diagnostic_owner(owner),
        path: path.iter().map(|part| (*part).into()).collect(),
        frame: frame.map(|frame| frame.iter().map(|part| (*part).into()).collect()),
    }
}

/// A closed context refusing one owned atomizer at its reached expression.
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

/// The first owned atomizer an older profile reaches.
fn older_refusal(model: &str) -> ProgramValidationError {
    const ROW: &[&str] = &["Docs", "Doc"];
    const ROW_PARA: &[&str] = &["Docs", "Doc", "Para"];
    match model {
        "M01-endpoints" => xml_operator(1, root(None, &[]), group_read(None, &["Para"], None)),
        "M02-nested" => xml_operator(30, root(None, &[0]), group_read(None, ROW_PARA, Some(ROW))),
        "M03-frames" => xml_operator(50, root(None, &[0]), group_read(None, &["Para"], Some(&[]))),
        "M04-lazy" => xml_operator(60, root(None, &[]), group_read(None, &["Para"], None)),
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
    let options = rust_options("xml-mixed468-refusal", &runtime_path()?);
    assert_eq!(
        codegen::validate_program_with_basic_owned_xml_mixed_content(program).as_ref(),
        Err(expected),
        "{label} validator"
    );
    assert_eq!(
        codegen::validate_basic_owned_xml_mixed_content_emission(program).as_ref(),
        Err(expected),
        "{label} emission validator"
    );
    assert!(
        matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content(program),
            Err(error) if error == *expected),
        "{label} catalogue"
    );
    assert!(
        matches!(codegen_rust::emit_with_basic_owned_xml_mixed_content(program, &options),
            Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
        "{label} Rust"
    );
    assert!(
        matches!(codegen_csharp::emit_with_basic_owned_xml_mixed_content(program),
            Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
        "{label} C#"
    );
    assert_eq!(program, &before, "{label}");
    Ok(())
}

#[test]
fn basic_xml_mixed468_interpreter_matches_expected() -> TestResult<()> {
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
fn basic_xml_mixed468_profile_lowers_validates_and_emits() -> TestResult<()> {
    let runtime = runtime_path()?;
    for (index, model) in projects::MODELS.iter().enumerate() {
        let project = projects::project(model);
        let before = mapping::project_file::encode_pretty(&project)?;
        assert!(before.contains("\"owned_xml_mixed_content\""), "{model}");
        let decoded = mapping::project_file::decode_str(&before)?;
        assert_eq!(mapping::project_file::encode_pretty(&decoded)?, before);
        let program = codegen::lower_with_basic_owned_xml_mixed_content(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        assert_eq!(mapping::project_file::encode_pretty(&project)?, before);
        let original = program.clone();
        codegen::validate_program_with_basic_owned_xml_mixed_content(&program)?;
        codegen::validate_basic_owned_xml_mixed_content_emission(&program)?;
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content(
            &program,
        )?;
        // Owned endpoints are never eagerly required legacy aliases.
        assert!(
            plan.all_outputs
                .entries
                .iter()
                .all(|entry| entry.availability == codegen::NamedInputAvailability::Optional),
            "{model}"
        );
        let rust = codegen_rust::emit_with_basic_owned_xml_mixed_content(
            &program,
            &rust_options(&format!("xml-mixed468-model{index}"), &runtime),
        )?;
        let csharp = codegen_csharp::emit_with_basic_owned_xml_mixed_content(&program)?;
        let text = |artifacts: &codegen::ArtifactSet| {
            artifacts
                .files()
                .iter()
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        assert!(
            text(&rust).contains("context.owned_xml_mixed_content("),
            "{model}"
        );
        assert!(
            text(&csharp).contains("context.OwnedXmlMixedContent("),
            "{model}"
        );
        assert_eq!(program, original, "{model}");
    }
    Ok(())
}

#[test]
fn basic_xml_mixed468_older_profiles_and_default_validation_refuse() -> TestResult<()> {
    let options = rust_options("xml-mixed468-old", &runtime_path()?);
    for model in projects::MODELS {
        let project = projects::project(model);
        let expected = older_refusal(model);
        let lowered = [
            codegen::lower(&project),
            codegen::lower_with_basic_owned_filters(&project),
            codegen::lower_with_basic_owned_sort(&project),
            codegen::lower_with_basic_owned_windows(&project),
            codegen::lower_with_basic_owned_row_failures(&project),
            codegen::lower_with_basic_owned_group_by(&project),
            codegen::lower_with_basic_owned_group_adjacent_by(&project),
            codegen::lower_with_basic_owned_group_starting_with(&project),
            codegen::lower_with_basic_owned_group_ending_with(&project),
            codegen::lower_with_basic_owned_failure_sources(&project),
            codegen::lower_with_basic_owned_sequence_item_at(&project),
            codegen::lower_with_basic_owned_path_hierarchy(&project),
            codegen::lower_with_basic_owned_adjacency_tree(&project),
            codegen::lower_with_basic_owned_group_into_blocks(&project),
            codegen::lower_with_basic_owned_recursive_collect(&project),
            codegen::lower_with_basic_owned_sequence_exists(&project),
            codegen::lower_with_basic_owned_sequence_aggregate(&project),
            codegen::lower_with_basic_owned_recursive_filter(&project),
            codegen::lower_with_basic_owned_lookup_find(&project),
            codegen::lower_with_basic_owned_dynamic_field(&project),
            codegen::lower_with_basic_owned_xml_serialize(&project),
            codegen::lower_with_basic_owned_collection_aggregates(&project),
            codegen::lower_with_basic_owned_joins(&project),
        ];
        let wanted = diagnostic(&expected);
        for (index, result) in lowered.iter().enumerate() {
            assert!(
                matches!(result, Err(error) if error.diagnostics() == std::slice::from_ref(&wanted)),
                "{model} lowering {index}: {result:?}"
            );
        }
        // Directly constructed Programs keep the same refusal on every older surface.
        let program = codegen::lower_with_basic_owned_xml_mixed_content(&project)?;
        let validators = [
            codegen::validate_program(&program),
            codegen::validate_owned_emission(&program),
            codegen::validate_program_with_basic_owned_filters(&program),
            codegen::validate_program_with_basic_owned_sort(&program),
            codegen::validate_program_with_basic_owned_windows(&program),
            codegen::validate_program_with_basic_owned_row_failures(&program),
            codegen::validate_basic_owned_emission(&program),
            codegen::validate_program_with_basic_owned_group_by(&program),
            codegen::validate_program_with_basic_owned_group_adjacent_by(&program),
            codegen::validate_program_with_basic_owned_group_starting_with(&program),
            codegen::validate_basic_owned_group_starting_with_emission(&program),
            codegen::validate_program_with_basic_owned_group_ending_with(&program),
            codegen::validate_basic_owned_group_ending_with_emission(&program),
            codegen::validate_program_with_basic_owned_sequence_item_at(&program),
            codegen::validate_program_with_basic_owned_failure_sources(&program),
            codegen::validate_basic_owned_failure_source_emission(&program),
            codegen::validate_program_with_basic_owned_path_hierarchy(&program),
            codegen::validate_basic_owned_path_hierarchy_emission(&program),
            codegen::validate_program_with_basic_owned_adjacency_tree(&program),
            codegen::validate_basic_owned_adjacency_tree_emission(&program),
            codegen::validate_program_with_basic_owned_group_into_blocks(&program),
            codegen::validate_basic_owned_group_into_blocks_emission(&program),
            codegen::validate_program_with_basic_owned_recursive_collect(&program),
            codegen::validate_basic_owned_recursive_collect_emission(&program),
            codegen::validate_program_with_basic_owned_sequence_exists(&program),
            codegen::validate_basic_owned_sequence_exists_emission(&program),
            codegen::validate_program_with_basic_owned_sequence_aggregate(&program),
            codegen::validate_basic_owned_sequence_aggregate_emission(&program),
            codegen::validate_program_with_basic_owned_recursive_filter(&program),
            codegen::validate_basic_owned_recursive_filter_emission(&program),
            codegen::validate_program_with_basic_owned_lookup_find(&program),
            codegen::validate_basic_owned_lookup_find_emission(&program),
            codegen::validate_program_with_basic_owned_dynamic_field(&program),
            codegen::validate_basic_owned_dynamic_field_emission(&program),
            codegen::validate_program_with_basic_owned_xml_serialize(&program),
            codegen::validate_basic_owned_xml_serialize_emission(&program),
            codegen::validate_program_with_basic_owned_collection_aggregates(&program),
            codegen::validate_basic_owned_collection_aggregates_emission(&program),
            codegen::validate_program_with_basic_owned_joins(&program),
            codegen::validate_basic_owned_joins_emission(&program),
        ];
        for (index, result) in validators.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(&expected), "{model} validator {index}");
        }
        let catalogues = [
            codegen::legacy_named_input_admission_plan(&program).map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_adjacency_tree(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_into_blocks(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_collect(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_exists(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(
                &program,
            )
            .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_recursive_filter(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_lookup_find(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_collection_aggregates(
                &program,
            )
            .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_joins(&program).map(|_| ()),
        ];
        for (index, result) in catalogues.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(&expected), "{model} catalogue {index}");
        }
        let rust = [
            codegen_rust::emit(&program, &options),
            codegen_rust::emit_with_basic_owned_sources(&program, &options),
            codegen_rust::emit_with_basic_owned_windows(&program, &options),
            codegen_rust::emit_with_basic_owned_group_ending_with(&program, &options),
            codegen_rust::emit_with_basic_owned_failure_sources(&program, &options),
            codegen_rust::emit_with_basic_owned_path_hierarchy(&program, &options),
            codegen_rust::emit_with_basic_owned_adjacency_tree(&program, &options),
            codegen_rust::emit_with_basic_owned_group_into_blocks(&program, &options),
            codegen_rust::emit_with_basic_owned_recursive_collect(&program, &options),
            codegen_rust::emit_with_basic_owned_sequence_exists(&program, &options),
            codegen_rust::emit_with_basic_owned_sequence_aggregate(&program, &options),
            codegen_rust::emit_with_basic_owned_recursive_filter(&program, &options),
            codegen_rust::emit_with_basic_owned_lookup_find(&program, &options),
            codegen_rust::emit_with_basic_owned_dynamic_field(&program, &options),
            codegen_rust::emit_with_basic_owned_xml_serialize(&program, &options),
            codegen_rust::emit_with_basic_owned_collection_aggregates(&program, &options),
            codegen_rust::emit_with_basic_owned_joins(&program, &options),
        ];
        for (index, result) in rust.iter().enumerate() {
            assert!(
                matches!(result, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
                "{model} Rust {index}: {result:?}"
            );
        }
        let csharp = [
            codegen_csharp::emit(&program),
            codegen_csharp::emit_with_basic_owned_sources(&program),
            codegen_csharp::emit_with_basic_owned_windows(&program),
            codegen_csharp::emit_with_basic_owned_group_ending_with(&program),
            codegen_csharp::emit_with_basic_owned_failure_sources(&program),
            codegen_csharp::emit_with_basic_owned_path_hierarchy(&program),
            codegen_csharp::emit_with_basic_owned_adjacency_tree(&program),
            codegen_csharp::emit_with_basic_owned_group_into_blocks(&program),
            codegen_csharp::emit_with_basic_owned_recursive_collect(&program),
            codegen_csharp::emit_with_basic_owned_sequence_exists(&program),
            codegen_csharp::emit_with_basic_owned_sequence_aggregate(&program),
            codegen_csharp::emit_with_basic_owned_recursive_filter(&program),
            codegen_csharp::emit_with_basic_owned_lookup_find(&program),
            codegen_csharp::emit_with_basic_owned_dynamic_field(&program),
            codegen_csharp::emit_with_basic_owned_xml_serialize(&program),
            codegen_csharp::emit_with_basic_owned_collection_aggregates(&program),
            codegen_csharp::emit_with_basic_owned_joins(&program),
        ];
        for (index, result) in csharp.iter().enumerate() {
            assert!(
                matches!(result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
                "{model} C# {index}: {result:?}"
            );
        }
    }
    Ok(())
}

/// The companion composes the latest companion chain: every owned XML
/// serialization, dynamic source-field and Lookup/CollectionFind model lowers,
/// validates and emits with it.
#[test]
fn basic_xml_mixed468_profile_composes_earlier_owned_companions() -> TestResult<()> {
    let runtime = runtime_path()?;
    let models = serialize_projects::MODELS
        .iter()
        .map(|model| (*model, serialize_projects::project(model)))
        .chain(
            dynamic_projects::PROFILES
                .iter()
                .map(|model| (*model, dynamic_projects::project(model))),
        )
        .chain(
            search_projects::PROFILES
                .iter()
                .map(|model| (*model, search_projects::project(model))),
        );
    for (model, project) in models {
        let program = codegen::lower_with_basic_owned_xml_mixed_content(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        codegen::validate_basic_owned_xml_mixed_content_emission(&program)?;
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content(&program)?;
        codegen_rust::emit_with_basic_owned_xml_mixed_content(
            &program,
            &rust_options("xml-mixed468-composed", &runtime),
        )?;
        codegen_csharp::emit_with_basic_owned_xml_mixed_content(&program)?;
    }
    Ok(())
}

#[test]
fn basic_xml_mixed468_direct_programs_refuse_unsupported_ownership() -> TestResult<()> {
    let base =
        codegen::lower_with_basic_owned_xml_mixed_content(&projects::project("M01-endpoints"))?;
    let with = |change: &dyn Fn(&mut mapping::OwnedXmlMixedContent)| {
        let mut program = base.clone();
        let node = program
            .expressions
            .iter_mut()
            .find(|node| node.id == 1)
            .expect("primary atomizer");
        let codegen::Expression::OwnedXmlMixedContent(mixed) = &mut node.expression else {
            unreachable!("authored owned atomizer");
        };
        change(mixed);
        program
    };
    let path = |segments: &[&str]| segments.iter().map(|part| (*part).into()).collect();
    let cases = [
        (
            "unknown owner",
            with(&|mixed| mixed.source.owner = projects::owner(Some("missing"))),
            structure(
                1,
                group_read(Some("missing"), &["Para"], None),
                Structure::UnknownOwner,
            ),
        ),
        (
            "empty owner",
            with(&|mixed| mixed.source.owner = projects::owner(Some(""))),
            structure(
                1,
                group_read(Some(""), &["Para"], None),
                Structure::InvalidNamedOwner,
            ),
        ),
        (
            "missing path",
            with(&|mixed| mixed.source.path = path(&["Docs", "Nope"])),
            structure(
                1,
                group_read(None, &["Docs", "Nope"], None),
                Structure::MissingOwnedPath { segment: 1 },
            ),
        ),
        (
            "frame outside path",
            with(&|mixed| mixed.frame = Some(path(&["Docs", "Doc"]))),
            structure(
                1,
                group_read(None, &["Para"], Some(&["Docs", "Doc"])),
                Structure::IncompatibleFramePath,
            ),
        ),
        (
            "frame not a collection",
            with(&|mixed| mixed.frame = Some(path(&["Para"]))),
            structure(
                1,
                group_read(None, &["Para"], Some(&["Para"])),
                Structure::InvalidOwnedCollection {
                    found: codegen::OwnedSourceSchemaShape::Group { repeating: false },
                },
            ),
        ),
        (
            "not a mixed group",
            with(&|mixed| mixed.source.path = path(&["Docs"])),
            ProgramValidationError::InvalidXmlMixedContentSource {
                node: 1,
                path: path(&["Docs"]),
            },
        ),
        (
            "occurrence of a single element",
            with(&|mixed| {
                mixed.replacements[0].element = "u".into();
            }),
            ProgramValidationError::InvalidXmlMixedContentCollection {
                node: 1,
                replacement: 0,
                collection: path(&["Para", "u"]),
            },
        ),
        (
            "empty element",
            with(&|mixed| mixed.replacements[1].element = String::new()),
            ProgramValidationError::EmptyXmlMixedContentElement {
                node: 1,
                replacement: 1,
            },
        ),
        (
            "duplicate element",
            with(&|mixed| mixed.replacements[1].element = "b".into()),
            ProgramValidationError::DuplicateXmlMixedContentElement {
                node: 1,
                element: "b".into(),
            },
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

/// A dynamic owner, failure-rule predicate, generated item and isolated
/// function body never admit the owned atomizer, in the interpreter or in
/// generation.
#[test]
fn basic_xml_mixed468_closed_contexts_refuse_before_emission() -> TestResult<()> {
    let lowered_error = |project: &Project, expected: &ProgramValidationError| {
        let lowered = codegen::lower_with_basic_owned_xml_mixed_content(project);
        assert!(
            matches!(&lowered, Err(error)
                if error.diagnostics() == std::slice::from_ref(&diagnostic(expected))),
            "{lowered:?}"
        );
    };
    let source = decode(&expand(
        &json!({"Title": "P", "Para": {"b": ["x"], "~": [{"b": "x"}]}}),
    ));

    // A dynamic named owner never supplies an exact owned group.
    let mut dynamic = projects::project("M01-endpoints");
    dynamic.graph.nodes.insert(
        90,
        Node::Const {
            value: Value::String("notes.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 90,
        iteration: vec!["Docs".into(), "Doc".into()],
    });
    lowered_error(
        &dynamic,
        &refused(
            2,
            Some(root(Some((0, "noted")), &[])),
            group_read(Some("notes"), &["Para"], None),
            Capability::DynamicOwner,
            Origin::DynamicSource {
                index: 0,
                name: "notes".into(),
            },
        ),
    );

    // A generated item context closes the owned read inside it.
    let mut generated = projects::project("M04-lazy");
    generated.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Item", vec![SchemaNode::scalar("Text", ScalarType::String)])
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
                target_field: "Text".into(),
                node: 60,
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
            60,
            Some(root(None, &[0])),
            group_read(None, &["Para"], None),
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

    // A failure-rule predicate is never a target context.
    let mut failure = projects::project("M04-lazy");
    failure.graph.nodes.insert(
        94,
        Node::Call {
            function: "exists".into(),
            args: vec![60],
        },
    );
    failure.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::WhenFalse { predicate: 94 },
        message: None,
    }];
    assert!(engine::validate(&failure).is_empty());
    lowered_error(
        &failure,
        &refused(
            60,
            None,
            group_read(None, &["Para"], None),
            Capability::FailureRule,
            Origin::Failure { index: 0 },
        ),
    );
    assert!(matches!(
        engine::run_with_sources(&failure, &source, vec![]),
        Err(engine::EngineError::UnsupportedOwnedContext {
            node: Some(60),
            context: engine::OwnedCapabilityContext::FailureRule,
            ..
        })
    ));

    // An isolated function body never reads an endpoint.
    let mut function = projects::project("M04-lazy");
    let id = FunctionId::new(1);
    function.user_functions.insert(
        id,
        UserFunction {
            library: "user".into(),
            name: "atomize".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(5, projects::mixed(None, &["Para"], None, &[]))].into(),
            },
            output: 5,
        },
    );
    function.graph.nodes.insert(
        60,
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
            kind: "owned_xml_mixed_content",
        })
    ));
    assert!(codegen::lower_with_basic_owned_xml_mixed_content(&function).is_err());
    Ok(())
}

#[test]
fn basic_xml_mixed468_legacy_mixed_content_and_errors_are_unchanged() -> TestResult<()> {
    let legacy = projects::legacy();
    let wire = mapping::project_file::encode_pretty(&legacy)?;
    assert!(wire.contains("\"xml_mixed_content\"") && !wire.contains("owned"));
    let decoded = mapping::project_file::decode_str(&wire)?;
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, wire);
    // Ordinary generation keeps the legacy route.
    let program = codegen::lower(&legacy)?;
    codegen_rust::emit(
        &program,
        &rust_options("xml-mixed468-legacy", &runtime_path()?),
    )?;
    codegen_csharp::emit(&program)?;
    // The owned companion keeps the same legacy refusal as earlier companions.
    let owned = codegen::lower_with_basic_owned_xml_mixed_content(&legacy);
    let serialize = codegen::lower_with_basic_owned_xml_serialize(&legacy);
    assert_eq!(
        owned.err().map(|error| error.diagnostics().to_vec()),
        serialize.err().map(|error| error.diagnostics().to_vec())
    );
    let para = expand(&json!({"Para": {"b": ["x", "y"], "~": ["a", {"b": "x"}, {"b": "y"}]}}));
    assert_eq!(
        encode(&engine::run(&legacy, &decode(&para))?),
        json!({"Primary": "axy"})
    );
    // Legacy outward fallback still reaches the root of the named endpoint
    // spelled like a missing primary group, where the owned form refuses.
    let alias = decode(&json!({"Title": "alias root"}));
    assert_eq!(
        encode(&engine::run_with_sources(
            &legacy,
            &decode(&json!({})),
            vec![("Para".into(), alias)]
        )?),
        json!({"Primary": null})
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
fn basic_xml_mixed468_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let corpus = corpus()?;
    let controls = controls(&corpus);
    let runtime = runtime_path()?;
    let temp = TempDir::new("xml_mixed468_hosts")?;
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
        let program = codegen::lower_with_basic_owned_xml_mixed_content(&projects::project(model))?;
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_xml_mixed_content(
                &program,
                &rust_options(&format!("xml-mixed468-model{index}"), &runtime),
            )?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_xml_mixed_content(&program)?,
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"xml-mixed468-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "                {index} => invoke!(model{index}, selection, &source, &inputs, &context, limits),\n"
        ));
    }
    let host = include_str!("basic_owned_xml_mixed_content/Host.rs.txt");
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
            "[package]\nname=\"xml-mixed468-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
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
        let run = Command::new(target.join("debug/xml-mixed468-host"))
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
            include_str!("basic_owned_xml_mixed_content/Host.cs.txt"),
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
