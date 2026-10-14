use super::*;

#[test]
fn enclosing_frame_clone_occurrences_compiled_complete_oracles() -> TestResult<()> {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/enclosing-frame-clones/cases.json"
    ))?;
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/enclosing-frame-clones");
    for design in [
        "implicit",
        "literal-only",
        "guaranteed",
        "mixed",
        "wrapped-markers",
        "nested",
        "edi",
        "many-branches",
        "false-zero-empty",
        "unconnected-declaration",
        "idempotent-physical",
    ] {
        let imported = mfd::import(&directory.join(format!("{design}.mfd")))?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty());
        let selected = cases
            .iter()
            .filter(|case| {
                case["design"] == design || design != "nested" && case["design"] == "all-flat"
            })
            .cloned()
            .collect::<Vec<_>>();
        json_text_boundaries::run_generated_boundary_cases(
            &imported.project,
            &selected,
            &format!("enclosing_clones_{design}"),
        )?;
    }
    Ok(())
}
