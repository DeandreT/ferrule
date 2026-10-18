use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/literal-group-drivers")
        .join(name)
}

fn assert_documents(path: &Path) {
    let imported = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/literal-group-drivers/cloned-cases.json"
    ))
    .unwrap();
    for case in cases {
        let input =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
        if case["reject"] == true {
            assert!(input.is_err(), "{case}");
            continue;
        }
        let output = engine::run(&imported.project, &input.unwrap()).unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &output).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{case}");
    }
}

#[test]
fn cloned_literals_keep_declaration_order_and_each_parent_context() {
    // Both input-port and feed keys run opposite to declaration order. False and
    // the empty string are present values: each constructs one sibling group.
    assert_documents(&fixture("cloned.mfd"));
}

fn with_design(label: &str, text: &str, check: impl FnOnce(&Path)) {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-cloned-literal-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    for name in ["source.schema.json", "cloned.schema.json"] {
        std::fs::copy(fixture(name), directory.join(name)).unwrap();
    }
    let design = directory.join("mapping.mfd");
    std::fs::write(&design, text).unwrap();
    check(&design);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cloned_literals_reusing_a_feed_still_construct_both_occurrences() {
    let original = std::fs::read_to_string(fixture("cloned.mfd")).unwrap();
    let text = original
        .replace(
            r#"<vertex vertexkey="101"><edges><edge vertexkey="30"/></edges></vertex>"#,
            "",
        )
        .replace(
            r#"<edge vertexkey="40"/>"#,
            r#"<edge vertexkey="40"/><edge vertexkey="30"/>"#,
        );
    assert_ne!(text, original);
    with_design("shared-feed", &text, assert_documents);
}

#[test]
fn cloned_literals_keep_competing_and_incomplete_connectors_refused() {
    let original = std::fs::read_to_string(fixture("cloned.mfd")).unwrap();
    let variants = [
        // A second producer for the same saved input is not another occurrence.
        original.replace(
            r#"<vertex vertexkey="101"><edges><edge vertexkey="30"/>"#,
            r#"<vertex vertexkey="101"><edges><edge vertexkey="40"/><edge vertexkey="30"/>"#,
        ),
        // Both a shared wrapper and the cloned item have connected group ports.
        original
            .replace(
                r#"<entry name="Details" type="json-property"><entry name="array">"#,
                r#"<entry name="Details" type="json-property"><entry name="array" inpkey="43">"#,
            )
            .replace(r#"<edge vertexkey="40"/>"#, r#"<edge vertexkey="40"/><edge vertexkey="43"/>"#),
        // Two same-path connectors inside one declared branch stay ambiguous.
        original
            .replace(r#"<entry name="array"><entry name="item" type="json-item">"#, r#"<entry name="array">"#)
            .replace(r#"<entry name="object" inpkey="40">"#, r#"<entry name="item" type="json-item" inpkey="43"><entry name="object" inpkey="40">"#)
            .replace(r#"<entry name="object" clone="1" inpkey="30">"#, r#"</entry><entry name="item" type="json-item" clone="1"><entry name="object" inpkey="30">"#)
            .replace(r#"<edge vertexkey="40"/>"#, r#"<edge vertexkey="40"/><edge vertexkey="43"/>"#),
        // A branch without any connected contents cannot borrow its sibling's.
        original.replace(r#"<edge vertexkey="31"/>"#, "").replace(r#"<edge vertexkey="32"/>"#, ""),
        // Preserve raw producer and literal validation as well as branch ownership.
        original.replace(r#"value="false" datatype="boolean""#, r#"value="FALSE" datatype="boolean""#),
        original.replace(r#"<datapoint pos="0" key="103"/>"#, r#"<datapoint pos="0" key="103"/><datapoint pos="0" key="103"/>"#),
        // A saved scalar input cannot belong to two sibling declarations.
        original.replace(r#"inpkey="31""#, r#"inpkey="41""#),
    ];
    for (index, text) in variants.into_iter().enumerate() {
        assert_ne!(text, original);
        with_design(&format!("refused-{index}"), &text, |path| {
            let result = mfd::import_with_profile(
                path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            );
            let error = result
                .err()
                .unwrap_or_else(|| panic!("control {index} was admitted"));
            assert!(
                matches!(error, mfd::MfdError::IncompatibleImport(_)),
                "control {index}: {error}"
            );
        });
    }
}
