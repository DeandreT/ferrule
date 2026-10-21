//! #465 owned path-hierarchy parity: the interpreter, generated Rust and
//! generated C# run the same conformance corpus against its expected results.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic as OwnedDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceStructureReason as Structure, ProgramValidationError, TargetConstruction,
};
use mapping::{SourceOwner, SourceScopeLocation, SourceScopeStep, SourceScopeTarget};
use serde_json::{Value as Json, json};
use std::fs;

#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-path-hierarchy/projects.rs"]
mod projects;

const CASES: usize = 21;
const CALLS: usize = 76;

fn corpus() -> TestResult<Json> {
    let corpus: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-path-hierarchy/cases.json"
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
        Instance::Scalar(Value::Bool(value)) => json!(value),
        Instance::Scalar(Value::Int(value)) => json!(value),
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

fn engine_error(error: &engine::EngineError) -> Json {
    use engine::EngineError as E;
    match error {
        E::PathHierarchyRootCount { count } => {
            json!({"kind": "PathHierarchyRootCount", "count": count})
        }
        E::PathHierarchyValueType { found } => {
            json!({"kind": "PathHierarchyValueType", "found": found})
        }
        E::PathHierarchyDepth { limit } => json!({"kind": "PathHierarchyDepth", "limit": limit}),
        E::PathHierarchyTooLarge { max } => json!({"kind": "PathHierarchyTooLarge", "max": max}),
        E::UnknownTarget { name } => json!({"kind": "UnknownTarget", "name": name}),
        E::MappingFailure { rule, message } => {
            json!({"kind": "MappingFailure", "rule": rule, "message": message})
        }
        E::OwnedSourceAccess {
            source:
                engine::OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named { name },
                },
            ..
        } => json!({"kind": "SourceUnavailable", "owner": name}),
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
    let execution = engine::ExecutionContext::new(Path::new("path-hierarchy465.mapping"));
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

fn request(owner: Option<&str>, path: &[&str]) -> Request {
    Request::Iteration {
        owner: match owner {
            None => Owner::Primary,
            Some(name) => Owner::Named { name: name.into() },
        },
        path: path.iter().map(|part| (*part).into()).collect(),
    }
}

fn capability(
    scope: SourceScopeLocation,
    request: Request,
    reason: Capability,
    origin: Origin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedDiagnostic::Capability {
            location: Location::Iteration { scope },
            request: Some(request),
            reason,
            origin,
        }),
    }
}

fn structure(request: Request, reason: Structure) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedDiagnostic::Structure {
            location: Location::Iteration {
                scope: root(None, &[]),
            },
            request,
            reason,
        }),
    }
}

/// The first owned hierarchy each older profile reaches, as a TreeOperator refusal.
fn tree_operator(model: &str) -> ProgramValidationError {
    let (scope, request) = match model {
        "P01-endpoints" => (root(None, &[]), request(None, &["Files", "File"])),
        "P02-nested" => (root(None, &[0, 0]), request(None, &["Batch", "File"])),
        "P03-separator" => (root(None, &[]), request(Some("audit"), &["Batch", "File"])),
        "P04-root-collection" => (root(None, &[]), request(Some("paths"), &[])),
        "P05-failure-walk" => (root(None, &[]), request(Some("audit"), &["Files", "File"])),
        _ => unreachable!("authored model"),
    };
    capability(
        scope.clone(),
        request,
        Capability::TreeOperator,
        Origin::Scope { scope },
    )
}

fn diagnostic(error: &ProgramValidationError) -> codegen::Diagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        unreachable!("owned refusal");
    };
    codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    }
}

/// Every complete profile surface for one directly constructed Program.
fn assert_new_profile_refuses(
    program: &codegen::Program,
    expected: &ProgramValidationError,
    label: &str,
) -> TestResult<()> {
    let before = program.clone();
    let options = rust_options("path-hierarchy465-refusal", &runtime_path()?);
    assert_eq!(
        codegen::validate_program_with_basic_owned_path_hierarchy(program).as_ref(),
        Err(expected),
        "{label} validator"
    );
    assert_eq!(
        codegen::validate_basic_owned_path_hierarchy_emission(program).as_ref(),
        Err(expected),
        "{label} emission validator"
    );
    assert!(
        matches!(codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(program),
            Err(error) if error == *expected),
        "{label} catalogue"
    );
    assert!(
        matches!(codegen_rust::emit_with_basic_owned_path_hierarchy(program, &options),
            Err(codegen_rust::EmitError::InvalidProgram(error)) if error == *expected),
        "{label} Rust"
    );
    assert!(
        matches!(codegen_csharp::emit_with_basic_owned_path_hierarchy(program),
            Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == *expected),
        "{label} C#"
    );
    assert_eq!(program, &before, "{label}");
    Ok(())
}

#[test]
fn basic_path_hierarchy465_interpreter_matches_expected() -> TestResult<()> {
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
fn basic_path_hierarchy465_profile_lowers_validates_and_emits() -> TestResult<()> {
    let runtime = runtime_path()?;
    for (index, model) in projects::MODELS.iter().enumerate() {
        let project = projects::project(model);
        let before = mapping::project_file::encode_pretty(&project)?;
        assert!(before.contains("\"owned_path_hierarchy\""), "{model}");
        let decoded = mapping::project_file::decode_str(&before)?;
        assert_eq!(mapping::project_file::encode_pretty(&decoded)?, before);
        let program = codegen::lower_with_basic_owned_path_hierarchy(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        assert_eq!(mapping::project_file::encode_pretty(&project)?, before);
        let original = program.clone();
        codegen::validate_program_with_basic_owned_path_hierarchy(&program)?;
        codegen::validate_basic_owned_path_hierarchy_emission(&program)?;
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_path_hierarchy(&program)?;
        // Owned endpoints are never eagerly required legacy aliases.
        assert!(
            plan.all_outputs
                .entries
                .iter()
                .all(|entry| entry.availability == codegen::NamedInputAvailability::Optional),
            "{model}"
        );
        let rust = codegen_rust::emit_with_basic_owned_path_hierarchy(
            &program,
            &rust_options(&format!("path-hierarchy465-model{index}"), &runtime),
        )?;
        let csharp = codegen_csharp::emit_with_basic_owned_path_hierarchy(&program)?;
        let text = |artifacts: &codegen::ArtifactSet| {
            artifacts
                .files()
                .iter()
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        assert!(
            text(&rust).contains("codegen_runtime::owned_path_hierarchy("),
            "{model}"
        );
        assert!(
            text(&csharp).contains("FerrulePathHierarchy.BuildOwned("),
            "{model}"
        );
        assert_eq!(program, original, "{model}");
    }
    Ok(())
}

#[test]
fn basic_path_hierarchy465_older_profiles_and_default_validation_refuse() -> TestResult<()> {
    let options = rust_options("path-hierarchy465-old", &runtime_path()?);
    for model in projects::MODELS {
        let project = projects::project(model);
        let expected = tree_operator(model);
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
        ];
        for (index, result) in lowered.iter().enumerate() {
            assert!(
                matches!(result, Err(error)
                    if error.diagnostics() == std::slice::from_ref(&diagnostic(&expected))),
                "{model} lowering {index}: {result:?}"
            );
        }
        // Directly constructed Programs keep the same refusal on every older surface.
        let program = codegen::lower_with_basic_owned_path_hierarchy(&project)?;
        let validators = [
            codegen::validate_program(&program),
            codegen::validate_owned_emission(&program),
            codegen::validate_program_with_basic_owned_filters(&program),
            codegen::validate_program_with_basic_owned_sort(&program),
            codegen::validate_program_with_basic_owned_windows(&program),
            codegen::validate_program_with_basic_owned_row_failures(&program),
            codegen::validate_program_with_basic_owned_group_by(&program),
            codegen::validate_program_with_basic_owned_group_adjacent_by(&program),
            codegen::validate_program_with_basic_owned_group_starting_with(&program),
            codegen::validate_program_with_basic_owned_sequence_item_at(&program),
            codegen::validate_basic_owned_emission(&program),
            codegen::validate_basic_owned_group_starting_with_emission(&program),
            codegen::validate_program_with_basic_owned_group_ending_with(&program),
            codegen::validate_basic_owned_group_ending_with_emission(&program),
            codegen::validate_program_with_basic_owned_failure_sources(&program),
            codegen::validate_basic_owned_failure_source_emission(&program),
        ];
        for (index, result) in validators.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(&expected), "{model} validator {index}");
        }
        let catalogues = [
            codegen::legacy_named_input_admission_plan(&program).map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_starting_with(
                &program,
            )
            .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_group_ending_with(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_failure_sources(&program)
                .map(|_| ()),
        ];
        for (index, result) in catalogues.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(&expected), "{model} catalogue {index}");
        }
        let rust = [
            codegen_rust::emit(&program, &options),
            codegen_rust::emit_with_basic_owned_sources(&program, &options),
            codegen_rust::emit_with_basic_owned_windows(&program, &options),
            codegen_rust::emit_with_basic_owned_group_starting_with(&program, &options),
            codegen_rust::emit_with_basic_owned_group_ending_with(&program, &options),
            codegen_rust::emit_with_basic_owned_failure_sources(&program, &options),
            codegen_rust::emit_with_basic_owned_sequence_item_at(&program, &options),
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
            codegen_csharp::emit_with_basic_owned_group_starting_with(&program),
            codegen_csharp::emit_with_basic_owned_group_ending_with(&program),
            codegen_csharp::emit_with_basic_owned_failure_sources(&program),
            codegen_csharp::emit_with_basic_owned_sequence_item_at(&program),
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

/// Directly constructed Programs with unsupported ownership and their refusals.
fn direct_programs() -> TestResult<Vec<(&'static str, codegen::Program, ProgramValidationError)>> {
    let base = codegen::lower_with_basic_owned_path_hierarchy(&projects::project("P01-endpoints"))?;
    let with = |owner: Option<&str>, path: &[&str], files: &str| {
        let mut program = base.clone();
        program.root.construction = TargetConstruction::OwnedPathHierarchy {
            collection: mapping::OwnedSourceRef {
                owner: projects::owner(owner),
                path: path.iter().map(|part| (*part).into()).collect(),
            },
            separator: "/".into(),
            directories: "directory".into(),
            files: files.into(),
            name: "name".into(),
        };
        program
    };
    let cases = [
        (
            "unknown owner",
            with(Some("missing"), &["Files", "File"], "file"),
            structure(
                request(Some("missing"), &["Files", "File"]),
                Structure::UnknownOwner,
            ),
        ),
        (
            "empty owner",
            with(Some(""), &["Files", "File"], "file"),
            structure(
                request(Some(""), &["Files", "File"]),
                Structure::InvalidNamedOwner,
            ),
        ),
        (
            "missing path",
            with(Some("audit"), &["Files", "Nope"], "file"),
            structure(
                request(Some("audit"), &["Files", "Nope"]),
                Structure::MissingOwnedPath { segment: 1 },
            ),
        ),
        (
            "non-repeating scalar",
            with(Some("audit"), &["Single"], "file"),
            ProgramValidationError::InvalidPathHierarchyCollection {
                target_path: vec![],
                collection: vec!["Single".into()],
            },
        ),
        (
            "group collection",
            with(None, &["Batch"], "file"),
            ProgramValidationError::InvalidPathHierarchyCollection {
                target_path: vec![],
                collection: vec!["Batch".into()],
            },
        ),
        (
            "output structure",
            with(None, &["Files", "File"], "directory"),
            ProgramValidationError::InvalidPathHierarchyConstruction {
                target_path: vec![],
            },
        ),
    ];
    Ok(cases.into())
}

#[test]
fn basic_path_hierarchy465_direct_programs_refuse_unsupported_ownership() -> TestResult<()> {
    let cases = direct_programs()?;
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

/// Projects whose owned collection sits in a closed context.
fn closed_context_projects() -> [(&'static str, Project); 2] {
    let mut dynamic = projects::project("P01-endpoints");
    dynamic.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("audit.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path =
        Some(mapping::DynamicSourcePath::legacy(0, vec!["Batch".into()]));
    let mut generated = projects::base(
        SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Tree", vec![projects::directory("Tree")]).repeating()],
        ),
        Scope {
            children: vec![Scope {
                target_field: "Tree".into(),
                iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
                    input: 0,
                    delimiter: 1,
                    item: 2,
                }),
                children: vec![Scope {
                    target_field: "Tree".into(),
                    ..projects::hierarchy(projects::owner(None), &["Files", "File"], "/")
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    generated.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    [
        ("closed-context/dynamic", dynamic),
        ("closed-context/generated", generated),
    ]
}

#[test]
fn basic_path_hierarchy465_closed_contexts_refuse_before_emission() -> TestResult<()> {
    let [(_, dynamic), (_, generated)] = closed_context_projects();
    // A dynamic named owner never supplies an exact owned collection.
    let expected = capability(
        root(Some((0, "audit")), &[]),
        request(Some("audit"), &["Files", "File"]),
        Capability::DynamicOwner,
        Origin::DynamicSource {
            index: 0,
            name: "audit".into(),
        },
    );
    let lowered = codegen::lower_with_basic_owned_path_hierarchy(&dynamic);
    assert!(
        matches!(&lowered, Err(error)
            if error.diagnostics() == std::slice::from_ref(&diagnostic(&expected))),
        "{lowered:?}"
    );

    // A generated item context closes the owned read inside it.
    assert!(engine::validate(&generated).is_empty());
    let expected = capability(
        root(None, &[0, 0]),
        request(None, &["Files", "File"]),
        Capability::GeneratedItem,
        Origin::Scope {
            scope: root(None, &[0]),
        },
    );
    let lowered = codegen::lower_with_basic_owned_path_hierarchy(&generated);
    assert!(
        matches!(&lowered, Err(error)
            if error.diagnostics() == std::slice::from_ref(&diagnostic(&expected))),
        "{lowered:?}"
    );
    assert!(matches!(
        engine::run_with_sources(
            &generated,
            &decode(&json!({"Files": {"File": ["r/a"]}})),
            vec![]
        ),
        Err(engine::EngineError::UnsupportedOwnedContext {
            context: engine::OwnedCapabilityContext::GeneratedItem,
            ..
        })
    ));
    Ok(())
}

#[test]
fn basic_path_hierarchy465_legacy_plan_and_errors_are_unchanged() -> TestResult<()> {
    let legacy = projects::legacy();
    let wire = mapping::project_file::encode_pretty(&legacy)?;
    assert!(wire.contains("\"path_hierarchy\"") && !wire.contains("owned"));
    let decoded = mapping::project_file::decode_str(&wire)?;
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, wire);
    // Ordinary generation keeps the legacy route.
    let program = codegen::lower(&legacy)?;
    codegen_rust::emit(
        &program,
        &rust_options("path-hierarchy465-legacy", &runtime_path()?),
    )?;
    codegen_csharp::emit(&program)?;
    // The owned companion keeps the same legacy refusal as earlier companions.
    let owned = codegen::lower_with_basic_owned_path_hierarchy(&legacy);
    let ending = codegen::lower_with_basic_owned_failure_sources(&legacy);
    assert!(owned.is_err());
    assert_eq!(
        owned.err().map(|error| error.diagnostics().to_vec()),
        ending.err().map(|error| error.diagnostics().to_vec())
    );
    // Legacy outward fallback and its typed errors are unchanged.
    let source = decode(&json!({"Files": {"File": ["one/a", "two/b"]}}));
    assert_eq!(
        engine::run(&legacy, &source),
        Err(engine::EngineError::PathHierarchyRootCount { count: 2 })
    );
    // The legacy collection's first segment may name an endpoint root.
    let alias = decode(&json!({"File": ["f/a"]}));
    assert_eq!(
        encode(&engine::run_with_sources(
            &legacy,
            &decode(&json!({})),
            vec![("Files".into(), alias)]
        )?),
        json!({"file": [{"name": "a"}], "directory": [], "name": "f"})
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
fn basic_path_hierarchy465_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let corpus = corpus()?;
    let controls = controls(&corpus);
    let runtime = runtime_path()?;
    let temp = TempDir::new("path_hierarchy465_hosts")?;
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
        let program = codegen::lower_with_basic_owned_path_hierarchy(&projects::project(model))?;
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_path_hierarchy(
                &program,
                &rust_options(&format!("path-hierarchy465-model{index}"), &runtime),
            )?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_path_hierarchy(&program)?,
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"path-hierarchy465-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "                {index} => invoke!(model{index}, selection, &source, &inputs, &context, limits),\n"
        ));
    }
    let host = include_str!("basic_owned_path_hierarchy/Host.rs.txt");
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
            "[package]\nname=\"path-hierarchy465-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
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
        let run = Command::new(target.join("debug/path-hierarchy465-host"))
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
            include_str!("basic_owned_path_hierarchy/Host.cs.txt"),
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

/// The models, the legacy project, the closed-context Projects and the
/// directly constructed Programs, for the owned capability preset record.
pub(super) fn capability_corpus() -> TestResult<super::owned_capability_presets::Corpus> {
    use super::owned_capability_presets as presets;
    let mut corpus = presets::projects(
        projects::MODELS
            .iter()
            .map(|model| (*model, projects::project(model))),
    );
    corpus.extend(presets::projects([("legacy", projects::legacy())]));
    corpus.extend(presets::projects(closed_context_projects()));
    corpus.extend(presets::programs(
        direct_programs()?
            .into_iter()
            .map(|(id, program, _)| (format!("direct/{id}"), program)),
    ));
    Ok(corpus)
}
