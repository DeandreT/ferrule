//! #449 owned join tuple and #450 owned joined aggregate parity: the
//! interpreter, generated Rust and generated C# run the same conformance
//! corpus against its expectations.
use super::*;
use serde_json::{Value as Json, json};
use std::fs;
// These modules are the conformance corpus. Keep their bodies byte-identical to
// the authored files when formatting this harness.
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-joins/projects.rs"]
mod projects;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-joins/cases.rs"]
mod cases;
#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-joins/closed.rs"]
mod closed;
use super::basic_owned_sequence_item_at::codec;

const MODELS: usize = 11;
const CASES: usize = 27;
const CALLS: usize = 3 * CASES;

pub(super) struct Case {
    id: &'static str,
    profile: &'static str,
    source: Instance,
    inputs: Vec<(String, Instance)>,
    calls: Vec<Json>,
}

/// Controls JSON shared by every engine, plus the Project for each model.
fn corpus() -> TestResult<(Json, Vec<Project>)> {
    let models: Vec<Project> = projects::PROFILES
        .iter()
        .map(|profile| projects::project(profile))
        .collect();
    let cases = cases::cases();
    let mut ids = std::collections::BTreeSet::new();
    let mut used = std::collections::BTreeSet::new();
    let mut records = Vec::new();
    for case in &cases {
        let model = projects::PROFILES
            .iter()
            .position(|profile| *profile == case.profile)
            .ok_or_else(|| format!("unknown profile {}", case.profile))?;
        if !ids.insert(case.id) || case.calls.len() != 3 {
            return Err(format!("duplicate case or call set: {}", case.id).into());
        }
        used.insert(model);
        records.push(json!({"id":case.id,"model":model,"source":codec::encode(&case.source),
            "inputs":case.inputs.iter().map(|(name,value)|json!([name,codec::encode(value)])).collect::<Vec<_>>(),
            "calls":case.calls}));
    }
    if models.len() != MODELS || used.len() != MODELS || cases.len() != CASES {
        return Err("11 profiles with 27 cases changed".into());
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

fn availability(plan: &codegen::NamedInputCatalogue<'_>) -> Vec<(String, String)> {
    plan.entries
        .iter()
        .map(|entry| (entry.name.to_owned(), format!("{:?}", entry.availability)))
        .collect()
}

/// Owned join scopes in this scope tree.
fn join_scopes(scope: &Scope) -> usize {
    usize::from(scope.iteration.owned_join().is_some())
        + scope.children.iter().map(join_scopes).sum::<usize>()
}

fn join_reads(project: &Project) -> usize {
    project
        .graph
        .nodes
        .values()
        .filter(|node| {
            matches!(
                node,
                Node::OwnedJoinField(_) | Node::OwnedJoinSourcePosition(_)
            )
        })
        .count()
}

/// Joined aggregates reachable from a target, each rendered once.
fn joined_aggregates(program: &codegen::Program) -> usize {
    program
        .expressions
        .iter()
        .filter(|node| matches!(node.expression, codegen::Expression::OwnedJoinAggregate(_)))
        .count()
}

#[test]
fn owned_joins449_profiles_lower_validate_and_emit() -> TestResult<()> {
    let (_, models) = corpus()?;
    let runtime = runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let profile = projects::PROFILES[index];
        let context = |error: &dyn std::fmt::Debug| format!("{profile}: {error:?}");
        assert!(
            engine::validate(project).is_empty(),
            "{profile}: {:?}",
            engine::validate(project)
        );
        let wire = mapping::project_file::encode_pretty(project)?;
        assert_eq!(
            wire.matches("\"owned_join_field\"").count()
                + wire.matches("\"owned_join_source_position\"").count(),
            join_reads(project),
            "{profile}"
        );
        assert_eq!(
            wire.matches("\"owned_join_aggregate\"").count(),
            project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(node, Node::OwnedJoinAggregate(_)))
                .count(),
            "{profile}"
        );
        let decoded = mapping::project_file::decode_str(&wire)?;
        assert_eq!(
            mapping::project_file::encode_pretty(&decoded)?,
            wire,
            "{profile}"
        );
        let program = codegen::lower_with_basic_owned_joins(project).map_err(|e| context(&e))?;
        let original = program.clone();
        codegen::validate_program_with_basic_owned_joins(&program).map_err(|e| context(&e))?;
        codegen::validate_basic_owned_joins_emission(&program).map_err(|e| context(&e))?;
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_joins(&program)
            .map_err(|e| context(&e))?;
        // Owned join sources are demand-driven, so no endpoint becomes eager.
        let expected = vec![("Lookup".to_owned(), "Optional".to_owned())];
        assert_eq!(availability(&plan.all_outputs), expected, "{profile}");
        assert_eq!(availability(&plan.primary), expected, "{profile}");
        assert_eq!(
            availability(&plan.named_targets[0].catalogue),
            expected,
            "{profile}"
        );
        let rust = codegen_rust::emit_with_basic_owned_joins(
            &program,
            &rust_options(&format!("joins449-model{index}"), &runtime),
        )
        .map_err(|e| context(&e))?;
        let csharp =
            codegen_csharp::emit_with_basic_owned_joins(&program).map_err(|e| context(&e))?;
        let text = |set: &codegen::ArtifactSet, suffix: &str| {
            set.files()
                .iter()
                .filter(|file| file.path.as_str().ends_with(suffix))
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        // Each owned join scope and joined aggregate renders exactly one owned join.
        let scopes = std::iter::once(&project.root)
            .chain(project.extra_targets.iter().map(|target| &target.root))
            .map(join_scopes)
            .sum::<usize>()
            + joined_aggregates(&program);
        assert_eq!(
            text(&rust, "lib.rs")
                .matches("context.owned_inner_join(")
                .count(),
            scopes,
            "{profile}"
        );
        assert_eq!(
            text(&csharp, "GeneratedMapping.cs")
                .matches("context.OwnedInnerJoin(")
                .count(),
            scopes,
            "{profile}"
        );
        assert_eq!(program, original, "{profile}");
    }
    Ok(())
}

fn engine_error_matches(error: &engine::EngineError, recipe: &Json) -> bool {
    match recipe["kind"].as_str() {
        Some("NotABool") => {
            *error
                == engine::EngineError::NotABool {
                    node: recipe["node"].as_u64().unwrap_or_default() as u32,
                    found: match recipe["found"].as_str() {
                        Some("string") => "string",
                        _ => panic!("authored found kind"),
                    },
                }
        }
        // Scope-level walk failures carry no node; a joined aggregate's carry its own.
        Some("OwnedSource") => match recipe["node"].as_u64() {
            None => matches!(
                error,
                engine::EngineError::OwnedSourceAccess {
                    node: None,
                    scope: Some(_),
                    source: engine::OwnedSourceError::SourceUnavailable { owner },
                } if *owner == projects::owner(recipe["owner"].as_str().unwrap_or_default())
            ),
            Some(node) => matches!(
                error,
                engine::EngineError::OwnedSourceAccess {
                    node: Some(found),
                    scope: None,
                    source: engine::OwnedSourceError::SourceUnavailable { owner },
                } if u64::from(*found) == node
                    && *owner == projects::owner(recipe["owner"].as_str().unwrap_or_default())
            ),
        },
        Some("AggregateNonFinite") => {
            *error
                == engine::EngineError::AggregateNonFinite {
                    function: match recipe["function"].as_str() {
                        Some("Sum") => mapping::AggregateOp::Sum,
                        _ => panic!("authored aggregate function"),
                    },
                }
        }
        Some("MappingException") => {
            *error
                == engine::EngineError::MappingException {
                    node: recipe["node"].as_u64().unwrap_or_default() as u32,
                    message: recipe["message"].as_str().map(str::to_owned),
                }
        }
        _ => panic!("authored error recipe"),
    }
}

#[test]
fn owned_joins449_interpreter_matches_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let execution = engine::ExecutionContext::new(Path::new("joins449.mapping"));
    let mut differing = Vec::new();
    let mut calls = 0;
    for case in controls["cases"].as_array().ok_or("cases")? {
        let project = &models[case["model"].as_u64().ok_or("model")? as usize];
        let source = codec::decode(&case["source"]);
        let inputs: Vec<(String, Instance)> = case["inputs"]
            .as_array()
            .ok_or("inputs")?
            .iter()
            .map(|input| {
                (
                    input[0].as_str().unwrap_or_default().to_owned(),
                    codec::decode(&input[1]),
                )
            })
            .collect();
        for call in case["calls"].as_array().ok_or("calls")? {
            calls += 1;
            let selection = call["selection"].as_str().ok_or("selection")?;
            let actual = if selection == "All" {
                engine::run_outputs_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                )
                .map(|outputs| {
                    std::iter::once(json!([null, codec::encode(&outputs.primary)]))
                        .chain(
                            outputs
                                .extras
                                .iter()
                                .map(|named| json!([named.name, codec::encode(&named.instance)])),
                        )
                        .collect::<Vec<_>>()
                })
            } else {
                let target = if selection == "Primary" {
                    engine::TargetSelection::Primary
                } else {
                    engine::TargetSelection::Named(selection)
                };
                engine::run_selected_target_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                    target,
                )
                .map(|output| match output {
                    engine::SelectedTargetOutput::Primary(instance) => {
                        vec![json!([null, codec::encode(&instance)])]
                    }
                    engine::SelectedTargetOutput::Named(named) => {
                        vec![json!([named.name, codec::encode(&named.instance)])]
                    }
                })
            };
            let matched = match (&actual, call["expected"].get("Ok")) {
                (Ok(trees), Some(expected)) => json!(trees) == *expected,
                (Err(error), None) => engine_error_matches(error, &call["expected"]["Err"]),
                _ => false,
            };
            if !matched {
                differing.push(format!(
                    "{} {selection}: {actual:?}\nexpected {}",
                    case["id"], call["expected"]
                ));
            }
        }
    }
    assert_eq!(calls, CALLS);
    assert!(differing.is_empty(), "{differing:#?}");
    Ok(())
}

#[test]
fn owned_joins449_older_profiles_and_closed_controls_refuse() -> TestResult<()> {
    // Every earlier generation profile keeps refusing owned joins.
    let project = projects::project("J01-same-path-rows");
    let program = codegen::lower_with_basic_owned_joins(&project)?;
    let expected = closed::join_closed();
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = &expected else {
        unreachable!("authored owned refusal");
    };
    let diagnostic = codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    };
    for result in closed::old_lower(&project) {
        assert!(
            matches!(&result, Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic)),
            "{result:?}"
        );
    }
    for result in closed::old_validators(&program)
        .into_iter()
        .chain(closed::old_catalogues(&program))
    {
        assert_eq!(result, Err(expected.clone()));
    }
    let options = rust_options("joins449-old", &runtime_path()?);
    for result in closed::old_rust(&program, &options) {
        assert!(
            matches!(&result, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
            "{result:?}"
        );
    }
    for result in closed::old_csharp(&program) {
        assert!(
            matches!(&result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
            "{result:?}"
        );
    }
    assert!(
        closed::ordinary_refusals(&program, &options)
            .iter()
            .all(|refused| *refused)
    );

    // Unsupported owned join Projects refuse before artifacts exist.
    for (id, project, refusal) in closed::projects() {
        let actual = codegen::lower_with_basic_owned_joins(&project);
        let matched = match (&actual, &refusal) {
            (Err(error), closed::Refusal::Capability(reason)) => {
                closed::reason(error.diagnostics()) == Some(*reason)
            }
            (Err(error), closed::Refusal::Validation(expected)) => {
                error.diagnostics() == expected.as_slice()
            }
            _ => false,
        };
        assert!(matched, "{id}: {actual:?}");
    }

    // Directly constructed Programs are refused by every new-profile entry.
    for (id, program, expected) in closed::programs(&program) {
        assert_eq!(
            codegen::validate_program_with_basic_owned_joins(&program),
            Err(expected.clone()),
            "{id}"
        );
        assert_eq!(
            codegen::validate_basic_owned_joins_emission(&program),
            Err(expected.clone()),
            "{id}"
        );
        assert_eq!(
            codegen::legacy_named_input_admission_plan_with_basic_owned_joins(&program).map(drop),
            Err(expected.clone()),
            "{id}"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_joins(&program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_joins(&program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == expected),
            "{id} C#"
        );
    }
    Ok(())
}

#[test]
fn owned_joins450_older_profiles_and_closed_aggregates_refuse() -> TestResult<()> {
    // Every earlier generation profile refuses owned joined aggregates.
    let project = projects::project("J08-joined-reducers");
    let program = codegen::lower_with_basic_owned_joins(&project)?;
    let expected = closed::aggregate_closed();
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = &expected else {
        unreachable!("authored owned refusal");
    };
    let diagnostic = codegen::Diagnostic::OwnedSource {
        diagnostic: diagnostic.clone(),
    };
    for result in closed::old_lower(&project) {
        assert!(
            matches!(&result, Err(error) if error.diagnostics() == std::slice::from_ref(&diagnostic)),
            "{result:?}"
        );
    }
    for result in closed::old_validators(&program)
        .into_iter()
        .chain(closed::old_catalogues(&program))
    {
        assert_eq!(result, Err(expected.clone()));
    }
    let options = rust_options("joins450-old", &runtime_path()?);
    for result in closed::old_rust(&program, &options) {
        assert!(
            matches!(&result, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
            "{result:?}"
        );
    }
    for result in closed::old_csharp(&program) {
        assert!(
            matches!(&result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
            "{result:?}"
        );
    }
    assert!(
        closed::ordinary_refusals(&program, &options)
            .iter()
            .all(|refused| *refused)
    );

    // Unsupported joined aggregate Projects refuse before artifacts exist.
    for (id, project, refusal) in closed::aggregate_projects() {
        let actual = codegen::lower_with_basic_owned_joins(&project);
        let matched = match (&actual, &refusal) {
            (Err(error), closed::Refusal::Capability(reason)) => {
                closed::reason(error.diagnostics()) == Some(*reason)
            }
            (Err(error), closed::Refusal::Validation(expected)) => {
                error.diagnostics() == expected.as_slice()
            }
            _ => false,
        };
        assert!(matched, "{id}: {actual:?}");
    }

    // Directly constructed Programs are refused by every new-profile entry.
    for (id, program, expected) in closed::aggregate_programs(&program) {
        assert_eq!(
            codegen::validate_program_with_basic_owned_joins(&program),
            Err(expected.clone()),
            "{id}"
        );
        assert_eq!(
            codegen::validate_basic_owned_joins_emission(&program),
            Err(expected.clone()),
            "{id}"
        );
        assert_eq!(
            codegen::legacy_named_input_admission_plan_with_basic_owned_joins(&program).map(drop),
            Err(expected.clone()),
            "{id}"
        );
        assert!(
            matches!(codegen_rust::emit_with_basic_owned_joins(&program, &options),
                Err(codegen_rust::EmitError::InvalidProgram(error)) if error == expected),
            "{id} Rust"
        );
        assert!(
            matches!(codegen_csharp::emit_with_basic_owned_joins(&program),
                Err(codegen_csharp::EmitError::ProgramValidation(error)) if error == expected),
            "{id} C#"
        );
    }
    Ok(())
}

#[test]
fn owned_joins450_legacy_joined_aggregates_keep_wire_and_generation() -> TestResult<()> {
    // A legacy joined aggregate over the same schema names collections by
    // path alone and keeps its wire, ordinary generation and interpreter
    // behavior, including correlation by collection path.
    let mut project = projects::base();
    project.extra_targets.clear();
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Codes", ScalarType::String)],
    );
    let plan = mapping::JoinPlan::new(
        mapping::JoinSource::new(projects::path(&["Rows"])),
        mapping::JoinSource::new(projects::path(&["Lookup", "Rows"])),
        mapping::JoinConditions::new(mapping::JoinKey::new(
            projects::path(&["Rows"]),
            projects::path(&["id"]),
            projects::path(&["id"]),
        )),
    )?;
    project.graph.nodes.clear();
    project.graph.nodes.insert(
        1,
        Node::JoinAggregate {
            function: mapping::AggregateOp::Join,
            join: mapping::JoinId::new(1),
            plan,
            expression: Some(2),
            arg: Some(3),
        },
    );
    project.graph.nodes.insert(
        2,
        Node::JoinField {
            join: mapping::JoinId::new(1),
            collection: projects::path(&["Lookup", "Rows"]),
            path: projects::path(&["code"]),
        },
    );
    project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("-".into()),
        },
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Codes".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert!(!wire.contains("owned_"));
    assert!(wire.contains("\"kind\": \"join_aggregate\""));
    let options = rust_options("joins450-legacy", &runtime_path()?);
    let program = codegen::lower(&project)?;
    codegen_rust::emit(&program, &options)?;
    codegen_csharp::emit(&program)?;
    let row = |id: i64, code: &str| {
        group_with(vec![
            ("id", Instance::Scalar(Value::Int(id))),
            ("code", Instance::Scalar(Value::String(code.into()))),
        ])
    };
    let source = group_with(vec![(
        "Rows",
        Instance::Repeated(vec![row(1, "a"), row(2, "b")]),
    )]);
    let lookup = group_with(vec![(
        "Rows",
        Instance::Repeated(vec![row(2, "x"), row(1, "y"), row(2, "z")]),
    )]);
    let output = engine::run_with_sources(&project, &source, vec![("Lookup".into(), lookup)])?;
    assert_eq!(
        codec::encode(&output),
        codec::encode(&group_with(vec![(
            "Codes",
            Instance::Scalar(Value::String("y-x-z".into()))
        )]))
    );
    assert_eq!(mapping::project_file::encode_pretty(&project)?, wire);
    Ok(())
}

#[test]
fn owned_joins449_legacy_joins_keep_wire_and_generation() -> TestResult<()> {
    // A legacy join over the same schema names collections by path alone and
    // keeps its wire, ordinary generation and interpreter behavior.
    let mut project = projects::base();
    project.extra_targets.clear();
    let plan = mapping::JoinPlan::new(
        mapping::JoinSource::new(projects::path(&["Rows"])),
        mapping::JoinSource::new(projects::path(&["Lookup", "Rows"])),
        mapping::JoinConditions::new(mapping::JoinKey::new(
            projects::path(&["Rows"]),
            projects::path(&["id"]),
            projects::path(&["id"]),
        )),
    )?;
    project.root.children[0].iteration = ScopeIteration::InnerJoin {
        id: mapping::JoinId::new(1),
        plan,
    };
    project.graph.nodes.clear();
    for (id, collection) in [(1, &["Rows"][..]), (2, &["Lookup", "Rows"][..])] {
        project.graph.nodes.insert(
            id,
            Node::JoinField {
                join: mapping::JoinId::new(1),
                collection: projects::path(collection),
                path: projects::path(&["code"]),
            },
        );
    }
    project.graph.nodes.insert(
        5,
        Node::JoinPosition {
            join: mapping::JoinId::new(1),
        },
    );
    project.root.children[0].bindings = vec![
        Binding {
            target_field: "left".into(),
            node: 1,
        },
        Binding {
            target_field: "right".into(),
            node: 2,
        },
        Binding {
            target_field: "tuple".into(),
            node: 5,
        },
    ];
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert!(!wire.contains("owned_"));
    assert!(wire.contains("\"kind\": \"join_field\""));
    assert!(wire.contains("\"collection\": [\n"));
    let options = rust_options("joins449-legacy", &runtime_path()?);
    let program = codegen::lower(&project)?;
    codegen_rust::emit(&program, &options)?;
    codegen_csharp::emit(&program)?;
    let row = |id: i64, code: &str| {
        group_with(vec![
            ("id", Instance::Scalar(Value::Int(id))),
            ("code", Instance::Scalar(Value::String(code.into()))),
        ])
    };
    let source = group_with(vec![(
        "Rows",
        Instance::Repeated(vec![row(1, "a"), row(2, "b")]),
    )]);
    let lookup = group_with(vec![(
        "Rows",
        Instance::Repeated(vec![row(2, "x"), row(1, "y")]),
    )]);
    let output = engine::run_with_sources(&project, &source, vec![("Lookup".into(), lookup)])?;
    let tuple = |left: &str, right: &str, position: i64| {
        group_with(vec![
            ("left", Instance::Scalar(Value::String(left.into()))),
            ("right", Instance::Scalar(Value::String(right.into()))),
            ("tuple", Instance::Scalar(Value::Int(position))),
        ])
    };
    assert_eq!(
        codec::encode(&output),
        codec::encode(&group_with(vec![(
            "Row",
            Instance::Repeated(vec![tuple("a", "y", 1), tuple("b", "x", 2)])
        )]))
    );
    assert_eq!(mapping::project_file::encode_pretty(&project)?, wire);
    Ok(())
}

fn group_with(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
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
fn host_project(model: usize) -> String {
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{model}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
    )
}
fn model_calls(controls: &Json, model: usize) -> usize {
    controls["cases"]
        .as_array()
        .map(|cases| {
            cases
                .iter()
                .filter(|case| case["model"] == model)
                .map(|case| case["calls"].as_array().map_or(0, Vec::len))
                .sum()
        })
        .unwrap_or_default()
}
/// Each host compares its calls itself; also require its complete row count.
fn host_result(results: &Path, calls: usize, run: &Output) -> Result<(), String> {
    let rows: Vec<Json> = fs::read(results)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
        .unwrap_or_default();
    let matched = rows.iter().filter(|row| row["matched"] == true).count();
    if run.status.success() && rows.len() == calls && matched == calls {
        return Ok(());
    }
    let differing: Vec<_> = rows.iter().filter(|row| row["matched"] != true).collect();
    Err(format!(
        "{}: status {:?}, {matched}/{} matched of {calls}\n{differing:#?}\n{}{}",
        results.display(),
        run.status,
        rows.len(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    ))
}

#[test]
#[ignore = "builds and runs one generated Rust host and 11 generated C# hosts; requires cargo and the .NET 10 SDK"]
fn owned_joins449_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let (controls, models) = corpus()?;
    let runtime = runtime_path()?;
    let temp = TempDir::new("joins449_hosts")?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(&controls)?,
    )?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, project) in models.iter().enumerate() {
        let program = codegen::lower_with_basic_owned_joins(project)?;
        let options = rust_options(&format!("joins449-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_joins(&program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_joins(&program)?,
        )?;
        let host = root.join(format!("csharp-host{index}"));
        write_file(&host.join("Host.csproj"), host_project(index).as_bytes())?;
        write_file(
            &host.join("Program.cs"),
            include_str!("basic_owned_joins/Host.cs.txt")
                .replace("@MODEL_CALLS@", &model_calls(&controls, index).to_string())
                .as_bytes(),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"joins449-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "            {index} => invoke!(model{index}, selection, &source, &inputs, &execution, limits),\n"
        ));
    }
    write_file(
        &root.join("rust-host/src/main.rs"),
        include_str!("basic_owned_joins/Host.rs.txt")
            .replace("@DISPATCH@", &dispatch)
            .replace("@CALLS@", &CALLS.to_string())
            .as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/codec.rs"),
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    write_file(
        &root.join("rust-host/Cargo.toml"),
        format!(
            "[package]\nname=\"joins449-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime.display().to_string())?
        )
        .as_bytes(),
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
        let results = root.join("rust-results.json");
        let run = Command::new(target.join("debug/joins449-host"))
            .arg(root.join("controls.json"))
            .arg(&results)
            .output()?;
        if let Err(error) = host_result(&results, CALLS, &run) {
            failures.push(format!("Rust host: {error}"));
        }
    } else {
        failures.push(format!(
            "Rust host build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }

    // Each C# model has its own Ferrule.Generated assembly, so each needs a host.
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
    let artifacts = root.join(format!("dotnet-artifacts-model{index}"));
    let failed = |stage: &str, output: &Output| {
        format!(
            "C# model{index} {stage}: {:?}\n{}{}",
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
        .output()
        .map_err(|error| format!("C# model{index} build: {error}"))?;
    if !build.status.success() {
        return Err(failed("build", &build));
    }
    let results = root.join(format!("csharp-model{index}-results.json"));
    let run = Command::new(dotnet)
        .arg(artifacts.join("bin/Host/release/Host.dll"))
        .arg(root.join("controls.json"))
        .arg(index.to_string())
        .arg(&results)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .output()
        .map_err(|error| format!("C# model{index} run: {error}"))?;
    host_result(&results, model_calls(controls, index), &run)
        .map_err(|error| format!("C# model{index}: {error}"))
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
    corpus.extend(presets::projects(
        closed::aggregate_projects()
            .into_iter()
            .map(|(id, project, _)| (format!("closed-aggregate/{id}"), project)),
    ));
    let base = codegen::lower_with_basic_owned_joins(&projects::project("J01-same-path-rows"))?;
    corpus.extend(presets::programs(
        closed::programs(&base)
            .into_iter()
            .map(|(id, program, _)| (format!("direct/{id}"), program)),
    ));
    let base = codegen::lower_with_basic_owned_joins(&projects::project("J08-joined-reducers"))?;
    corpus.extend(presets::programs(
        closed::aggregate_programs(&base)
            .into_iter()
            .map(|(id, program, _)| (format!("direct-aggregate/{id}"), program)),
    ));
    Ok(corpus)
}
