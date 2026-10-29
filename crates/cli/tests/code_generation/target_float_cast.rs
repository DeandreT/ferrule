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

/// Ferrule's own String-to-double contract on the lexical edges where the
/// reference application differs (see docs/conformance/xml-target-casting.md):
/// trimmed input, finite values only, and round-trip positional output.
const LEXICAL_CASES: [(&str, &str, Option<&str>); 28] = [
    ("plus", "+2.5", Some("2.5")),
    ("leading-dot", ".5", Some("0.5")),
    ("trailing-dot", "5.", Some("5")),
    ("upper-exponent", "1E+03", Some("1000")),
    ("leading-space", " 2.5", Some("2.5")),
    ("trailing-space", "2.5 ", Some("2.5")),
    ("empty", "", None),
    ("inf", "INF", None),
    ("negative-inf", "-INF", None),
    ("nan", "NaN", None),
    ("infinity", "Infinity", None),
    ("hex", "0x10", None),
    ("comma", "2,5", None),
    ("overflow", "1e400", None),
    ("underflow", "1e-400", Some("0")),
    ("leading-zeros", "00029", Some("29")),
    ("tenth", "0.1", Some("0.1")),
    (
        "tenth-sum",
        "0.30000000000000004",
        Some("0.30000000000000004"),
    ),
    ("digits-15", "123456789012345", Some("123456789012345")),
    ("digits-16", "1234567890123456", Some("1234567890123456")),
    (
        "digits-18",
        "123456789012345678",
        Some("123456789012345680"),
    ),
    ("e21", "1e21", Some("1000000000000000000000")),
    ("e-7", "1e-7", Some("0.0000001")),
    ("e-4", "0.0001", Some("0.0001")),
    ("pi", "3.141592653589793", Some("3.141592653589793")),
    ("e300", "1.5e300", None),
    ("max", "1.7976931348623157e308", None),
    ("denormal", "4.9406564584124654e-324", None),
];

#[test]
fn target_float_cast_lexical_edges_match_across_backends() -> TestResult<()> {
    let project = project(true, true)?;
    let mut cases = Vec::new();
    for (name, text, pinned) in LEXICAL_CASES {
        let input = format!(
            "<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>{text}</Text><Label>safe</Label></Envelope>"
        );
        let source = format_xml::from_str(&input, &project.source)?;
        let case = match engine::run(&project, &source) {
            Ok(output) => {
                let xml = format_xml::to_string(&project.target, &output)?;
                // Values too long to pin inline are checked for their exact
                // round trip instead.
                let first = xml
                    .split("<Amount>")
                    .nth(1)
                    .and_then(|rest| rest.split("</Amount>").next())
                    .ok_or("first Amount")?;
                match pinned {
                    Some(expected) => assert_eq!(first, expected, "{name}"),
                    None => assert_eq!(
                        first.parse::<f64>()?.to_bits(),
                        text.trim().parse::<f64>()?.to_bits(),
                        "{name}"
                    ),
                }
                serde_json::json!({"name": name, "input": input, "error": null, "expected_xml": xml})
            }
            Err(_) => {
                assert!(
                    pinned.is_none()
                        && text
                            .trim()
                            .parse::<f64>()
                            .map_or(true, |value| !value.is_finite()),
                    "{name} unexpectedly failed"
                );
                serde_json::json!({"name": name, "input": input, "error": "cast"})
            }
        };
        cases.push(case);
    }
    // The non-finite and malformed inputs stay refused rather than written as
    // the reference application's INF/NaN text.
    let refused = cases.iter().filter(|case| case["error"] == "cast").count();
    assert_eq!(refused, 8);
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &project,
        &cases,
        "target_float_lexical_edges",
        include_str!("target_float_cast/Host.rs.txt"),
        include_str!("target_float_cast/Host.cs.txt"),
    )
}
