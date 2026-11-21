//! Three admitted syntax variants retain the unchanged literal lookup catalog.
use std::path::{Path, PathBuf};

use engine::EngineError;
use functions::FunctionError;
use ir::Value;

#[path = "offline_composition/hosts.rs"]
mod hosts;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-array-lookup")
        .join(name)
}

fn import_design(design: &str) -> (hosts::TempDir, mapping::Project) {
    let directory = hosts::TempDir::new("lookup-syntax").unwrap();
    for name in [
        "primary.schema.json",
        "lookup.schema.json",
        "target.schema.json",
    ] {
        std::fs::copy(fixture(name), directory.0.join(name)).unwrap();
    }
    let path = directory.0.join("mapping.mfd");
    std::fs::write(&path, design).unwrap();
    let imported = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    (directory, imported.project)
}

fn variant(reverse: bool, keyless: bool) -> (hosts::TempDir, mapping::Project) {
    // Change only the equality operand positions and/or the unused false slot.
    let mut design = std::fs::read_to_string(fixture("mapping.mfd")).unwrap();
    let mut replace = |from: &str, to: &str| {
        assert_eq!(design.matches(from).count(), 1, "{from}");
        design = design.replacen(from, to, 1);
    };
    if reverse {
        replace(
            "<datapoint pos=\"0\" key=\"40\"/><datapoint pos=\"1\" key=\"41\"/>",
            "<datapoint pos=\"0\" key=\"41\"/><datapoint pos=\"1\" key=\"40\"/>",
        );
    }
    if keyless {
        replace("<datapoint pos=\"1\" key=\"53\"/>", "<datapoint/>");
    }
    import_design(&design)
}

fn display_variant() -> (hosts::TempDir, mapping::Project) {
    // The same source key feeds the equality operand and an uppercased display
    // value. Distinct wire keys can have the same display spelling.
    let design = include_str!("fixtures/json-array-lookup/mapping.mfd")
        .replace(
            "</children>",
            r#"<component name="uppercase" library="lang" kind="5"><sources><datapoint pos="0" key="60"/></sources><targets><datapoint pos="0" key="61"/></targets></component></children>"#,
        )
        .replace(r#"<edge vertexkey="31"/>"#, r#"<edge vertexkey="60"/>"#)
        .replace(
            "</vertices>",
            r#"<vertex vertexkey="61"><edges><edge vertexkey="31"/></edges></vertex></vertices>"#,
        );
    import_design(&design)
}

#[test]
fn casing_a_display_field_preserves_the_distinct_lookup_keys_in_each_row() {
    let (_directory, project) = display_variant();
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/json-array-lookup/display-cases.json"
    ))
    .unwrap();
    for case in cases {
        let source = format_json::from_str(&case["primary"].to_string(), &project.source).unwrap();
        let mut parameters = engine::RuntimeParameters::new();
        parameters
            .insert(
                "LookupText",
                Value::String(case["lookup"].as_str().unwrap().into()),
            )
            .unwrap();
        let context =
            engine::ExecutionContext::new(Path::new("mapping.mfd")).with_parameters(&parameters);
        let output = engine::run_with_context(&project, &source, &context).unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap())
                .unwrap();
        assert_eq!(actual, case["expected"], "{}", case["name"]);
    }
}

#[test]
#[ignore = "compiles generated Rust and C# Release consumers; requires .NET 10"]
fn generated_lookup_display_keeps_wire_keys() {
    let (_directory, project) = display_variant();
    hosts::run(
        &project,
        &fixture("display-cases.json"),
        "lookup-display",
        include_str!("offline_composition/lookup.rs.txt"),
        include_str!("offline_composition/lookup.cs.txt"),
    );
}

#[test]
fn admitted_syntax_combinations_preserve_every_literal_success_and_typed_failure() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/json-array-lookup/cases.json")).unwrap();
    assert_eq!(cases.len(), 19);
    for (reverse, keyless) in [(true, false), (false, true), (true, true)] {
        let (_directory, project) = variant(reverse, keyless);
        hosts::ordinary_artifacts(&project);
        for case in cases.iter().chain(cases.iter().rev()) {
            let mut parameters = engine::RuntimeParameters::new();
            match case["parameter_failure"].as_str() {
                Some("missing") => {}
                Some("null") => parameters.insert("LookupText", Value::Null).unwrap(),
                Some("int") => parameters.insert("LookupText", Value::Int(7)).unwrap(),
                None => parameters
                    .insert(
                        "LookupText",
                        Value::String(case["lookup"].as_str().unwrap().into()),
                    )
                    .unwrap(),
                other => panic!("unknown parameter control {other:?}"),
            }
            let context = engine::ExecutionContext::new(Path::new("mapping.mfd"))
                .with_parameters(&parameters);
            let source =
                format_json::from_str(&case["primary"].to_string(), &project.source).unwrap();
            let result = engine::run_with_context(&project, &source, &context);
            match case["parameter_failure"].as_str() {
                Some("missing") => assert!(
                    matches!(result, Err(EngineError::MissingRuntimeParameter { name, .. }) if name == "LookupText"),
                    "{case}"
                ),
                Some("null") => assert_eq!(
                    result.unwrap_err(),
                    EngineError::Function(FunctionError::TypeMismatch {
                        function: "json_parse_array_length",
                        got: "null"
                    }),
                    "{case}"
                ),
                Some("int") => assert_eq!(
                    result.unwrap_err(),
                    EngineError::Function(FunctionError::InvalidArgument {
                        function: "json_parse_array_length",
                        message: "input does not match the JSON schema"
                    }),
                    "{case}"
                ),
                None if case.get("error").is_some() => assert!(
                    matches!(result, Err(EngineError::Function(FunctionError::InvalidArgument { function: "json_parse_array_length", message })) if Some(message) == case["error"].as_str()),
                    "{case}"
                ),
                None => {
                    let actual = result.unwrap();
                    assert_eq!(
                        actual,
                        // Keep the existing JSON-target typed contract: a bound
                        // missing scalar is Null, distinct from explicit JsonNull.
                        format_json::from_str(&case["expected"].to_string(), &project.target)
                            .unwrap(),
                        "reverse={reverse} keyless={keyless}: {case}"
                    );
                    let json: serde_json::Value = serde_json::from_str(
                        &format_json::to_string(&project.target, &actual).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(json, case["expected"], "{case}");
                }
                other => panic!("unknown parameter control {other:?}"),
            }
        }
    }
}

#[test]
#[ignore = "compiles three ordinary generated Rust and C# Release hosts; needs resource clearance and .NET 10"]
fn generated_lookup_syntax_release_hosts_match_every_literal_case() {
    for (reverse, keyless) in [(true, false), (false, true), (true, true)] {
        let (_directory, project) = variant(reverse, keyless);
        hosts::run(
            &project,
            &fixture("cases.json"),
            "lookup",
            include_str!("offline_composition/lookup.rs.txt"),
            include_str!("offline_composition/lookup.cs.txt"),
        );
    }
}
