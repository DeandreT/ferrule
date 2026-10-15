use super::*;

const MAP: &str = r#"<component name="value-map" library="core" kind="23"><sources><datapoint pos="0" key="1"/></sources><targets><datapoint key="2"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable><entry from="D" to="first"/><entry from="D" to="second"/><entry from="" to=""/></valuemapTable><input name="input" type="string"/><result name="result" type="string" defaultValue=""/></valuemap></data></component>"#;

#[test]
fn total_string_metadata_preserves_duplicate_empty_and_enabled_default_order() {
    for text in [
        MAP.to_string(),
        MAP.replace("defaultValueMode=\"custom\"", "enableDefaultValue=\"1\""),
    ] {
        let document = roxmltree::Document::parse(&text).unwrap();
        let component = read(&document.root_element());
        assert!(component.bounded_scalar_pins_valid && component.raw_single_output_valid);
        let map = component.valuemap.unwrap();
        let proof = map.total_string.unwrap();
        assert_eq!(
            proof.table,
            [
                ("D".into(), "first".into()),
                ("D".into(), "second".into()),
                (String::new(), String::new())
            ]
        );
        assert_eq!(proof.default, "");
        assert_eq!(map.default, Some(Value::String(String::new())));
    }
}

#[test]
fn total_string_metadata_refuses_missing_typed_default_and_hidden_subtrees() {
    for text in [
        MAP.replace("defaultValueMode=\"custom\"", "defaultValueMode=\"none\""),
        MAP.replace("defaultValueMode=\"custom\"", ""),
        MAP.replace("defaultValueMode=\"custom\"", "enableDefaultValue=\"0\""),
        MAP.replace(
            "defaultValueMode=\"custom\"",
            "defaultValueMode=\"custom\" enableDefaultValue=\"1\"",
        ),
        MAP.replace(" defaultValue=\"\"", ""),
        MAP.replace(
            "name=\"result\" type=\"string\"",
            "name=\"result\" type=\"null\"",
        ),
        MAP.replace(
            "name=\"input\" type=\"string\"",
            "name=\"input\" type=\"integer\"",
        ),
        MAP.replace(
            "name=\"input\" type=\"string\"",
            "name=\"input\" type=\"unknown\"",
        ),
        MAP.replace("name=\"input\" type=\"string\"", "name=\"input\""),
        MAP.replace("from=\"D\" to=\"first\"", "to=\"first\""),
        MAP.replace("from=\"D\" to=\"first\"", "from=\"D\""),
        MAP.replace(
            "from=\"D\" to=\"first\"",
            "from=\"D\" to=\"first\" alias=\"x\"",
        ),
        MAP.replace(
            "<entry from=\"D\" to=\"first\"/>",
            "<hidden><entry from=\"D\" to=\"first\"/></hidden>",
        ),
        MAP.replace("</valuemapTable>", "text</valuemapTable>"),
        MAP.replace("</valuemapTable>", "</valuemapTable><valuemapTable/>"),
        MAP.replace(
            "</valuemap>",
            "<input name=\"input\" type=\"string\"/></valuemap>",
        ),
        MAP.replace("</valuemap>", "<unknown/></valuemap>"),
        MAP.replace(
            "</data>",
            "<constant value=\"x\" datatype=\"string\"/></data>",
        ),
        MAP.replace("</data>", "</data><data/>"),
    ] {
        let document = roxmltree::Document::parse(&text).unwrap();
        assert!(
            read(&document.root_element())
                .valuemap
                .unwrap()
                .total_string
                .is_none(),
            "{text}"
        );
    }
}

#[test]
fn total_string_metadata_refuses_normalized_pin_aliases_and_shadows() {
    for text in [
        MAP.replace("pos=\"0\" key=\"1\"", "pos=\"bad\" key=\"1\""),
        MAP.replace("pos=\"0\" key=\"1\"", "pos=\"1\" key=\"1\""),
        MAP.replace("pos=\"0\" key=\"1\"", "pos=\"0\""),
        MAP.replace(
            "pos=\"0\" key=\"1\"",
            "pos=\"0\" key=\"1\" alias=\"foreign\"",
        ),
        MAP.replace(
            "<datapoint key=\"2\"/>",
            "<datapoint key=\"2\"/><datapoint key=\"3\"/>",
        ),
        MAP.replace(
            "<datapoint key=\"2\"/>",
            "<datapoint pos=\"0\" key=\"3\"/><datapoint pos=\"0\" key=\"2\"/>",
        ),
        MAP.replace("<datapoint key=\"2\"/>", "<datapoint key=\"1\"/>"),
        MAP.replace("</sources>", "</sources><sources/>"),
        MAP.replace("</targets>", "</targets><targets/>"),
    ] {
        let document = roxmltree::Document::parse(&text).unwrap();
        let component = read(&document.root_element());
        assert!(
            !component.bounded_scalar_pins_valid
                || !component.raw_single_output_valid
                || component.valuemap.unwrap().total_string.is_none(),
            "{text}"
        );
    }
}

#[test]
fn total_string_metadata_entry_and_decoded_utf8_budgets_have_exact_boundaries() {
    let template = MAP.replace("<entry from=\"D\" to=\"first\"/><entry from=\"D\" to=\"second\"/><entry from=\"\" to=\"\"/>", "ENTRIES");
    for (count, expected) in [(0, true), (256, true), (257, false)] {
        let text = template.replace("ENTRIES", &"<entry from=\"\" to=\"\"/>".repeat(count));
        let document = roxmltree::Document::parse(&text).unwrap();
        assert_eq!(
            read(&document.root_element())
                .valuemap
                .unwrap()
                .total_string
                .is_some(),
            expected
        );
    }
    for (value, expected) in [
        ("a".repeat(65_536), true),
        ("a".repeat(65_537), false),
        ("&#945;".repeat(32_768), true),
        ("&#945;".repeat(32_769), false),
    ] {
        let text = template
            .replace("ENTRIES", "")
            .replace("defaultValue=\"\"", &format!("defaultValue=\"{value}\""));
        let document = roxmltree::Document::parse(&text).unwrap();
        assert_eq!(
            read(&document.root_element())
                .valuemap
                .unwrap()
                .total_string
                .is_some(),
            expected
        );
    }
}

#[test]
fn unsupported_total_proof_does_not_change_general_value_map_normalization() {
    let text = MAP.replace(
        "name=\"input\" type=\"string\"",
        "name=\"input\" type=\"unknown\"",
    );
    let document = roxmltree::Document::parse(&text).unwrap();
    let map = read(&document.root_element()).valuemap.unwrap();
    assert!(map.total_string.is_none());
    assert_eq!(map.input_type, Some(ScalarType::String));
    assert_eq!(
        map.table[0],
        (Value::String("D".into()), Value::String("first".into()))
    );
    assert_eq!(map.default, Some(Value::String(String::new())));
}

#[test]
fn total_string_raw_ownership_counts_skipped_owners_and_every_saved_root() {
    let source = r#"<component name="source" library="json" kind="31"><data><root><entry outkey="10"/></root></data></component>"#;
    let wrap = |source: &str, extra: &str, edges: &str| {
        format!(
            r#"<mapping><component name="map"><structure><children>{source}{MAP}{extra}</children><connections>{edges}</connections></structure></component></mapping>"#
        )
    };
    for (source, extra, edges, expected) in [
        (source.to_string(), String::new(), r#"<edge from="10" to="1"/>"#, true),
        (source.to_string(), String::new(), r#"<edge from="10" to="1"/><edge from="10" to="1"/>"#, true),
        (source.to_string(), r#"<component library="unknown" kind="999"><targets><datapoint key="2"/></targets></component>"#.into(), r#"<edge from="10" to="1"/>"#, false),
        (source.to_string(), r#"<component library="unknown" kind="999"><targets><datapoint key="10"/></targets></component>"#.into(), r#"<edge from="10" to="1"/>"#, false),
        (source.to_string(), r#"<component library="unknown" kind="999"><data><root><entry outkey="2"/></root></data></component>"#.into(), r#"<edge from="10" to="1"/>"#, false),
        (source.to_string(), r#"<component library="unknown" kind="999"><data><root><entry outkey="10"/></root></data></component>"#.into(), r#"<edge from="10" to="1"/>"#, false),
        (source.replace("</data>", "<root><entry outkey=\"10\"/></root></data>"), String::new(), r#"<edge from="10" to="1"/>"#, false),
        (source.to_string(), String::new(), r#"<edge from="10" to="1"/><edge from="11" to="1"/>"#, false),
        (source.to_string(), r#"<component name="embedded-definition"><structure><children><component library="unknown"><targets><datapoint key="2"/><datapoint key="10"/></targets></component></children></structure></component>"#.into(), r#"<edge from="10" to="1"/>"#, true),
    ] {
        let text = wrap(&source, &extra, edges);
        let document = roxmltree::Document::parse(&text).unwrap();
        let node = document.descendants().find(|node| node.has_tag_name("component") && node.attribute("name") == Some("value-map")).unwrap();
        let raw = read(&node).valuemap.unwrap().total_string.unwrap();
        assert_eq!(raw.ownership.is_some(), expected, "{text}");
        if let Some(owner) = raw.ownership { assert_eq!((owner.source, owner.output), (10, 2)); }
    }
}
