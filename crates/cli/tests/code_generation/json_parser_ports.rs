use super::*;

#[test]
fn dormant_json_parser_compiled_complete_documents() -> TestResult<()> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/dormant-json-parser/mapping.mfd");
    let imported = mfd::import_with_profile(
        &fixture,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?
    .imported;
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node,
                Node::RuntimeParameter { name, .. } if name == "payload"
            ))
    );
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/dormant-json-parser/cases.json"
    ))?;
    let cases = cases
        .into_iter()
        .flat_map(|case| {
            [
                serde_json::json!({}),
                serde_json::json!({"payload": "{"}),
                serde_json::json!({"payload": "{\"Ignored\":\"ok\"}"}),
            ]
            .map(|parameters| {
                let mut case = case.clone();
                case["parameters"] = parameters;
                case
            })
        })
        .collect::<Vec<_>>();
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &imported.project,
        &cases,
        "dormant_json_parser",
        include_str!("fixtures/optional_parser_input_rust.rs.txt"),
        include_str!("fixtures/optional_parser_input_csharp.cs.txt"),
    )
}

#[test]
fn json_parser_unused_ports_compiled_complete_oracles() -> TestResult<()> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/json-parser-unused-ports/mapping.mfd");
    let imported = mfd::import(&fixture)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/json-parser-unused-ports/cases.json"
    ))?;
    json_text_boundaries::run_generated_boundary_cases(
        &imported.project,
        &cases,
        "json_parser_unused_ports",
    )
}
