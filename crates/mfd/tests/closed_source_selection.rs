use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/closed-source-selection")
        .join(name)
}

#[test]
fn native_design_and_resave_select_expanded_names_and_report_root_mismatch() {
    for design in ["probe.mfd", "saved.mfd"] {
        let project = mfd::import_with_profile(
            &fixture(design),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap()
        .imported
        .project;
        assert!(project.source.xml_closed_source_selection, "{design}");
        assert!(!project.target.xml_closed_source_selection);
        for (file, expected) in [
            (
                "input.xml",
                "<Results><Row><Inner><Count>7</Count></Inner></Row><Row/><Row><Inner><Count>9</Count></Inner></Row></Results>",
            ),
            ("qualified-root.xml", "<Results/>"),
            (
                "qualified-child.xml",
                "<Results><Row><Inner/></Row></Results>",
            ),
            (
                "qualified-before-valid.xml",
                "<Results><Row><Inner><Count>9</Count></Inner></Row></Results>",
            ),
        ] {
            let mut warnings = Vec::new();
            let input = format_xml::from_str_with_warnings(
                &std::fs::read_to_string(fixture(file)).unwrap(),
                &project.source,
                &Default::default(),
                |warning| warnings.push(warning),
            )
            .unwrap();
            if file == "qualified-root.xml" {
                assert_eq!(
                    warnings,
                    [
                        "The selected XML root is '{}Input'. But the input file contains the root element '{urn:other}Input'. No data will be selected."
                    ]
                );
            } else {
                assert!(warnings.is_empty(), "{warnings:?}");
            }
            let output = engine::run(&project, &input).unwrap();
            let xml = format_xml::to_string(&project.target, &output).unwrap();
            assert_eq!(
                format_xml::from_str(&xml, &project.target).unwrap(),
                format_xml::from_str(expected, &project.target).unwrap(),
                "{design} {file}: {xml}",
            );
        }
        // Malformed documents and a genuinely different root still fail.
        for xml in ["<q:Input xmlns:q='urn:other'><bad></q:Input>", "<Other/>"] {
            assert!(format_xml::from_str(xml, &project.source).is_err());
        }
    }
}

#[test]
fn xsd_backed_selection_survives_export_and_reimport() -> Result<(), Box<dyn std::error::Error>> {
    let dir = test_support::TempDir::new("closed_source_selection_roundtrip")?;
    let imported = mfd::import(&fixture("probe.mfd"))?;
    let path = dir.join("roundtrip.mfd");
    mfd::export(&imported.project, &path)?;
    let reopened = mfd::import(&path)?;
    assert_eq!(reopened.warnings, imported.warnings);
    assert!(reopened.project.source.xml_closed_source_selection);
    let xml = std::fs::read_to_string(fixture("qualified-root.xml"))?;
    let mut warnings = Vec::new();
    let selected = format_xml::from_str_with_warnings(
        &xml,
        &reopened.project.source,
        &Default::default(),
        |warning| warnings.push(warning),
    )?;
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        selected
            .field("Row")
            .and_then(ir::Instance::as_repeated)
            .map(|rows| rows.len()),
        Some(0)
    );
    Ok(())
}
