use super::*;

#[path = "../../../engine/tests/fixtures/singleton_frames.rs"]
mod fixture;

#[test]
fn singleton_frames_reducers_complete_native_rust_csharp_oracles() -> TestResult<()> {
    let project = fixture::reducer_project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let cases = fixture::reducer_cases();
    for case in &cases {
        let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source)?;
        let output = engine::run(&project, &source)?;
        let output: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
        assert_eq!(output, case["expected"]);
    }
    json_text_boundaries::run_generated_boundary_cases(
        &project,
        &cases,
        "singleton_frames_reducers",
    )
}

#[test]
fn singleton_frames_complete_native_rust_csharp_oracles() -> TestResult<()> {
    for separate in [false, true] {
        for hops in [1, 2] {
            let project = fixture::project(separate, hops);
            assert!(
                engine::validate(&project).is_empty(),
                "{:?}",
                engine::validate(&project)
            );
            let mut cases = fixture::cases(separate, hops);
            for case in &cases {
                let input =
                    format_json::from_str(case["input"].as_str().unwrap(), &project.source)?;
                let output = engine::run(&project, &input)?;
                let output: serde_json::Value =
                    serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
                assert_eq!(output, case["expected"]);
            }
            let input = r#"{"Rows":[{"Label":"missing","Detail":{"Flag":"child","Inner":{"Flag":"inner"}}}]}"#;
            let source = format_json::from_str(input, &project.source)?;
            assert_eq!(
                engine::run(&project, &source),
                Err(engine::EngineError::NotABool {
                    node: 0,
                    found: "null"
                })
            );
            cases
                .push(serde_json::json!({"input":input,"boolean_error":{"node":0,"found":"null"}}));
            json_text_boundaries::run_generated_boundary_cases(
                &project,
                &cases,
                &format!("singleton_frames_{separate}_{hops}"),
            )?;
        }
    }
    Ok(())
}

#[test]
fn singleton_frames_lazy_children_and_boolean_error_owner() -> TestResult<()> {
    let mut project = fixture::project(false, 1);
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("invalid condition".into()),
        },
    );
    project.graph.nodes.insert(
        10,
        Node::If {
            condition: 9,
            then: 3,
            else_: 3,
        },
    );
    project.root.children[0].children[0].bindings[0].node = 10;
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let cases = [
        serde_json::json!({"input":r#"{"Rows":[{"Flag":false,"Label":"ancestor","Detail":{"Flag":"not bool","Label":"child"}}]}"#,
            "expected":{"Rows":[{"Pinned":false,"Absolute":false,"Fallback":"child","Item":[],"Child":[]}]}}),
        serde_json::json!({"input":r#"{"Rows":[{"Flag":true,"Label":"ancestor","Detail":{"Flag":"not bool","Label":"child"}}]}"#,
            "boolean_error":{"node":9,"found":"string"}}),
    ];
    for case in &cases {
        let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source)?;
        if let Some(expected) = case.get("boolean_error") {
            match engine::run(&project, &source) {
                Err(engine::EngineError::NotABool { node, found }) => {
                    assert_eq!(Some(u64::from(node)), expected["node"].as_u64());
                    assert_eq!(Some(found), expected["found"].as_str());
                }
                other => panic!("expected Boolean error for {case}, got {other:?}"),
            }
        } else {
            let output = engine::run(&project, &source)?;
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&format_json::to_string(
                    &project.target,
                    &output
                )?)?,
                case["expected"]
            );
        }
    }
    json_text_boundaries::run_generated_boundary_cases(&project, &cases, "singleton_frames_lazy")
}
