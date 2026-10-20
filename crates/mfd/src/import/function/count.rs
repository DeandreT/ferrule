use super::*;

/// Exact raw signature of a sequence count: `core` kind 5 `count` with a
/// keyless optional context slot at input 0, one keyed sequence input at 1,
/// and one keyed primary output. General aggregate lowering accepts looser
/// spellings; this flag does not.
pub(super) fn signature_valid(
    component: &roxmltree::Node<'_, '_>,
    inputs: &[Option<u32>],
    output_pins: &[Option<u32>],
) -> bool {
    if !matches!(
        (
            component.attribute("library"),
            component.attribute("kind"),
            component.attribute("name"),
        ),
        (Some("core"), Some("5"), Some("count"))
    ) || !exact_component_children(component, &["sources", "targets"])
    {
        return false;
    }
    let mut containers = component
        .children()
        .filter(|node| node.has_tag_name("sources"));
    let (Some(sources), None) = (containers.next(), containers.next()) else {
        return false;
    };
    let points = sources
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [context, sequence] = points.as_slice() else {
        return false;
    };
    let Some(sequence_key) = parse_u32(sequence.attribute("key")) else {
        return false;
    };
    sources.attributes().len() == 0
        && !sources
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
        && context.has_tag_name("datapoint")
        && sequence.has_tag_name("datapoint")
        && (exact_metadata_leaf(*context, &[])
            || exact_metadata_leaf(*context, &["pos"]) && context.attribute("pos") == Some("0"))
        && exact_metadata_leaf(*sequence, &["pos", "key"])
        && sequence.attribute("pos") == Some("1")
        && inputs == [None, Some(sequence_key)]
        && exact_substitution_pins(component, "targets", 1)
            .is_some_and(|keys| output_pins == [Some(keys[0])] && sequence_key != keys[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(text: &str) -> bool {
        let document = roxmltree::Document::parse(text).unwrap();
        read(&document.root_element()).raw_count_signature_valid
    }

    #[test]
    fn count_signature_is_an_exact_context_slot_and_keyed_sequence_contract() {
        let count = r#"<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="61"/></sources><targets><datapoint pos="0" key="60"/></targets></component>"#;
        for text in [
            count.to_string(),
            count.replace("<datapoint/>", "<datapoint pos=\"0\"/>"),
            count.replace(
                "<datapoint pos=\"0\" key=\"60\"/>",
                "<datapoint key=\"60\"/>",
            ),
            count.replace(" kind=\"5\"", " kind=\"5\" uid=\"9\""),
        ] {
            assert!(valid(&text), "{text}");
        }
        for text in [
            count.replace("library=\"core\"", "library=\"lang\""),
            count.replace("kind=\"5\"", "kind=\"4\""),
            count.replace("name=\"count\"", "name=\"sum\""),
            count.replace(" kind=\"5\"", " kind=\"5\" other=\"1\""),
            count.replace("<datapoint/>", ""),
            count.replace("<datapoint/>", "<datapoint key=\"62\"/>"),
            count.replace("<datapoint/>", "<datapoint pos=\"0\" key=\"62\"/>"),
            count.replace("<datapoint/>", "<datapoint pos=\"1\"/>"),
            count.replace("<datapoint/>", "<datapoint pos=\"bad\"/>"),
            count.replace("<datapoint/>", "<datapoint><hidden/></datapoint>"),
            count.replace("<datapoint/>", "<other/>"),
            count.replace(" pos=\"1\" key=\"61\"", " key=\"61\""),
            count.replace(" key=\"61\"", ""),
            count.replace("pos=\"1\" key=\"61\"", "pos=\"2\" key=\"61\""),
            count.replace("key=\"61\"", "key=\"bad\""),
            count.replace("key=\"61\"", "key=\"60\""),
            count.replace("key=\"61\"/>", "key=\"61\" type=\"x\"/>"),
            count.replace("</sources>", "<datapoint pos=\"2\" key=\"63\"/></sources>"),
            count.replace("</sources>", "</sources><sources/>"),
            count.replace("<sources>", "<sources kind=\"x\">"),
            count.replace("<sources>", "<sources>text"),
            count.replace("</targets>", "</targets><targets/>"),
            count.replace("</targets>", "</targets><data/>"),
            count.replace(
                "<datapoint pos=\"0\" key=\"60\"/>",
                "<datapoint pos=\"0\" key=\"60\"/><datapoint pos=\"1\" key=\"64\"/>",
            ),
            count.replace("pos=\"0\" key=\"60\"", "pos=\"1\" key=\"60\""),
            count.replace(" key=\"60\"", ""),
        ] {
            assert!(!valid(&text), "{text}");
        }
    }
}
