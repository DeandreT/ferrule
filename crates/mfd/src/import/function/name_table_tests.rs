use super::{map_component_name, read};

#[test]
fn every_export_row_round_trips_through_its_component_library() {
    for (ferrule, mfd, library) in crate::function_names::export_names() {
        let xml = format!(r#"<component name="{mfd}" library="{library}" kind="5"/>"#);
        let document = roxmltree::Document::parse(&xml).expect("authored component XML");
        let component = read(&document.root_element());
        if ferrule == "round_half_ceiling" {
            // The name-only helper has always passed this name through; the
            // native exporter instead chooses round/round-precision by arity.
            assert_eq!(map_component_name(&component), None);
            for native_name in ["round", "round-precision"] {
                let xml = format!(r#"<component name="{native_name}" library="core" kind="5"/>"#);
                let document = roxmltree::Document::parse(&xml).expect("authored rounding XML");
                assert_eq!(
                    map_component_name(&read(&document.root_element())),
                    Some(ferrule)
                );
            }
        } else {
            assert_eq!(
                map_component_name(&component),
                Some(ferrule),
                "{library}.{mfd}"
            );
        }
    }
}
