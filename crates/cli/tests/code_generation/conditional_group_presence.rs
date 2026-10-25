use super::*;

/// Generated Rust and C# hosts reproduce every native conditional presence
/// value and the typed missing-Boolean error for each authored design.
#[test]
fn conditional_group_presence_compiled_complete_oracles() -> TestResult<()> {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/conditional-group-presence/cases.json"
    ))?;
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/conditional-group-presence");
    for design in ["nested", "root", "lazy"] {
        let imported = mfd::import(&directory.join(format!("{design}.mfd")))?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let design_cases = cases
            .iter()
            .filter(|case| case["design"] == design)
            .cloned()
            .collect::<Vec<_>>();
        assert!(!design_cases.is_empty(), "{design}");
        json_text_boundaries::run_generated_boundary_cases(
            &imported.project,
            &design_cases,
            &format!("conditional_group_presence_{design}"),
        )?;
    }
    Ok(())
}
