use super::*;

#[test]
fn conditional_raw_ownership_counts_skipped_owners_and_primary_frame_ports() {
    let source = r#"<component name="source" library="json" kind="31"><data><root><entry outkey="10"><entry outkey="11"/><entry outkey="12"/><entry outkey="13"/></entry></root></data></component>"#;
    let conditional = r#"<component name="if-else" library="core" kind="4"><sources><datapoint pos="0" key="111"/><datapoint pos="1" key="112"/><datapoint pos="2" key="113"/></sources><targets><datapoint key="101"/></targets></component>"#;
    let edges = r#"<edge from="11" to="111"/><edge from="12" to="112"/><edge from="13" to="113"/>"#;
    for (source, extra, extra_edges, expected) in [
        (source.to_string(), String::new(), "", true),
        (source.to_string(), String::new(), r#"<edge from="12" to="112"/>"#, true),
        (source.to_string(), String::new(), r#"<edge from="13" to="112"/>"#, false),
        (source.to_string(), r#"<component library="unknown" kind="999"><targets><datapoint key="101"/></targets></component>"#.into(), "", false),
        (source.to_string(), r#"<component library="unknown" kind="999"><targets><datapoint key="12"/></targets></component>"#.into(), "", false),
        (source.to_string(), r#"<component library="unknown" kind="999"><data><root><entry outkey="11"/></root></data></component>"#.into(), "", false),
        (source.to_string(), r#"<component library="unknown" kind="999"><targets><datapoint key="10"/></targets></component>"#.into(), "", false),
        (source.replace("</data>", "<root><entry outkey=\"12\"/></root></data>"), String::new(), "", false),
        (source.to_string(), r#"<component name="embedded-definition"><structure><children><component library="unknown"><targets><datapoint key="10"/><datapoint key="12"/></targets></component></children></structure></component>"#.into(), "", true),
    ] {
        let text = format!(r#"<mapping><component name="map"><structure><children>{source}{conditional}{extra}</children><connections>{edges}{extra_edges}</connections></structure></component></mapping>"#);
        let document = roxmltree::Document::parse(&text).unwrap();
        let component = document.descendants().find(|node| node.has_tag_name("component")
            && node.attribute("name") == Some("if-else")).unwrap();
        let raw = read(&component).conditional_ownership;
        assert_eq!(raw.is_some(), expected, "{text}");
        if let Some(raw) = raw {
            assert_eq!(raw.output, 101);
            assert_eq!(raw.sources, [11, 12, 13]);
            assert_eq!(raw.primary_outputs, BTreeSet::from([10, 11, 12, 13]));
        }
    }
}
