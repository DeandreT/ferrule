//! A renamed native literal keeps its kind-2 identity, not its display name.

use roxmltree::Node;

use super::{exact_component_children, exact_metadata_leaf, exact_substitution_pins};

pub(super) fn is_supported_literal(component: &Node<'_, '_>) -> bool {
    if component.attribute("library") != Some("core")
        || component.attribute("kind") != Some("2")
        || !exact_component_children(component, &["sources", "targets", "data", "view"])
        || exact_substitution_pins(component, "sources", 0).is_none()
        || exact_substitution_pins(component, "targets", 1).is_none()
    {
        return false;
    }
    let views = component
        .children()
        .filter(|node| node.has_tag_name("view"))
        .collect::<Vec<_>>();
    if views.len() > 1
        || views.iter().any(|view| {
            view.tag_name().namespace().is_some()
                || view.attributes().any(|attr| {
                    attr.namespace().is_some()
                        || !matches!(attr.name(), "ltx" | "lty" | "rbx" | "rby")
                })
                || view.children().any(|node| {
                    node.is_element()
                        || node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
                })
        })
    {
        return false;
    }
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return false;
    };
    let literals = data.children().filter(Node::is_element).collect::<Vec<_>>();
    let [literal] = literals.as_slice() else {
        return false;
    };
    if data.tag_name().namespace().is_some()
        || data.attributes().len() != 0
        || data
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
        || !literal.has_tag_name("constant")
        || !exact_metadata_leaf(*literal, &["datatype", "value"])
    {
        return false;
    }
    let value = literal.attribute("value").unwrap_or_default();
    match literal.attribute("datatype") {
        Some("string") => true,
        Some("integer" | "int" | "long") => value.parse::<i64>().is_ok(),
        Some("decimal" | "double" | "float") => value.parse::<f64>().is_ok_and(f64::is_finite),
        Some("boolean") => matches!(value, "true" | "false" | "1" | "0"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(xml: &str) -> bool {
        let doc = roxmltree::Document::parse(xml).unwrap();
        is_supported_literal(&doc.root_element())
    }

    const BASE: &str = r#"<component name="Renamed" library="core" kind="2"><targets><datapoint pos="0" key="9"/></targets><data><constant datatype="integer" value="29"/></data></component>"#;

    #[test]
    fn renamed_constant_requires_one_complete_literal_boundary() {
        assert!(check(BASE));
        assert!(check(
            &BASE.replace("<data>", "<view ltx=\"10\" rby=\"20\"/><data>")
        ));
        for xml in [
            BASE.replace("library=\"core\"", "library=\"other\""),
            BASE.replace("kind=\"2\"", "kind=\"5\""),
            BASE.replace(
                "<targets>",
                "<sources><datapoint key=\"8\"/></sources><targets>",
            ),
            BASE.replace("</targets>", "<datapoint pos=\"1\" key=\"10\"/></targets>"),
            BASE.replace(
                "</data>",
                "<constant datatype=\"integer\" value=\"30\"/></data>",
            ),
            BASE.replace("<data>", "<data unknown=\"1\">"),
            BASE.replace("<data>", "<data xmlns=\"urn:foreign\">"),
            BASE.replace("<constant ", "<constant xmlns=\"urn:foreign\" "),
            BASE.replace(
                "</targets>",
                "</targets><targets><datapoint key=\"10\"/></targets>",
            ),
            BASE.replace(
                "</component>",
                "<data><constant datatype=\"integer\" value=\"30\"/></data></component>",
            ),
            BASE.replace("<data>", "<view unknown=\"1\"/><data>"),
            BASE.replace("<data>", "<view/><view/><data>"),
            BASE.replace("value=\"29\"", "value=\"invalid\""),
            BASE.replace("datatype=\"integer\"", "datatype=\"unknown\""),
            BASE.replace("<constant ", "<constant extra=\"1\" "),
            BASE.replace(
                "datatype=\"integer\" value=\"29\"",
                "datatype=\"double\" value=\"NaN\"",
            ),
        ] {
            assert!(!check(&xml), "{xml}");
        }
    }
}
