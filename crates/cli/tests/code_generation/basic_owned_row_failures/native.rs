use super::*;
use engine::EngineError;
use mapping::SourceOwner;
use std::error::Error as _;

fn expected_error(recipe: &Json) -> EngineError {
    let node = recipe["node"].as_u64().unwrap_or(0) as u32;
    let strings = |key: &str| {
        recipe[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().into())
            .collect::<Vec<String>>()
    };
    match recipe["kind"].as_str().unwrap() {
        "MappingFailure" => EngineError::MappingFailure {
            rule: recipe["rule"].as_u64().unwrap() as usize,
            message: recipe["message"].as_str().map(str::to_owned),
        },
        "MappingException" => EngineError::MappingException {
            node,
            message: recipe["message"].as_str().map(str::to_owned),
        },
        "NotABool" => EngineError::NotABool {
            node,
            found: match recipe["found"].as_str().unwrap() {
                "null" => "null",
                "string" => "string",
                _ => panic!("literal bool failure domain"),
            },
        },
        "InactivePosition" => EngineError::OwnedSourceAccess {
            node: Some(node),
            scope: None,
            source: engine::OwnedSourceError::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(strings("collection")),
            },
        },
        "OwnedSource" => EngineError::OwnedSourceAccess {
            node: Some(node),
            scope: None,
            source: match recipe["cause"].as_str().unwrap() {
                "SourceUnavailable" => engine::OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named {
                        name: recipe["owner"].as_str().unwrap().into(),
                    },
                },
                "CannotTraverse" => engine::OwnedSourceError::MalformedOwnedTraversal {
                    path: strings("native_path"),
                    found: "a non-null scalar",
                },
                "ExpectedScalar" => engine::OwnedSourceError::MalformedOwnedTraversal {
                    path: strings("native_path"),
                    found: "a group",
                },
                "UniteratedRepetition" => engine::OwnedSourceError::UniteratedOwnedRepetition {
                    path: strings("native_path"),
                },
                _ => panic!("literal owned cause"),
            },
        },
        _ => panic!("literal native error"),
    }
}

pub(super) fn run(
    projects: &[Project],
    controls: &Json,
    evidence: &Evidence,
) -> TestResult<Vec<(String, String, bool)>> {
    let execution = engine::ExecutionContext::new(Path::new("row808.mapping"));
    let mut checks = Vec::new();
    for (ordinal, case) in controls["cases"].as_array().unwrap().iter().enumerate() {
        let project = &projects[case["model"].as_u64().unwrap() as usize];
        let source: Instance = serde_json::from_value(case["source"].clone())?;
        let inputs: Vec<(String, Instance)> = serde_json::from_value(case["inputs"].clone())?;
        let before = source.clone();
        for call in case["calls"].as_array().unwrap() {
            let selection = call["selection"].as_str().unwrap();
            let prefix = format!("native/{ordinal:02}-{selection}");
            evidence.debug(
                &format!("{prefix}-COMPLETE-INPUT.txt"),
                &(&source, &inputs, case, call),
            )?;
            let expected: Result<Vec<(Option<String>, Instance)>, EngineError> =
                match call["expected"].get("Ok") {
                    Some(output) => Ok(serde_json::from_value(output.clone())?),
                    None => Err(expected_error(&call["expected"]["Err"])),
                };
            evidence.debug(&format!("{prefix}-COMPLETE-FROZEN-EXPECTED.txt"), &expected)?;
            let actual = if selection == "All" {
                engine::run_outputs_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                )
                .map(|output| {
                    let mut outputs = vec![(None, output.primary)];
                    outputs.extend(
                        output
                            .extras
                            .into_iter()
                            .map(|extra| (Some(extra.name), extra.instance)),
                    );
                    outputs
                })
            } else {
                let selection = if selection == "Primary" {
                    engine::TargetSelection::Primary
                } else {
                    engine::TargetSelection::Named("audit")
                };
                engine::run_selected_target_with_sources_and_context(
                    project,
                    &source,
                    inputs.clone(),
                    &execution,
                    selection,
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
                let mut chain = format!("Debug: {error:#?}\nDisplay: {error}\n");
                let mut cause = error.source();
                while let Some(error) = cause {
                    chain.push_str(&format!(
                        "Cause Debug: {error:#?}\nCause Display: {error}\n"
                    ));
                    cause = error.source();
                }
                evidence.write(
                    &format!("{prefix}-COMPLETE-CAUSE-CHAIN.txt"),
                    chain.as_bytes(),
                )?;
            }
            evidence.debug(
                &format!("{prefix}-COMPLETE-AFTER-INPUT.txt"),
                &(&source, &inputs),
            )?;
            checks.push((
                case["id"].as_str().unwrap().into(),
                selection.into(),
                actual == expected && source == before,
            ));
        }
    }
    Ok(checks)
}
