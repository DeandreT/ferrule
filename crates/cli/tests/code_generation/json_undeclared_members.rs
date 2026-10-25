use super::*;

/// Generated Rust and C# hosts write authored undeclared JSON members under
/// their exact names, enforce their copied constraints, and refuse an absent
/// required member exactly as the native boundary and matching-name control do.
#[test]
fn json_undeclared_members_compiled_complete_oracles() -> TestResult<()> {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/json-undeclared-members/cases.json"
    ))?;
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/json-undeclared-members");
    for design in ["open", "typed", "nested", "matching"] {
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
            &format!("json_undeclared_members_{design}"),
        )?;
    }
    Ok(())
}
