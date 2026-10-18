//! Native checks are typed mapping checks, never invented native JSON policies.
use super::*;
use std::error::Error as _;

pub(super) const CALLS: usize = 26;

pub(super) fn tree(value: &Json) -> Instance {
    match value {
        Json::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), tree(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
        Json::Array(items) => Instance::Repeated(items.iter().map(tree).collect()),
        Json::String(value) => Instance::Scalar(Value::String(value.clone())),
        Json::Bool(value) => Instance::Scalar(Value::Bool(*value)),
        Json::Number(value) => Instance::Scalar(Value::Int(value.as_i64().unwrap())),
        Json::Null => Instance::Scalar(Value::Null),
    }
}
type Outcome = Result<Vec<(Option<String>, Instance)>, engine::EngineError>;
fn controls() -> Json {
    let portable = fixtures::controls();
    let mut cases = portable["cases"].as_array().unwrap()[..3].to_vec();
    // These controls have real native typed equivalents; JSON boundary-only
    // controls are intentionally absent rather than represented by fake limits.
    for id in [
        "optional-omitted-primary-is-lazy",
        "optional-omitted-demand-is-source-unavailable",
        "mapping-error-has-no-partial-output",
        "unselected-primary-poison-is-lazy",
    ] {
        cases.push(
            portable["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["id"] == id)
                .unwrap()
                .clone(),
        );
    }
    let mut empty = cases[2].clone();
    empty["id"] = "native-repeated-empty".into();
    empty["inputs"][0][1] = "{\"Code\":\"N.root\",\"Rows\":[]}".into();
    empty["calls"] = json!([{"selection":"Primary","expected":{"Ok":[[null,"[]",0]]}}]);
    cases.push(empty);
    let mut malformed = cases[1].clone();
    malformed["id"] = "native-owned-terminal-structural-cause".into();
    malformed["inputs"][0][1] = "{\"Code\":{}}".into();
    malformed["calls"] =
        json!([{"selection":"Primary","expected":{"Err":{"kind":"ExpectedScalar","node":2}}}]);
    cases.push(malformed);
    json!({"version":1,"native_typed_only":true,"cases":cases})
}
fn expected(call: &Json) -> TestResult<Outcome> {
    if let Some(outputs) = call["expected"].get("Ok") {
        let mut result = Vec::new();
        for row in outputs.as_array().unwrap() {
            let value: Json = serde_json::from_str(row[1].as_str().unwrap())?;
            result.push((row[0].as_str().map(str::to_owned), tree(&value)));
        }
        return Ok(Ok(result));
    }
    let error = &call["expected"]["Err"];
    Ok(Err(match error["kind"].as_str().unwrap() {
        "MappingException" => engine::EngineError::MappingException {
            node: 6,
            message: Some("520 target poison".into()),
        },
        "SourceUnavailable" => engine::EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: engine::OwnedSourceError::SourceUnavailable {
                owner: mapping::SourceOwner::Named { name: "Ref".into() },
            },
        },
        "ExpectedScalar" => engine::EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: engine::OwnedSourceError::MalformedOwnedTraversal {
                path: vec!["Code".into()],
                found: "a group",
            },
        },
        _ => panic!("only frozen native typed errors"),
    }))
}
pub(super) fn run(
    projects: &[Project],
    evidence: &Evidence,
) -> TestResult<Vec<(String, String, bool)>> {
    let controls = controls();
    evidence.write(
        "native/COMPLETE-INDEPENDENT-TYPED-CONTROLS.json",
        &serde_json::to_vec_pretty(&controls)?,
    )?;
    let context = engine::ExecutionContext::new(Path::new("json520.mapping"));
    let mut checks = Vec::new();
    for (ordinal, case) in controls["cases"].as_array().unwrap().iter().enumerate() {
        let project = &projects[case["model"].as_u64().unwrap() as usize];
        let source = tree(&serde_json::from_str(case["source"].as_str().unwrap())?);
        let inputs = case["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| {
                Ok((
                    input[0].as_str().unwrap().to_owned(),
                    tree(&serde_json::from_str(input[1].as_str().unwrap())?),
                ))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let before = (source.clone(), inputs.clone());
        for call in case["calls"].as_array().unwrap() {
            let selection = call["selection"].as_str().unwrap();
            let prefix = format!("native/{ordinal:02}-{selection}");
            evidence.debug(&format!("{prefix}-COMPLETE-INPUT.txt"), &(&source, &inputs, call, "mapping_file_path=json520.mapping; main_mapping_file_path=json520.mapping; current_datetime=None; parameters=None; loader=None; cancellation=None"))?;
            let expected = expected(call)?;
            evidence.debug(&format!("{prefix}-COMPLETE-FROZEN-EXPECTED.txt"), &expected)?;
            let actual: Outcome = if selection == "All" {
                engine::run_outputs_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &context,
                )
                .map(|output| {
                    let mut outputs = vec![(None, output.primary)];
                    outputs.extend(
                        output
                            .extras
                            .into_iter()
                            .map(|output| (Some(output.name), output.instance)),
                    );
                    outputs
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
                    &context,
                    target,
                )
                .map(|output| match output {
                    engine::SelectedTargetOutput::Primary(instance) => vec![(None, instance)],
                    engine::SelectedTargetOutput::Named(output) => {
                        vec![(Some(output.name), output.instance)]
                    }
                })
            };
            evidence.debug(&format!("{prefix}-COMPLETE-ACTUAL.txt"), &actual)?;
            if let Err(error) = &actual {
                let mut original = format!("Debug={error:#?}\nDisplay={error}\n");
                let mut cause = error.source();
                while let Some(error) = cause {
                    original.push_str(&format!("CauseDebug={error:#?}\nCauseDisplay={error}\n"));
                    cause = error.source();
                }
                original.push_str("CAUSE_CHAIN_TERMINATED=None\n");
                evidence.write(
                    &format!("{prefix}-COMPLETE-CAUSE-CHAIN.txt"),
                    original.as_bytes(),
                )?;
            }
            evidence.debug(
                &format!("{prefix}-COMPLETE-AFTER-INPUT.txt"),
                &(&source, &inputs),
            )?;
            checks.push((
                case["id"].as_str().unwrap().to_owned(),
                selection.to_owned(),
                actual == expected && before == (source.clone(), inputs.clone()),
            ));
        }
    }
    Ok(checks)
}
