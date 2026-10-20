use super::*;

/// Exact raw signature of a host-supplied clock: `lang` `now` or `xpath2`
/// `current-dateTime`, both kind 5, with no input declarations and one keyed
/// primary output. General lowering accepts looser spellings; this flag does not.
pub(super) fn signature_valid(
    component: &roxmltree::Node<'_, '_>,
    output_pins: &[Option<u32>],
) -> bool {
    matches!(
        (
            component.attribute("library"),
            component.attribute("kind"),
            component.attribute("name"),
        ),
        (Some("lang"), Some("5"), Some("now"))
            | (Some("xpath2"), Some("5"), Some("current-dateTime"))
    ) && exact_component_children(component, &["sources", "targets"])
        && exact_substitution_pins(component, "sources", 0).is_some()
        && exact_substitution_pins(component, "targets", 1)
            .is_some_and(|keys| output_pins == [Some(keys[0])])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(text: &str) -> bool {
        let document = roxmltree::Document::parse(text).unwrap();
        let component = read(&document.root_element());
        component.raw_clock_signature_valid
    }

    #[test]
    fn clock_signature_is_an_exact_inputless_single_output_contract() {
        let now = r#"<component name="now" library="lang" kind="5"><targets><datapoint pos="0" key="4"/></targets></component>"#;
        let current = r#"<component name="current-dateTime" library="xpath2" kind="5"><targets><datapoint pos="0" key="4"/></targets></component>"#;
        for text in [
            now.to_string(),
            current.to_string(),
            now.replace(" pos=\"0\"", ""),
            now.replace("<targets>", "<sources/><targets>"),
            current.replace("<targets>", "<sources></sources><targets>"),
            now.replace(" kind=\"5\"", " kind=\"5\" uid=\"9\""),
        ] {
            assert!(valid(&text), "{text}");
        }
        for text in [
            now.replace("library=\"lang\"", "library=\"foreign\""),
            now.replace("library=\"lang\"", "library=\"core\""),
            now.replace("kind=\"5\"", "kind=\"4\""),
            now.replace("name=\"now\"", "name=\"current-dateTime\""),
            current.replace("name=\"current-dateTime\"", "name=\"now\""),
            current.replace("library=\"xpath2\"", "library=\"lang\""),
            now.replace(
                "<targets>",
                "<sources><datapoint pos=\"0\" key=\"1\"/></sources><targets>",
            ),
            now.replace("<targets>", "<sources><datapoint/></sources><targets>"),
            now.replace("<targets>", "<sources/><sources/><targets>"),
            now.replace("<targets>", "<sources kind=\"x\"/><targets>"),
            now.replace("</targets>", "</targets><targets/>"),
            now.replace(
                "<datapoint pos=\"0\" key=\"4\"/>",
                "<datapoint pos=\"0\" key=\"4\"/><datapoint pos=\"1\" key=\"5\"/>",
            ),
            now.replace(
                "<datapoint pos=\"0\" key=\"4\"/>",
                "<datapoint pos=\"0\" key=\"4\"/><datapoint pos=\"0\" key=\"5\"/>",
            ),
            now.replace("<datapoint pos=\"0\" key=\"4\"/>", ""),
            now.replace(" key=\"4\"", ""),
            now.replace("pos=\"0\"", "pos=\"1\""),
            now.replace("pos=\"0\"", "pos=\"bad\""),
            now.replace("key=\"4\"", "key=\"bad\""),
            now.replace("pos=\"0\" key=\"4\"", "pos=\"0\" key=\"4\" type=\"x\""),
            now.replace("</targets>", "</targets><data/>"),
            now.replace("</targets>", "</targets>text"),
            now.replace(" kind=\"5\"", " kind=\"5\" other=\"1\""),
        ] {
            assert!(!valid(&text), "{text}");
        }
    }
}
