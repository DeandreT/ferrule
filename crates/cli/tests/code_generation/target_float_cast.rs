use super::*;

pub(super) fn project(cast_first: bool, explicit: bool) -> TestResult<Project> {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/target_float_cast");
    let package = TempDir::new("target_float_cast")?;
    for name in ["source.xsd", "target.xsd"] {
        std::fs::copy(fixtures.join(name), package.0.join(name))?;
    }
    let mut mapping = std::fs::read_to_string(fixtures.join("cast-first.mfd"))?;
    if !explicit {
        assert!(mapping.contains(" casttotargettypemode=\"cast-in-subtree\""));
        mapping = mapping.replace(" casttotargettypemode=\"cast-in-subtree\"", "");
        assert!(!mapping.contains("casttotargettypemode"));
    }
    if !cast_first {
        mapping = mapping
            .replace(
                "<edge vertexkey=\"120\"/>",
                "<edge vertexkey=\"temporary\"/>",
            )
            .replace("<edge vertexkey=\"20\"/>", "<edge vertexkey=\"120\"/>")
            .replace(
                "<edge vertexkey=\"temporary\"/>",
                "<edge vertexkey=\"20\"/>",
            );
    }
    std::fs::write(package.0.join("mapping.mfd"), mapping)?;
    let imported = mfd::import(&package.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    let mut project = imported.project;
    project.source_path = Some("input.xml".into());
    project.target_path = Some("output.xml".into());
    Ok(project)
}

#[test]
fn target_float_cast_keeps_exact_xml_failure_selection() -> TestResult<()> {
    for (cast_first, explicit) in [(true, true), (false, true), (true, false), (false, false)] {
        let project = project(cast_first, explicit)?;
        let mut cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../mfd/tests/fixtures/target_float_cast/cases.json"
        ))?;
        for case in &mut cases {
            case["input"] = format!("<Envelope><Numerator>{}</Numerator><Denominator>{}</Denominator><Text>{}</Text><Label>safe</Label></Envelope>", case["numerator"].as_str().unwrap(), case["denominator"].as_str().unwrap(), case["text"].as_str().unwrap()).into();
            if case["error"] == "first" {
                case["error"] = if cast_first { "cast" } else { "divide" }.into();
            }
            if case["error"].is_null() {
                let values = if cast_first { [29, 17] } else { [17, 29] };
                case["expected_xml"] = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Envelope>\n  <Amount>{}</Amount>\n  <Amount>{}</Amount>\n  <Label>safe</Label>\n</Envelope>", values[0], values[1]).into();
            }
        }
        assert_eq!(cases.len(), 4);
        json_text_boundaries::run_generated_boundary_cases_with_harnesses(
            &project,
            &cases,
            match (cast_first, explicit) {
                (true, true) => "target_cast_first_explicit",
                (false, true) => "target_math_first_explicit",
                (true, false) => "target_cast_first_default",
                (false, false) => "target_math_first_default",
            },
            include_str!("target_float_cast/Host.rs.txt"),
            include_str!("target_float_cast/Host.cs.txt"),
        )?;
    }
    Ok(())
}
