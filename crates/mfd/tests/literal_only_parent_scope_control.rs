use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/constructed-variable-drivers")
}

fn import_design(resources: &Path, design: &str, label: &str) -> mfd::Imported {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-ordinary-literal-parent-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    for entry in std::fs::read_dir(resources).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
    }
    let path = directory.join("mapping.mfd");
    std::fs::write(&path, design).unwrap();
    let imported = mfd::import(&path).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    imported
}

#[test]
fn ordinary_literal_only_variable_records_refuse_controlled_parent_scopes() {
    let resources = fixtures();
    let compiled =
        format_edi_config::import_config(&resources.join("Envelope.Config"), &["850".into()])
            .unwrap();
    let selected = ["Interchange", "Group", "Message_850", "Loop0100", "N1"]
        .iter()
        .try_fold(&compiled.schema, |node, name| node.child(name))
        .unwrap();
    assert_eq!(selected.name, "N1");
    assert!(!selected.repeating, "the ordinary driver uses one record");

    let original = std::fs::read_to_string(resources.join("nested-edi.mfd")).unwrap();
    let parent_edge = "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex>";
    let count_edge = "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"24\"/></edges></vertex>";
    let count_literal = r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="106"/></targets><data><constant value="0" datatype="integer"/></data></component>"#;
    let parent_control = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="120"/><datapoint pos="1" key="121"/></sources><targets><datapoint pos="0" key="110"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="105"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    assert!(original.contains(parent_edge) && original.contains(count_edge));
    let literal_only = original
        .replace("</children>", &format!("{count_literal}</children>"))
        .replace(
            count_edge,
            r#"<vertex vertexkey="106"><edges><edge vertexkey="24"/></edges></vertex>"#,
        );
    let controlled = literal_only
        .replace("</children>", &format!("{parent_control}</children>"))
        .replace(
            parent_edge,
            r#"<vertex vertexkey="10"><edges><edge vertexkey="120"/></edges></vertex><vertex vertexkey="105"><edges><edge vertexkey="121"/></edges></vertex><vertex vertexkey="110"><edges><edge vertexkey="40"/></edges></vertex>"#,
        );

    // The source schemas, nonrepeating selected N1 and ordinary group connector
    // stay intact. Only the Count assignment and the parent's filter differ.
    let baseline = mfd::import(&resources.join("nested-edi.mfd")).unwrap();
    assert!(baseline.warnings.is_empty(), "{:?}", baseline.warnings);
    let plain = import_design(&resources, &literal_only, "plain");
    assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);
    assert_eq!(plain.project.source, baseline.project.source);
    assert_eq!(plain.project.target, baseline.project.target);
    let imported = import_design(&resources, &controlled, "filtered");
    assert_eq!(imported.project.source, baseline.project.source);
    assert_eq!(imported.project.target, baseline.project.target);
    assert!(
        imported.warnings.iter().any(|warning| warning
            == "iteration into `Batches/Records` comes from an unsupported feed; skipped"),
        "{:?}",
        imported.warnings
    );
}
