use super::*;

#[derive(Clone)]
pub(in crate::import) struct RawGroupByOwnership {
    pub(in crate::import) inputs: [u32; 2],
    pub(in crate::import) outputs: [u32; 2],
    pub(in crate::import) rows: u32,
    pub(in crate::import) key: u32,
    pub(in crate::import) literal: Option<Value>,
    pub(in crate::import) primary_outputs: BTreeSet<u32>,
    pub(in crate::import) consumers: [BTreeSet<u32>; 2],
}

fn children(node: roxmltree::Node<'_, '_>, names: &[&str]) -> bool {
    let items = node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    items.len() == names.len()
        && names
            .iter()
            .all(|name| items.iter().filter(|item| item.has_tag_name(*name)).count() == 1)
        && node
            .children()
            .all(|item| !item.is_text() || item.text().is_none_or(|text| text.trim().is_empty()))
}

fn pins(component: &roxmltree::Node<'_, '_>, tag: &str, count: usize) -> Option<Vec<u32>> {
    let container = component.children().find(|node| node.has_tag_name(tag))?;
    if container.tag_name().namespace().is_some()
        || container.attributes().len() != 0
        || container
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
        || container
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|point| {
                point.tag_name().namespace().is_some()
                    || !point.has_tag_name("datapoint")
                    || point.attributes().any(|attribute| {
                        attribute.namespace().is_some()
                            || !matches!(attribute.name(), "key" | "pos")
                    })
                    || point.children().any(|child| {
                        child.is_element()
                            || child.is_text()
                                && child.text().is_some_and(|text| !text.trim().is_empty())
                    })
            })
    {
        return None;
    }
    exact_pin_keys(component, tag, count)
}

fn output(node: roxmltree::Node<'_, '_>) -> Option<u32> {
    parse_u32(node.attribute("outkey")).or_else(|| {
        (node.has_tag_name("datapoint")
            && node
                .ancestors()
                .any(|parent| parent.has_tag_name("targets")))
        .then(|| parse_u32(node.attribute("key")))
        .flatten()
    })
}

fn input(node: roxmltree::Node<'_, '_>) -> Option<u32> {
    parse_u32(node.attribute("inpkey")).or_else(|| {
        (node.has_tag_name("datapoint")
            && node
                .ancestors()
                .any(|parent| parent.has_tag_name("sources")))
        .then(|| parse_u32(node.attribute("key")))
        .flatten()
    })
}

fn ordinary_json(component: roxmltree::Node<'_, '_>) -> bool {
    if component.tag_name().namespace().is_some()
        || component.attribute("library") != Some("json")
        || component.attribute("kind") != Some("31")
        || component.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "name" | "library" | "kind")
        })
        || !children(component, &["data"])
    {
        return false;
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .unwrap();
    if !children(data, &["root", "json"]) {
        return false;
    }
    let json = data
        .children()
        .find(|node| node.has_tag_name("json"))
        .unwrap();
    let root = data
        .children()
        .find(|node| node.has_tag_name("root"))
        .unwrap();
    json.tag_name().namespace().is_none()
        && json
            .attribute("schema")
            .is_some_and(|schema| !schema.is_empty())
        && json.attributes().all(|attribute| {
            attribute.namespace().is_none()
                && matches!(attribute.name(), "schema" | "inputinstance")
        })
        && !json.children().any(|child| child.is_element())
        && root
            .descendants()
            .filter(roxmltree::Node::is_element)
            .all(|node| {
                node.tag_name().namespace().is_none()
                    && (node == root || node.has_tag_name("entry"))
                    && node.attributes().all(|attribute| {
                        attribute.namespace().is_none()
                            && matches!(attribute.name(), "name" | "type" | "outkey")
                    })
            })
}

// This positive route does not prove function/control composition. Both output
// ports must terminate at plain final XML projections before either key API may
// allocate an adapter. The compatibility importer keeps its independent route.
fn ordinary_final_xml(component: roxmltree::Node<'_, '_>) -> bool {
    if component.tag_name().namespace().is_some()
        || component.attribute("library") != Some("xml")
        || component.attribute("kind") != Some("14")
        || component.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "name" | "library" | "kind")
        })
        || !children(component, &["properties", "data"])
    {
        return false;
    }
    let properties = component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .unwrap();
    if properties.tag_name().namespace().is_some()
        || properties.attributes().len() != 1
        || properties.attribute("XSLTDefaultOutput") != Some("1")
        || properties
            .attributes()
            .any(|attribute| attribute.namespace().is_some())
        || properties.children().any(|node| {
            node.is_element()
                || node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
        })
    {
        return false;
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .unwrap();
    if data.tag_name().namespace().is_some()
        || data.attributes().len() != 0
        || !children(data, &["root", "document"])
    {
        return false;
    }
    let document = data
        .children()
        .find(|node| node.has_tag_name("document"))
        .unwrap();
    if document.tag_name().namespace().is_some()
        || document.attributes().len() != 3
        || document.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(
                    attribute.name(),
                    "schema" | "outputinstance" | "instanceroot"
                )
                || attribute.value().is_empty()
        })
        || document.children().any(|node| {
            node.is_element()
                || node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
        })
    {
        return false;
    }
    let root = data
        .children()
        .find(|node| node.has_tag_name("root"))
        .unwrap();
    if root.tag_name().namespace().is_some()
        || root.attributes().len() != 0
        || !children(root, &["entry"])
    {
        return false;
    }
    let payload = root
        .children()
        .find(|node| node.has_tag_name("entry"))
        .unwrap();
    if payload.attribute("name").is_none_or(|name| {
        document.attribute("instanceroot") != Some(format!("{{}}{name}").as_str())
    }) {
        return false;
    }
    root.descendants()
        .filter(|node| node.is_element() && *node != root)
        .all(|node| {
            node.tag_name().namespace().is_none()
                && node.has_tag_name("entry")
                && node.attribute("name").is_some_and(|name| {
                    !name.is_empty() && !matches!(name, "compute-when" | "FileInstance")
                })
                && node.attributes().all(|attribute| {
                    attribute.namespace().is_none() && matches!(attribute.name(), "name" | "inpkey")
                })
                && node.children().all(|child| {
                    !child.is_text() || child.text().is_none_or(|text| text.trim().is_empty())
                })
        })
}

fn output_consumers(
    component: roxmltree::Node<'_, '_>,
    outputs: &[u32],
    feeds: &std::collections::BTreeMap<u32, u32>,
    nodes: &[roxmltree::Node<'_, '_>],
) -> Option<[BTreeSet<u32>; 2]> {
    let structure = component
        .ancestors()
        .find(|node| node.has_tag_name("structure"))?;
    // Match import_resolved's selected immediate component container. A strict
    // target elsewhere in the document may be skipped by ordinary lowering.
    let children = structure
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "children")?;
    if component.parent() != Some(children) {
        return None;
    }
    let mut consumers = [BTreeSet::new(), BTreeSet::new()];
    for (position, &source) in outputs.iter().enumerate() {
        for edge in nodes {
            let connection = parse_u32(edge.attribute("from")) == Some(source);
            let graph = edge.has_tag_name("edge")
                && edge.ancestors().any(|node| {
                    node.has_tag_name("vertex")
                        && parse_u32(node.attribute("vertexkey")) == Some(source)
                });
            if !connection && !graph {
                continue;
            }
            if !edge.has_tag_name("edge")
                || edge.tag_name().namespace().is_some()
                || edge.children().any(|child| {
                    child.is_element()
                        || child.is_text()
                            && child.text().is_some_and(|text| !text.trim().is_empty())
                })
            {
                return None;
            }
            let destination = if connection && !graph {
                let connections = edge.parent()?;
                if !connections.has_tag_name("connections")
                    || connections.tag_name().namespace().is_some()
                    || ![Some(structure), structure.parent()].contains(&connections.parent())
                    || edge.attributes().len() != 2
                    || edge.attributes().any(|attribute| {
                        attribute.namespace().is_some()
                            || !matches!(attribute.name(), "from" | "to")
                    })
                {
                    return None;
                }
                parse_u32(edge.attribute("to"))?
            } else if graph && !connection {
                let edges = edge.parent()?;
                let vertex = edges.parent()?;
                let vertices = vertex.parent()?;
                let graph = vertices.parent()?;
                if !edges.has_tag_name("edges")
                    || edges.tag_name().namespace().is_some()
                    || edges.attributes().len() != 0
                    || !vertex.has_tag_name("vertex")
                    || vertex.tag_name().namespace().is_some()
                    || vertex.attributes().len() != 1
                    || vertex.attributes().any(|attribute| {
                        attribute.namespace().is_some() || attribute.name() != "vertexkey"
                    })
                    || parse_u32(vertex.attribute("vertexkey")) != Some(source)
                    || !vertices.has_tag_name("vertices")
                    || vertices.tag_name().namespace().is_some()
                    || vertices.attributes().len() != 0
                    || !graph.has_tag_name("graph")
                    || graph.tag_name().namespace().is_some()
                    || graph.parent() != Some(structure)
                    || graph.attributes().len() != 0
                    || edge.attributes().len() != 1
                    || edge.attributes().any(|attribute| {
                        attribute.namespace().is_some() || attribute.name() != "vertexkey"
                    })
                {
                    return None;
                }
                parse_u32(edge.attribute("vertexkey"))?
            } else {
                return None;
            };
            if feeds.get(&destination) != Some(&source) {
                return None;
            }
            let declarations = nodes
                .iter()
                .copied()
                .filter(|node| input(*node) == Some(destination))
                .collect::<Vec<_>>();
            let [declaration] = declarations.as_slice() else {
                return None;
            };
            let owner = declaration
                .ancestors()
                .find(|node| node.has_tag_name("component"))?;
            if owner.parent() != Some(children)
                || !ordinary_final_xml(owner)
                || !declaration.has_tag_name("entry")
                || declaration
                    .attribute("inpkey")
                    .and_then(|key| key.parse::<u32>().ok())
                    != Some(destination)
                || declaration
                    .ancestors()
                    .take_while(|node| *node != owner)
                    .all(|node| !node.has_tag_name("root"))
                || (position == 0)
                    != declaration
                        .children()
                        .any(|node| node.has_tag_name("entry"))
            {
                return None;
            }
            consumers[position].insert(destination);
        }
        if consumers[position].is_empty()
            || feeds
                .iter()
                .filter_map(|(&destination, &feed)| (feed == source).then_some(destination))
                .collect::<BTreeSet<_>>()
                != consumers[position]
        {
            return None;
        }
    }
    Some(consumers)
}

pub(super) fn ownership(component: &roxmltree::Node<'_, '_>) -> Option<RawGroupByOwnership> {
    if component.tag_name().namespace().is_some()
        || component.attribute("library") != Some("core")
        || component.attribute("kind") != Some("5")
        || component.attribute("name") != Some("group-by")
        || component.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "name" | "library" | "kind")
        })
        || !children(*component, &["sources", "targets"])
    {
        return None;
    }
    let inputs = pins(component, "sources", 2)?;
    let outputs = pins(component, "targets", 2)?;
    if inputs.iter().any(|key| outputs.contains(key)) {
        return None;
    }
    let structure = component
        .ancestors()
        .find(|node| node.has_tag_name("structure"))?;
    let parent = structure.parent();
    let edges = super::super::graph::read_edge_inputs(&structure, parent.as_ref());
    let rows = *edges.feeds.get(&inputs[0])?;
    let key = *edges.feeds.get(&inputs[1])?;
    let nodes = component
        .document()
        .root()
        .descendants()
        .collect::<Vec<_>>();
    let owner = |key| {
        let declarations = nodes
            .iter()
            .filter(|node| output(**node) == Some(key))
            .collect::<Vec<_>>();
        let [declaration] = declarations.as_slice() else {
            return None;
        };
        declaration
            .ancestors()
            .find(|node| node.has_tag_name("component"))
    };
    if inputs.iter().any(|key| {
        nodes
            .iter()
            .filter(|node| input(**node) == Some(*key))
            .count()
            != 1
    }) || outputs.iter().any(|key| owner(*key) != Some(*component))
    {
        return None;
    }
    let consumers = output_consumers(*component, &outputs, &edges.feeds, &nodes)?;
    let primary = owner(rows)?;
    if !ordinary_json(primary) {
        return None;
    }
    let primary_outputs = primary
        .descendants()
        .filter_map(output)
        .collect::<BTreeSet<_>>();
    if primary_outputs
        .iter()
        .any(|key| owner(*key) != Some(primary))
    {
        return None;
    }
    let key_owner = owner(key)?;
    let literal = if key_owner == primary {
        None
    } else {
        if key_owner.tag_name().namespace().is_some()
            || key_owner.attribute("library") != Some("core")
            || key_owner.attribute("kind") != Some("2")
            || key_owner.attribute("name") != Some("constant")
            || key_owner.attributes().any(|attribute| {
                attribute.namespace().is_some()
                    || !matches!(attribute.name(), "name" | "library" | "kind")
            })
            || !children(key_owner, &["targets", "data"])
            || pins(&key_owner, "targets", 1)?.as_slice() != [key]
            || !exact_literal_data(&key_owner)
        {
            return None;
        }
        let datum = key_owner
            .descendants()
            .find(|node| node.has_tag_name("constant"))?;
        let value = parse_constant(datum.attribute("value")?, datum.attribute("datatype")?);
        if !matches!(value, Value::String(_) | Value::Int(_) | Value::Bool(_)) {
            return None;
        }
        Some(value)
    };
    Some(RawGroupByOwnership {
        inputs: [inputs[0], inputs[1]],
        outputs: [outputs[0], outputs[1]],
        rows,
        key,
        literal,
        primary_outputs,
        consumers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/present-string-group-keys/string.mfd"),
        )
        .unwrap()
    }

    #[test]
    fn exact_positions_and_whole_document_owners_are_not_normalized() {
        let original = fixture();
        for (from, to) in [
            ("pos=\"1\" key=\"112\"", "pos=\"2\" key=\"112\""),
            ("pos=\"1\" key=\"112\"", "pos=\"0\" key=\"112\""),
            ("pos=\"1\" key=\"112\"", "pos=\"1\" key=\"111\""),
            ("pos=\"1\" key=\"112\"", "pos=\"1\""),
            (
                "<datapoint pos=\"1\" key=\"112\"/>",
                "<datapoint pos=\"1\" key=\"112\"><hidden/></datapoint>",
            ),
            ("<edge from=\"11\" to=\"112\"/>", ""),
            (
                "<edge from=\"11\" to=\"112\"/>",
                "<edge from=\"11\" to=\"112\"/><edge from=\"12\" to=\"112\"/>",
            ),
            (
                "</children>",
                "<component name=\"ignored\" library=\"unknown\" kind=\"999\"><targets><datapoint key=\"11\"/></targets></component></children>",
            ),
            (
                "</children>",
                "<component name=\"ignored\" library=\"unknown\" kind=\"999\"><targets><datapoint key=\"122\"/></targets></component></children>",
            ),
        ] {
            assert!(original.contains(from));
            let text = original.replace(from, to);
            let document = roxmltree::Document::parse(&text).unwrap();
            let group = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("group-by")
                })
                .unwrap();
            assert!(ownership(&group).is_none(), "{from} -> {to}");
        }
    }

    #[test]
    fn complete_raw_consumers_refuse_unproved_controls_and_target_owners() {
        let original = fixture();
        let control = r#"<component name="distinct-values" library="core" kind="5"><sources><datapoint pos="0" key="131"/></sources><targets><datapoint pos="0" key="132"/></targets></component>"#;
        let with_control = original.replace("</children>", &format!("{control}</children>"));
        for output in [121, 122] {
            for unsafe_first in [false, true] {
                let edge = format!(r#"<edge from="{output}" to="131"/>"#);
                let text = if unsafe_first {
                    with_control.replace("<connections>", &format!("<connections>{edge}"))
                } else {
                    with_control.replace("</connections>", &format!("{edge}</connections>"))
                };
                let document = roxmltree::Document::parse(&text).unwrap();
                let group = document
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("component") && node.attribute("name") == Some("group-by")
                    })
                    .unwrap();
                assert!(
                    ownership(&group).is_none(),
                    "output={output}, unsafe_first={unsafe_first}"
                );
            }
        }
        for (from, to) in [
            (
                r#"<properties XSLTDefaultOutput="1"/>"#,
                r#"<properties XSLTDefaultOutput="1" PassThrough="1"/>"#,
            ),
            (
                r#"<properties XSLTDefaultOutput="1"/>"#,
                r#"<properties XSLTDefaultOutput="1"/><parameter usageKind="variable"/>"#,
            ),
            (
                r#"<entry name="Key" inpkey="31"/>"#,
                r#"<entry name="compute-when" inpkey="31"/>"#,
            ),
            (
                r#"<entry name="Key" inpkey="31"/>"#,
                r#"<entry name="FileInstance" inpkey="31"/>"#,
            ),
            (
                r#"<entry name="Key" inpkey="31"/>"#,
                r#"<entry name="Key" inpkey="31" outkey="300"/>"#,
            ),
            (
                r#"<entry name="Group" inpkey="30">"#,
                r#"<entry name="Group" inpkey="30" condition="unproved">"#,
            ),
            (
                r#"<document schema="target.xsd""#,
                r#"<document inputinstance="source.xml" schema="target.xsd""#,
            ),
            (
                r#"library="xml" kind="14""#,
                r#"library="unknown" kind="999""#,
            ),
            (
                r#"<entry name="Summary">"#,
                r#"<entry name="wrapper"><entry name="Summary">"#,
            ),
            (
                r#"<edge from="121" to="30"/>"#,
                r#"<edge from="121" to="930"/>"#,
            ),
            (
                r#"<edge from="121" to="30"/>"#,
                r#"<edge from="121" to="30"/><edge from="12" to="30"/>"#,
            ),
            (
                "</children>",
                r#"<component name="ignored" library="unknown" kind="999"><data><entry name="hidden" inpkey="30"/></data></component></children>"#,
            ),
        ] {
            assert!(original.contains(from));
            let mut text = original.replace(from, to);
            if from == r#"<entry name="Summary">"# {
                text = text.replace("</root><document", "</entry></root><document");
            }
            let document = roxmltree::Document::parse(&text).unwrap();
            let group = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("group-by")
                })
                .unwrap();
            assert!(ownership(&group).is_none(), "{from} -> {to}");
        }
    }

    #[test]
    fn shadowed_consumers_are_refused_and_identical_both_vocabulary_edges_are_idempotent() {
        let original = fixture();
        let safe_graph = r#"<graph><vertices><vertex vertexkey="121"><edges><edge vertexkey="30"/></edges></vertex><vertex vertexkey="122"><edges><edge vertexkey="31"/></edges></vertex></vertices></graph>"#;
        let duplicate = original
            .replace("</structure>", &format!("{safe_graph}</structure>"))
            .replace(
                r#"<edge from="121" to="30"/>"#,
                r#"<edge from="121" to="30"/><edge from="121" to="30"/>"#,
            );
        let document = roxmltree::Document::parse(&duplicate).unwrap();
        let group = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("group-by")
            })
            .unwrap();
        let raw = ownership(&group).unwrap();
        assert_eq!(raw.consumers, [BTreeSet::from([30]), BTreeSet::from([31])]);
        for hidden in [
            r#"<connections><edge from="121" to="131"/></connections>"#,
            r#"<graph><vertices><vertex vertexkey="121"><edges><edge vertexkey="131"/></edges></vertex></vertices></graph>"#,
            r#"<connections><edge from="122" to="131"/></connections>"#,
        ] {
            let text = original.replace(
                "</structure></component>",
                &format!("</structure>{hidden}</component>"),
            );
            let document = roxmltree::Document::parse(&text).unwrap();
            let group = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("group-by")
                })
                .unwrap();
            let structure = group
                .ancestors()
                .find(|node| node.has_tag_name("structure"))
                .unwrap();
            let normalized = super::super::super::graph::read_edge_inputs(
                &structure,
                structure.parent().as_ref(),
            );
            assert!(!normalized.feeds.contains_key(&131));
            assert!(normalized.conflicts.is_empty());
            assert!(ownership(&group).is_none(), "{hidden}");
        }
    }

    #[test]
    fn strict_xml_consumers_must_belong_to_the_selected_immediate_container() {
        let original = fixture();
        let document = roxmltree::Document::parse(&original).unwrap();
        let target = document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("report"))
            .unwrap();
        let report = &original[target.range()];
        let nested = original.replace(report, &format!(r#"<component name="ignored" library="unknown" kind="999"><children>{report}</children></component>"#));
        let other = original.replace(report, "").replace("</mapping>", &format!(r#"<component name="other"><structure><children>{report}</children></structure></component></mapping>"#));
        let shadowed = original.replace(report, "").replace(
            "</children><connections>",
            &format!("</children><children>{report}</children><connections>"),
        );
        for text in [nested, other, shadowed] {
            let document = roxmltree::Document::parse(&text).unwrap();
            let group = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("group-by")
                })
                .unwrap();
            let target = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some("report")
                })
                .unwrap();
            assert!(
                ordinary_final_xml(target),
                "the target body remains strict and unchanged"
            );
            let structure = group
                .ancestors()
                .find(|node| node.has_tag_name("structure"))
                .unwrap();
            let normalized = super::super::super::graph::read_edge_inputs(
                &structure,
                structure.parent().as_ref(),
            );
            assert_eq!(normalized.feeds.get(&30), Some(&121));
            assert_eq!(normalized.feeds.get(&31), Some(&122));
            assert!(normalized.conflicts.is_empty());
            assert!(ownership(&group).is_none());
        }
    }
}
