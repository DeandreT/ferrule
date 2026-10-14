use super::*;

#[derive(Clone)]
pub(in crate::import) struct RawParseDateOwnership {
    pub(in crate::import) inputs: [u32; 2],
    pub(in crate::import) output: u32,
    pub(in crate::import) source: u32,
    pub(in crate::import) picture: u32,
    pub(in crate::import) picture_value: String,
    pub(in crate::import) primary_outputs: BTreeSet<u32>,
}

pub(super) fn pins_valid(
    component: &roxmltree::Node<'_, '_>,
    inputs: &[Option<u32>],
    outputs: &[Option<u32>],
) -> bool {
    component.tag_name().namespace().is_none()
        && component.attribute("library") == Some("core")
        && component.attribute("kind") == Some("5")
        && component.attribute("name") == Some("parse-date")
        && component.attributes().all(|attribute| attribute.namespace().is_none() && matches!(attribute.name(), "library" | "kind" | "name"))
        && exact_children(*component, &["sources", "targets"])
        && exact_pins(component, "sources", 2).is_some_and(|keys| {
            inputs == [Some(keys[0]), Some(keys[1])]
                && exact_pins(component, "targets", 1).is_some_and(|output| {
                    outputs == [Some(output[0])] && !keys.contains(&output[0])
                })
        })
}

fn exact_children(node: roxmltree::Node<'_, '_>, names: &[&str]) -> bool {
    let children = node.children().filter(roxmltree::Node::is_element).collect::<Vec<_>>();
    children.len() == names.len()
        && names.iter().all(|name| children.iter().filter(|child| child.has_tag_name(*name)).count() == 1)
        && !node.children().any(|child| child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()))
}

fn exact_pins(component: &roxmltree::Node<'_, '_>, tag: &str, count: usize) -> Option<Vec<u32>> {
    let container = component.children().find(|node| node.has_tag_name(tag))?;
    if container.attributes().len() != 0
        || container.children().any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
        || container.children().filter(roxmltree::Node::is_element).any(|point| {
            point.tag_name().namespace().is_some()
                || !point.has_tag_name("datapoint")
                || point.attribute("key").is_none()
                || point.attributes().any(|attribute| attribute.namespace().is_some() || !matches!(attribute.name(), "key" | "pos"))
                || point.children().any(|child| child.is_element() || child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()))
        })
    {
        return None;
    }
    exact_pin_keys(component, tag, count)
}

fn output_key(node: roxmltree::Node<'_, '_>) -> Option<u32> {
    parse_u32(node.attribute("outkey")).or_else(|| {
        (node.has_tag_name("datapoint") && node.ancestors().any(|parent| parent.has_tag_name("targets")))
            .then(|| parse_u32(node.attribute("key")))
            .flatten()
    })
}

fn input_key(node: roxmltree::Node<'_, '_>) -> Option<u32> {
    parse_u32(node.attribute("inpkey")).or_else(|| {
        (node.has_tag_name("datapoint") && node.ancestors().any(|parent| parent.has_tag_name("sources")))
            .then(|| parse_u32(node.attribute("key")))
            .flatten()
    })
}

pub(super) fn ownership(component: &roxmltree::Node<'_, '_>) -> Option<RawParseDateOwnership> {
    let structure = component.ancestors().find(|node| node.has_tag_name("structure"))?;
    let nodes = structure.descendants().filter(|node| {
        node.ancestors().find(|parent| parent.has_tag_name("structure")) == Some(structure)
    }).collect::<Vec<_>>();
    let inputs = exact_pins(component, "sources", 2)?;
    let output = exact_pins(component, "targets", 1)?[0];
    let connected = |input| {
        if nodes.iter().filter(|node| input_key(**node) == Some(input)).count() != 1 {
            return None;
        }
        let edges = nodes.iter().filter(|node| node.has_tag_name("edge") && parse_u32(node.attribute("to")) == Some(input)).collect::<Vec<_>>();
        let [edge] = edges.as_slice() else { return None; };
        parse_u32(edge.attribute("from"))
    };
    let source = connected(inputs[0])?;
    let picture = connected(inputs[1])?;
    let owner = |key| {
        let owners = nodes.iter().filter(|node| output_key(**node) == Some(key))
            .filter_map(|node| node.ancestors().find(|parent| parent.has_tag_name("component")))
            .collect::<Vec<_>>();
        let [owner] = owners.as_slice() else { return None; };
        Some(*owner)
    };
    let source_owner = owner(source)?;
    let picture_owner = owner(picture)?;
    if owner(output)? != *component
        || !ordinary_json_owner(source_owner)
        || !literal_string_owner(picture_owner)
    {
        return None;
    }
    let primary_outputs = source_owner.descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
        .collect::<BTreeSet<_>>();
    if primary_outputs.iter().any(|key| owner(*key) != Some(source_owner)) {
        return None;
    }
    let picture_value = picture_owner.descendants().find(|node| node.has_tag_name("constant"))?.attribute("value")?.to_string();
    Some(RawParseDateOwnership { inputs: [inputs[0], inputs[1]], output, source, picture, picture_value, primary_outputs })
}

fn ordinary_json_owner(component: roxmltree::Node<'_, '_>) -> bool {
    if component.tag_name().namespace().is_some()
        || component.attribute("library") != Some("json")
        || component.attribute("kind") != Some("31")
        || component.attributes().any(|attribute| attribute.namespace().is_some() || !matches!(attribute.name(), "name" | "library" | "kind"))
        || !exact_children(component, &["data"])
    {
        return false;
    }
    let data = component.children().find(|node| node.has_tag_name("data")).unwrap();
    if !exact_children(data, &["root", "json"]) { return false; }
    let json = data.children().find(|node| node.has_tag_name("json")).unwrap();
    if json.tag_name().namespace().is_some()
        || json.attribute("schema").is_none_or(|schema| schema.is_empty())
        || json.attributes().any(|attribute| !matches!(attribute.name(), "schema" | "inputinstance"))
        || json.children().any(|node| node.is_element())
    {
        return false;
    }
    let root = data.children().find(|node| node.has_tag_name("root")).unwrap();
    root.descendants().filter(roxmltree::Node::is_element).all(|node| {
        node.tag_name().namespace().is_none()
            && (node == root || node.has_tag_name("entry"))
            && node.attributes().all(|attribute| attribute.namespace().is_none() && matches!(attribute.name(), "name" | "type" | "outkey"))
    })
}

fn literal_string_owner(component: roxmltree::Node<'_, '_>) -> bool {
    if component.tag_name().namespace().is_some()
        || component.attribute("library") != Some("core")
        || component.attribute("kind") != Some("2")
        || component.attribute("name") != Some("constant")
        || component.attributes().any(|attribute| attribute.namespace().is_some() || !matches!(attribute.name(), "name" | "library" | "kind"))
        || !exact_children(component, &["targets", "data"])
        || exact_pins(&component, "targets", 1).is_none()
    { return false; }
    let data = component.children().find(|node| node.has_tag_name("data")).unwrap();
    if !exact_children(data, &["constant"]) { return false; }
    let literal = data.children().find(|node| node.has_tag_name("constant")).unwrap();
    literal.tag_name().namespace().is_none()
        && literal.attribute("datatype") == Some("string")
        && literal.attribute("value").is_some()
        && literal.attributes().all(|attribute| attribute.namespace().is_none() && matches!(attribute.name(), "value" | "datatype"))
        && !literal.children().any(|node| node.is_element() || node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/enclosing-frame-date-producers/literal-picture.mfd");

    fn with_component(text: &str, run: impl FnOnce(FnComponent)) {
        let document = roxmltree::Document::parse(text).unwrap();
        let component = document.descendants().find(|node| node.has_tag_name("component") && node.attribute("name") == Some("parse-date")).unwrap();
        run(read(&component));
    }

    #[test]
    fn parse_date_raw_two_inputs_and_one_output_are_a_dedicated_contract() {
        with_component(FIXTURE, |component| {
            assert!(component.raw_parse_date_pins_valid && component.raw_single_output_valid);
            assert!(!component.bounded_scalar_pins_valid);
            let raw = component.parse_date_ownership.unwrap();
            assert_eq!(raw.inputs, [111, 112]);
            assert_eq!((raw.output, raw.source, raw.picture), (102, 11, 101));
            assert_eq!(raw.picture_value, "[Y]-[M]-[D]");
            assert_eq!(raw.primary_outputs, BTreeSet::from([10, 11, 12]));
        });
        let fragment = r#"<component name="parse-date" library="core" kind="5"><sources><datapoint pos="0" key="1"/><datapoint pos="1" key="2"/></sources><targets><datapoint key="3"/></targets></component>"#;
        for text in [
            fragment.replace("pos=\"1\"", "pos=\"2\""),
            fragment.replace("pos=\"1\"", "pos=\"0\""),
            fragment.replace("pos=\"0\"", "pos=\"bad\""),
            fragment.replace("key=\"2\"", "key=\"1\""),
            fragment.replace("key=\"2\"", "key=\"3\""),
            fragment.replace(" key=\"2\"", ""),
            fragment.replace("</sources>", "<datapoint pos=\"2\" key=\"4\"/></sources>"),
            fragment.replace("</sources>", "</sources><sources/>"),
            fragment.replace("</targets>", "</targets><targets/>"),
            fragment.replace("<sources>", "<sources><hidden/>"),
            fragment.replace("key=\"2\"/>", "key=\"2\"><hidden/></datapoint>"),
            fragment.replace("key=\"2\"", "key=\"2\" alias=\"foreign\""),
            fragment.replace("library=\"core\"", "library=\"xpath2\""),
            fragment.replace("kind=\"5\"", "kind=\"4\""),
            fragment.replace("name=\"parse-date\"", "name=\"parse_date\""),
        ] {
            let document = roxmltree::Document::parse(&text).unwrap();
            assert!(!read(&document.root_element()).raw_parse_date_pins_valid, "{text}");
        }
    }

    #[test]
    fn parse_date_ownership_census_includes_skipped_hidden_and_competing_producers() {
        for key in [10, 11, 12, 101, 102] {
            for shadow in [
                format!(r#"<component name="unknown" library="unlowered" kind="999"><targets><datapoint key="{key}"/></targets></component>"#),
                format!(r#"<component name="unknown" library="xml" kind="999"><data><root><entry name="Hidden" outkey="{key}"/></root></data></component>"#),
                format!(r#"<component name="unknown" library="unlowered" kind="999"><targets><hidden><datapoint key="{key}"/></hidden></targets></component>"#),
                format!(r#"<component name="constant" library="core" kind="2"><targets><datapoint key="{key}"/></targets><data><constant value="shadow" datatype="string"/></data></component>"#),
            ] {
                let text = FIXTURE.replace("</children>", &format!("{shadow}</children>"));
                with_component(&text, |component| assert!(component.parse_date_ownership.is_none(), "key {key}: {shadow}"));
            }
        }
        for text in [
            FIXTURE.replace("<edge from=\"11\" to=\"111\"/>", ""),
            FIXTURE.replace("<edge from=\"11\" to=\"111\"/>", "<edge from=\"11\" to=\"111\"/><edge from=\"12\" to=\"111\"/>"),
            FIXTURE.replace("<edge from=\"11\" to=\"111\"/>", "<edge from=\"101\" to=\"111\"/>"),
            FIXTURE.replace("<edge from=\"101\" to=\"112\"/>", "<edge from=\"12\" to=\"112\"/>"),
            FIXTURE.replace("<json schema=", "<json jsonlines=\"1\" schema="),
            FIXTURE.replace("</data></component>", "<unknown/></data></component>"),
        ] {
            with_component(&text, |component| assert!(component.parse_date_ownership.is_none(), "{text}"));
        }
    }
}
