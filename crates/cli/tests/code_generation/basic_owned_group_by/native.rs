use super::*;
fn strings(recipe: &Json, key: &str) -> Vec<String> {
    recipe[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().into())
        .collect()
}
fn scope(recipe: &Json) -> Option<Box<mapping::SourceScopeLocation>> {
    let value = recipe.get("native_scope")?;
    if value.is_null() {
        return None;
    }
    let target = if value["target"] == "Primary" {
        mapping::SourceScopeTarget::Primary
    } else {
        mapping::SourceScopeTarget::Named {
            index: value["target"]["index"].as_u64().unwrap() as usize,
            name: value["target"]["named"].as_str().unwrap().into(),
        }
    };
    Some(Box::new(mapping::SourceScopeLocation {
        target,
        path: value["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| mapping::SourceScopeStep::Child(index.as_u64().unwrap() as usize))
            .collect(),
    }))
}
fn expected_error(recipe: &Json) -> engine::EngineError {
    let node = recipe["node"].as_u64().map(|n| n as u32);
    let found = match recipe["found"].as_str() {
        Some("bool") => "bool",
        Some("string") => "string",
        _ => "null",
    };
    match recipe["kind"].as_str().unwrap() {
        "MappingException" => engine::EngineError::MappingException {
            node: node.unwrap(),
            message: recipe["message"].as_str().map(str::to_owned),
        },
        "MappingFailure" => engine::EngineError::MappingFailure {
            rule: recipe["rule"].as_u64().unwrap() as usize,
            message: recipe["message"].as_str().map(str::to_owned),
        },
        "NotABool" => engine::EngineError::NotABool {
            node: node.unwrap(),
            found,
        },
        "NotAnItemCount" => engine::EngineError::NotAnItemCount {
            node: node.unwrap(),
            found,
        },
        "UnknownTarget" => engine::EngineError::UnknownTarget {
            name: recipe["name"].as_str().unwrap().into(),
        },
        "OwnedSource" => {
            let owner = if recipe["owner"] == "Primary" {
                mapping::SourceOwner::Primary
            } else {
                mapping::SourceOwner::Named {
                    name: recipe["owner"].as_str().unwrap().into(),
                }
            };
            let source = match recipe["cause"].as_str().unwrap() {
                "SourceUnavailable" => engine::OwnedSourceError::SourceUnavailable { owner },
                "InactiveSourceFrame" => engine::OwnedSourceError::InactiveSourceFrame {
                    owner,
                    collection: Some(strings(recipe, "collection")),
                },
                "UniteratedRepetition" => engine::OwnedSourceError::UniteratedOwnedRepetition {
                    path: strings(recipe, "native_path"),
                },
                "CannotTraverse" => engine::OwnedSourceError::MalformedOwnedTraversal {
                    path: strings(recipe, "native_path"),
                    found: "a non-null scalar",
                },
                "ExpectedScalar" => engine::OwnedSourceError::MalformedOwnedTraversal {
                    path: strings(recipe, "native_path"),
                    found: "a group",
                },
                _ => panic!("authored native cause"),
            };
            engine::EngineError::OwnedSourceAccess {
                node,
                scope: scope(recipe),
                source,
            }
        }
        _ => panic!("authored native error"),
    }
}
#[derive(Debug)]
enum Product {
    All(engine::ExecutionOutputs),
    Selected(engine::SelectedTargetOutput),
}

pub(super) fn run(
    controls: &Json,
    projects: &[Project],
    evidence: &Evidence,
) -> TestResult<Vec<Json>> {
    let execution = engine::ExecutionContext::new(Path::new("group441.mapping"));
    let mut checks = Vec::new();
    for (ordinal, case) in controls["cases"].as_array().unwrap().iter().enumerate() {
        let project = &projects[case["model"].as_u64().unwrap() as usize];
        let source = codec::decode(&case["source"]);
        let inputs = case["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| (input[0].as_str().unwrap().into(), codec::decode(&input[1])))
            .collect::<Vec<_>>();
        let before = (
            codec::encode(&source),
            inputs
                .iter()
                .map(|(name, input)| json!([name, codec::encode(input)]))
                .collect::<Vec<_>>(),
        );
        for call in case["calls"].as_array().unwrap() {
            let selection = call["selection"].as_str().unwrap();
            let prefix = format!("native/{ordinal:02}-{selection}");
            evidence.debug(
                &format!("{prefix}-COMPLETE-INPUT.txt"),
                &(case, call, &source, &inputs),
            )?;
            let actual = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if selection == "All" {
                    engine::run_outputs_with_sources_and_context(
                        project,
                        &source,
                        inputs.clone(),
                        &execution,
                    )
                    .map(Product::All)
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
                    .map(Product::Selected)
                }
            }));
            let observed = codec::retain_outcome(
                actual,
                |suffix, bytes| evidence.write_io(&format!("{prefix}-{suffix}"), bytes),
                |product| match product {
                    Product::All(output) => {
                        json!({"variant":"All","primary":codec::raw_tree(&output.primary),
                        "extras":output.extras.iter().map(|output|json!({"name":output.name,"instance":codec::raw_tree(&output.instance)})).collect::<Vec<_>>()})
                    }
                    Product::Selected(engine::SelectedTargetOutput::Primary(instance)) => {
                        json!({"variant":"Primary","instance":codec::raw_tree(instance)})
                    }
                    Product::Selected(engine::SelectedTargetOutput::Named(output)) => {
                        json!({"variant":"Named","name":output.name,"instance":codec::raw_tree(&output.instance)})
                    }
                },
                |product| {
                    let trees = match product {
                        Ok(Product::All(output)) => {
                            let mut trees = vec![json!([null, codec::encode(&output.primary)])];
                            trees.extend(output.extras.iter().map(|output| {
                                json!([output.name, codec::encode(&output.instance)])
                            }));
                            Some(json!(trees))
                        }
                        Ok(Product::Selected(engine::SelectedTargetOutput::Primary(instance))) => {
                            Some(json!([[null, codec::encode(instance)]]))
                        }
                        Ok(Product::Selected(engine::SelectedTargetOutput::Named(output))) => {
                            Some(json!([[output.name, codec::encode(&output.instance)]]))
                        }
                        Err(_) => None,
                    };
                    let after = (
                        codec::encode(&source),
                        inputs
                            .iter()
                            .map(|(name, input)| json!([name, codec::encode(input)]))
                            .collect::<Vec<_>>(),
                    );
                    let matched = match product {
                        Ok(_) => call["expected"].get("Ok").is_some_and(|expected| {
                            trees.as_ref().is_some_and(|actual| actual == expected)
                        }),
                        Err(error) => call["expected"]
                            .get("Err")
                            .is_some_and(|recipe| *error == expected_error(recipe)),
                    } && before == after;
                    json!({"actual_trees":trees,"after_inputs":after,"matched":matched})
                },
            )?;
            let matched = observed.is_some_and(|record| record["matched"] == true);
            checks.push(json!({"case":case["id"],"selection":selection,"matched":matched}));
        }
    }
    Ok(checks)
}
