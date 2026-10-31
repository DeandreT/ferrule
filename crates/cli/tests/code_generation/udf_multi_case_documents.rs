use super::*;

#[path = "../../../mfd/tests/udf_multi_case_if_else/fixtures.rs"]
mod fixtures;

use fixtures::NestedFailure;

const FULL: &[Option<u32>] = &[Some(1000), Some(1002), Some(1001), Some(1003), Some(1004)];
const INPUTS: [(&str, &str); 4] = [
    (
        "TT",
        "<Input><First>true</First><Second>true</Second></Input>",
    ),
    (
        "TF",
        "<Input><First>true</First><Second>false</Second></Input>",
    ),
    (
        "FT",
        "<Input><First>false</First><Second>true</Second></Input>",
    ),
    (
        "FF",
        "<Input><First>false</First><Second>false</Second></Input>",
    ),
];

// Complete, independently authored document bytes: the public XML writer uses
// a UTF-8 declaration, LF separators, two-space indentation, and no final LF.
// The imported target keeps the reference default schema-location hint.
// Neither a serializer nor an interpreter supplies any expected document.
const A: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>A</Choice>\n</Output>";
const B: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>B</Choice>\n</Output>";
const C: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>C</Choice>\n</Output>";
const B_NESTED: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>B!</Choice>\n</Output>";
const C_NESTED: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>C?</Choice>\n</Output>";
const A_MULTI: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>A</Choice>\n  <Marker>D</Marker>\n</Output>";
const B_MULTI: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>B</Choice>\n  <Marker>D</Marker>\n</Output>";
const C_MULTI: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>C</Choice>\n  <Marker>D</Marker>\n</Output>";
const B_NESTED_MULTI: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>B!</Choice>\n  <Marker>D</Marker>\n</Output>";
const C_NESTED_MULTI: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Choice>C?</Choice>\n  <Marker>D</Marker>\n</Output>";

fn document_cases(multi: bool, nested: bool, bad: Option<NestedFailure>) -> Vec<serde_json::Value> {
    let documents = match (multi, nested) {
        (false, false) => [A, A, B, C],
        (true, false) => [A_MULTI, A_MULTI, B_MULTI, C_MULTI],
        (false, true) => [A, A, B_NESTED, C_NESTED],
        (true, true) => [A_MULTI, A_MULTI, B_NESTED_MULTI, C_NESTED_MULTI],
    };
    let mut cases = INPUTS
        .iter()
        .zip(documents)
        .enumerate()
        .map(|(index, ((name, input), expected))| {
            let selected_bad = matches!(
                (bad, index),
                (Some(NestedFailure::Late), 2) | (Some(NestedFailure::Default), 3)
            );
            if selected_bad {
                serde_json::json!({
                    "case": name,
                    "input": input,
                    "mapping_error": {
                        "function": "format_guid_string",
                        "message": "requires exactly 32 ASCII hexadecimal characters",
                    },
                })
            } else {
                serde_json::json!({"case": name, "input": input, "expected_xml": expected})
            }
        })
        .collect::<Vec<_>>();
    // Even an otherwise selected first branch must not hide a malformed source
    // Boolean or an incomplete document: these fail at the typed input boundary.
    for (name, input) in [
        (
            "malformed-boolean",
            "<Input><First>true</First><Second>not-a-boolean</Second></Input>",
        ),
        (
            "malformed-xml",
            "<Input><First>true</First><Second>false</Second>",
        ),
    ] {
        cases.push(serde_json::json!({"case": name, "input": input, "input_error": true}));
    }
    cases
}

fn import_document(path: &Path, multi: bool, outer_name: &str) -> TestResult<Project> {
    let report = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    assert!(
        report.imported.warnings.is_empty(),
        "{:?}",
        report.imported.warnings
    );
    let mut project = report.imported.project;
    let choice = project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Choice")
        .expect("imported Choice binding")
        .node;
    if multi {
        assert!(matches!(project.graph.nodes[&choice], Node::If { .. }));
        assert!(
            project
                .user_functions
                .values()
                .all(|function| function.name != outer_name),
            "multiple-output outer definition must inline"
        );
    } else {
        let Node::UserFunctionCall { function, .. } = &project.graph.nodes[&choice] else {
            panic!("single-output outer definition must remain a first-class call");
        };
        assert_eq!(project.user_functions[function].name, outer_name);
    }
    // Explicitly select the ordinary public document capability for these
    // authored fixtures; do not change the imported expressions or schemas.
    project.source_options.xml_document = true;
    project.target_options.xml_document = true;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    let boundary = lowered.xml_boundary.expect("public XML document adapters");
    assert_eq!(
        boundary.input.profile(),
        Some(codegen::XmlInputProfile::Structured)
    );
    assert!(boundary.output.declaration && boundary.output.indent);
    assert!(boundary.output.default_namespace.is_none());
    assert_eq!(
        boundary.output.schema_hints,
        Some(ir::XmlSchemaHints {
            no_namespace_location: Some("target.xsd".into()),
            ..Default::default()
        })
    );
    Ok(project)
}

#[test]
fn generated_udf_multi_case_documents_preserve_literal_xml_and_owned_failures() -> TestResult<()> {
    for multi in [false, true] {
        let route = if multi { "inlined" } else { "first_class" };
        let directory = TempDir::new(&format!("udf_multi_case_documents_{route}"))?;
        let simple_path = fixtures::fixture(&directory.0, multi, FULL, None);
        let simple = import_document(&simple_path, multi, "Choose")?;
        json_text_boundaries::run_generated_boundary_cases_with_harnesses(
            &simple,
            &document_cases(multi, false, None),
            &format!("udf_multi_case_documents_ordered_{route}"),
            include_str!("fixtures/udf_multi_case_documents_rust.rs.txt"),
            include_str!("fixtures/udf_multi_case_documents_csharp.cs.txt"),
        )?;
        // One valid nested project and one project for each fallible location.
        // Every project exercises TT/TF/FT/FF through both document APIs, so an
        // unselected invalid call must still return the complete literal XML.
        for (name, bad) in [
            ("nested", None),
            ("nested_bad_late", Some(NestedFailure::Late)),
            ("nested_bad_default", Some(NestedFailure::Default)),
        ] {
            let root = directory.0.join(name);
            std::fs::create_dir(&root)?;
            let path = fixtures::nested_fixture(&root, multi, bad);
            let project = import_document(&path, multi, "ChooseNested")?;
            json_text_boundaries::run_generated_boundary_cases_with_harnesses(
                &project,
                &document_cases(multi, true, bad),
                &format!("udf_multi_case_documents_{name}_{route}"),
                include_str!("fixtures/udf_multi_case_documents_rust.rs.txt"),
                include_str!("fixtures/udf_multi_case_documents_csharp.cs.txt"),
            )?;
        }
    }
    Ok(())
}
