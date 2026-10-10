// Complete independently authored native CSV-to-JSON boundary expectations.
// No cleanup on success or failure; every stage is retained before assertions.
mod expected {
    include!("Program.rs.txt");
}
use ir::{Instance, Value};
use serde_json::{Value as Json, json};
use std::{fs, path::Path};

fn save(
    root: &Path,
    name: &str,
    value: &impl serde::Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(root.join(name), serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn tree(value: &Instance) -> Json {
    match value {
        Instance::Scalar(value) => json!({"kind":"Scalar","value":match value {
            Value::Null=>json!({"kind":"Null"}),Value::JsonNull(_)=>json!({"kind":"JsonNull"}),
            Value::XmlNil(_)=>json!({"kind":"XmlNil"}),Value::String(v)=>json!({"kind":"String","value":v}),
            Value::Int(v)=>json!({"kind":"Int","value":v}),Value::Bool(v)=>json!({"kind":"Bool","value":v}),
            Value::Float(v)=>json!({"kind":"Float","bits":format!("{:016x}",v.to_bits())})}}),
        Instance::Group(group) => json!({"kind":"Group","origin":match group.xml_type_origin() {
            ir::XmlTypeOrigin::Unknown=>json!({"kind":"Unknown"}),
            ir::XmlTypeOrigin::Absent=>json!({"kind":"Absent"}),
            ir::XmlTypeOrigin::Explicit(value)=>json!({"kind":"Explicit","identity":value}),
            ir::XmlTypeOrigin::ExplicitPadded{literal,resolved_identity}=>json!({"kind":"ExplicitPadded","literal":literal,"identity":resolved_identity})
        },"fields":group.iter().map(|(name,value)|json!({"name":name,"value":tree(value)})).collect::<Vec<_>>()}),
        Instance::Repeated(items) => {
            json!({"kind":"Repeated","items":items.iter().map(tree).collect::<Vec<_>>()})
        }
        Instance::MappedSequence(items) => {
            json!({"kind":"MappedSequence","items":items.iter().map(tree).collect::<Vec<_>>()})
        }
        _ => {
            json!({"kind":"UnselectedInstanceKind","complete_debug_original":format!("{value:#?}")})
        }
    }
}

fn csv_error(error: &format_csv::CsvFormatError) -> Json {
    use format_csv::CsvFormatError;
    match error {
        CsvFormatError::Parse {
            row,
            field,
            expected,
            value,
        } => {
            json!({"type":"CsvFormatError","variant":"Parse","fields":{"row":row,"field":field,"expected":format!("{expected:?}"),"value":value},"message":error.to_string()})
        }
        _ => {
            json!({"type":"CsvFormatError","unselected_variant_original":format!("{error:?}"),"message":error.to_string()})
        }
    }
}

fn engine_error(error: &engine::EngineError) -> Json {
    use engine::EngineError;
    let (variant, fields) = match error {
        EngineError::MissingRuntimeParameter { node, name } => {
            ("MissingRuntimeParameter", json!({"node":node,"name":name}))
        }
        EngineError::MappingFailure { rule, message } => {
            ("MappingFailure", json!({"rule":rule,"message":message}))
        }
        EngineError::UnknownTarget { name } => ("UnknownTarget", json!({"name":name})),
        _ => {
            return json!({"type":"EngineError","unselected_variant_original":format!("{error:?}"),"message":error.to_string()});
        }
    };
    json!({"type":"EngineError","variant":variant,"fields":fields,"message":error.to_string()})
}

fn json_error(error: &format_json::JsonFormatError) -> Json {
    use format_json::JsonFormatError;
    let (variant, fields) = match error {
        JsonFormatError::Shape {
            name,
            expected,
            got,
        } => ("Shape", json!({"name":name,"expected":expected,"got":got})),
        JsonFormatError::MissingRequiredProperty { object, property } => (
            "MissingRequiredProperty",
            json!({"object":object,"property":property}),
        ),
        _ => {
            return json!({"type":"JsonFormatError","unselected_variant_original":format!("{error:?}"),"message":error.to_string()});
        }
    };
    json!({"type":"JsonFormatError","variant":variant,"fields":fields,"message":error.to_string()})
}

fn check(fixture: &Path, evidence: &Path, case: &Json) -> Result<bool, Box<dyn std::error::Error>> {
    let root = evidence.join(case["id"].as_str().unwrap());
    fs::create_dir_all(&root)?;
    save(&root, "CASE-ORIGINAL.json", case)?;
    let input = fs::read(fixture.join(case["input"].as_str().unwrap()))?;
    fs::write(root.join("INPUT-ORIGINAL.csv"), &input)?;
    let family = case["family"].as_str().unwrap();
    let project_bytes = fs::read(fixture.join("projects").join(format!("{family}.json")))?;
    fs::write(root.join("PROJECT-ORIGINAL.json"), &project_bytes)?;
    let project: mapping::Project = serde_json::from_slice(&project_bytes)?;
    let validation = engine::validate(&project);
    save(
        &root,
        "VALIDATION-ORIGINAL.json",
        &json!({"debug":format!("{validation:?}"),"display":validation.iter().map(ToString::to_string).collect::<Vec<_>>()}),
    )?;
    let program = codegen::lower(&project);
    save(
        &root,
        "LOWER-ORIGINAL.json",
        &json!({"debug":format!("{program:#?}")}),
    )?;
    let expected = expected::expected_program(&project, family);
    save(
        &root,
        "PROGRAM-MODEL-ORIGINAL.json",
        &json!({"debug":format!("{expected:#?}")}),
    )?;
    let program_equal = program.as_ref().is_ok_and(|actual| actual == &expected);
    let program_validation = program.as_ref().map(codegen::validate_program);
    save(
        &root,
        "PROGRAM-VALIDATION-ORIGINAL.json",
        &json!({"debug":format!("{program_validation:#?}")}),
    )?;
    let text = std::str::from_utf8(&input)?;
    let parsed = format_csv::from_str_with_options(
        text,
        &project.source,
        &format_csv::CsvReadOptions::from(&project.source_options),
    );
    save(
        &root,
        "PARSE-ORIGINAL.json",
        &match &parsed {
            Ok(rows) => {
                json!({"debug":format!("{rows:#?}"),"typed":tree(&Instance::Repeated(rows.clone()))})
            }
            Err(error) => csv_error(error),
        },
    )?;
    let mut failures = vec![];
    if !validation.is_empty() {
        failures.push("project validation");
    }
    if !program_equal {
        failures.push("complete independently authored Program");
    }
    if !program_validation.as_ref().is_ok_and(|r| r.is_ok()) {
        failures.push("program validation");
    }
    let error_expected = &case["expected_error"];
    match parsed {
        Err(error) => {
            if error_expected["stage"] != "parse" || csv_error(&error) != error_expected["native"] {
                failures.push("full parse error");
            }
        }
        Ok(rows) => {
            let source = Instance::Repeated(rows);
            if tree(&source) != case["expected_parsed"] {
                failures.push("complete parsed source");
            }
            let selection_name = case["selection"].as_str().unwrap();
            let selection = if selection_name == "Primary" {
                engine::TargetSelection::Primary
            } else {
                engine::TargetSelection::Named(selection_name)
            };
            let mut parameters = engine::RuntimeParameters::default();
            for (key, value) in case["context"].as_object().unwrap() {
                parameters.insert(key.clone(), Value::String(value.as_str().unwrap().into()))?;
            }
            let context = engine::ExecutionContext::new(Path::new("authored-csv-json.mapping"))
                .with_parameters(&parameters);
            let output = engine::run_selected_target_with_sources_and_context(
                &project,
                &source,
                vec![],
                &context,
                selection,
            );
            let source_after = tree(&source);
            save(&root, "SOURCE-AFTER-ORIGINAL.json", &source_after)?;
            save(
                &root,
                "SELECTED-ORIGINAL.json",
                &match &output {
                    Ok(engine::SelectedTargetOutput::Primary(instance)) => {
                        json!({"kind":"Primary","instance":tree(instance)})
                    }
                    Ok(engine::SelectedTargetOutput::Named(named)) => {
                        json!({"kind":"Named","name":named.name,"instance":tree(&named.instance)})
                    }
                    Err(error) => engine_error(error),
                },
            )?;
            if source_after != case["expected_parsed"] {
                failures.push("parsed source changed during selected evaluation");
            }
            match output {
                Err(error) => {
                    if error_expected["stage"] != "mapping"
                        || engine_error(&error) != error_expected["native"]
                    {
                        failures.push("full mapping error");
                    }
                }
                Ok(output) => {
                    let (snapshot, instance, schema) = match &output {
                        engine::SelectedTargetOutput::Primary(instance) => (
                            json!({"kind":"Primary","instance":tree(instance)}),
                            instance,
                            &project.target,
                        ),
                        engine::SelectedTargetOutput::Named(named) => (
                            json!({"kind":"Named","name":named.name,"instance":tree(&named.instance)}),
                            &named.instance,
                            &project
                                .extra_targets
                                .iter()
                                .find(|target| target.name == named.name)
                                .unwrap()
                                .schema,
                        ),
                    };
                    if snapshot != case["expected_selected"] {
                        failures.push("complete selected output");
                    }
                    let serialized = format_json::to_string(schema, instance);
                    save(
                        &root,
                        "JSON-ORIGINAL.json",
                        &match &serialized {
                            Ok(text) => json!({"text":text,"utf8":text.as_bytes()}),
                            Err(error) => json_error(error),
                        },
                    )?;
                    match serialized {
                        Err(error) => {
                            if error_expected["stage"] != "serialize"
                                || json_error(&error) != error_expected["native"]
                            {
                                failures.push("full JSON error");
                            }
                        }
                        Ok(text) => {
                            fs::write(root.join("OUTPUT-ORIGINAL.json"), text.as_bytes())?;
                            if !error_expected.is_null() {
                                failures.push("expected error was success");
                            } else if text.as_bytes()
                                != fs::read(fixture.join(case["expected_wire"].as_str().unwrap()))?
                            {
                                failures.push("complete JSON text/UTF8");
                            }
                        }
                    }
                }
            }
        }
    }
    if fs::read(fixture.join(case["input"].as_str().unwrap()))? != input {
        failures.push("input bytes changed");
    }
    save(
        &root,
        "COMPARISON.json",
        &json!({"passed":failures.is_empty(),"failures":failures}),
    )?;
    Ok(failures.is_empty())
}

pub(super) fn run(fixture: &Path, evidence: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if evidence.exists() {
        return Err(
            "evidence directory exists; preserve it and choose a fresh owned directory".into(),
        );
    }
    fs::create_dir_all(evidence)?;
    let bytes = fs::read(fixture.join("cases.json"))?;
    fs::write(evidence.join("CASES-ORIGINAL.json"), &bytes)?;
    let cases: Vec<Json> = serde_json::from_slice(&bytes)?;
    let mut outcomes = vec![];
    for case in cases {
        match check(fixture, evidence, &case) {
            Ok(passed) => outcomes.push(json!({"id":case["id"],"passed":passed})),
            Err(error) => {
                let actual =
                    json!({"id":case["id"],"error":error.to_string(),"debug":format!("{error:?}")});
                let case_root = evidence.join(case["id"].as_str().unwrap());
                fs::create_dir_all(&case_root)?;
                save(&case_root, "SETUP-ERROR-ORIGINAL.json", &actual)?;
                outcomes.push(json!({"id":case["id"],"passed":false,"error_original":actual}));
            }
        }
        save(evidence, "PARTIAL-OUTCOMES-ORIGINAL.json", &outcomes)?;
    }
    save(evidence, "FINAL.json", &outcomes)?;
    if outcomes.iter().any(|row| row["passed"] != true) {
        return Err("authored native oracle comparison failed; all originals retained".into());
    }
    Ok(())
}
