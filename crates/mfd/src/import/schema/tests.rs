use std::path::Path;

use super::{instance_root_segments, normalize_xml_entry_name, read_json_component};

#[test]
fn instance_root_paths_do_not_split_namespace_uris() {
    assert_eq!(
        instance_root_segments(
            "{http://example.com/people}People/{http://example.com/people}Person"
        ),
        ["People", "Person"]
    );
    assert_eq!(
        instance_root_segments("{}People/{}Person"),
        ["People", "Person"]
    );
}

#[test]
fn indexed_xml_entry_names_are_normalized_without_touching_qnames() {
    assert_eq!(normalize_xml_entry_name("0:Person"), ("Person", false));
    assert_eq!(normalize_xml_entry_name("12:@type"), ("type", true));
    assert_eq!(normalize_xml_entry_name("Person"), ("Person", false));
    assert_eq!(normalize_xml_entry_name("ns:Person"), ("ns:Person", false));
}

#[test]
fn json_lines_component_sets_runtime_format_option_without_a_downgrade_warning() {
    let document = roxmltree::Document::parse(
        r#"<component name="Rows"><data><root><entry name="FileInstance"><entry name="document"><entry name="root" outkey="1"/></entry></entry></root><json jsonlines="1" inputinstance="rows.jsonl"/></data></component>"#,
    )
    .unwrap();
    let mut warnings = Vec::new();
    let component = read_json_component(
        &document.root_element(),
        Path::new("mapping.mfd"),
        &mut warnings,
    )
    .unwrap();

    assert!(component.options.json_lines);
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.contains("JSON Lines")),
        "{warnings:?}"
    );
}
