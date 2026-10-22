use super::*;

/// Exact raw pins and saved ownership of one scalar operator admitted by the
/// conditional group-presence proof: core kind-4 `if-else`, core kind-5
/// `equal`, or lang kind-5 `empty`.
///
/// This is separate from `bounded_scalar_pins_valid`, which requires every
/// conditional input to be keyed. Here only an `if-else` branch (raw
/// positions 1 and 2) may be a keyless `<datapoint/>`, meaning Null.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::import) struct RawPresenceOperator {
    /// Input pins in raw position order; `None` only for a keyless branch.
    pub(in crate::import) inputs: Vec<Option<u32>>,
    pub(in crate::import) output: u32,
    /// The single saved feed of each keyed input, in pin order.
    pub(in crate::import) feeds: Vec<Option<u32>>,
}

pub(super) fn operator(component: &roxmltree::Node<'_, '_>) -> Option<RawPresenceOperator> {
    let (arity, keyless) = match (
        component.attribute("library"),
        component.attribute("kind"),
        component.attribute("name"),
    ) {
        (Some("core"), Some("4"), Some("if-else")) => (3, &[1, 2][..]),
        (Some("core"), Some("5"), Some("equal")) => (2, &[][..]),
        (Some("lang"), Some("5"), Some("empty")) => (1, &[][..]),
        _ => return None,
    };
    if !operator_children(component) {
        return None;
    }
    let inputs = operator_pins(component, "sources", arity, keyless)?;
    let output = match operator_pins(component, "targets", 1, &[])?.as_slice() {
        [Some(output)] => *output,
        _ => return None,
    };
    let mut keys = BTreeSet::from([output]);
    if inputs.iter().flatten().any(|key| !keys.insert(*key)) {
        return None;
    }
    let siblings = component.parent()?;
    let structure = siblings
        .parent()
        .filter(|node| node.has_tag_name("structure"))?;
    let owners = || {
        siblings
            .children()
            .filter(|node| node.has_tag_name("component"))
    };
    let count = |key| super::super::constructed_parser_scalar::declared_output_count(owners(), key);
    if count(output) != 1 {
        return None;
    }
    let parent = structure.parent();
    let edges = super::super::graph::read_edge_inputs(&structure, parent.as_ref());
    let mut feeds = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let feed = match input {
            Some(input) => {
                let feed = *edges.feeds.get(input)?;
                if count(feed) != 1 {
                    return None;
                }
                Some(feed)
            }
            None => None,
        };
        feeds.push(feed);
    }
    Some(RawPresenceOperator {
        inputs,
        output,
        feeds,
    })
}

/// Saved operators carry their pins plus optional empty properties and
/// layout. Any other metadata, including priority contexts, is refused.
fn operator_children(component: &roxmltree::Node<'_, '_>) -> bool {
    let attributes_known = component.attributes().all(|attribute| {
        attribute.namespace().is_none()
            && matches!(attribute.name(), "name" | "library" | "kind" | "uid")
    });
    let mut seen = BTreeSet::new();
    attributes_known
        && component.tag_name().namespace().is_none()
        && component.children().all(|node| {
            if node.is_text() {
                return node.text().is_none_or(|text| text.trim().is_empty());
            }
            if !node.is_element() {
                return true;
            }
            let name = node.tag_name().name();
            node.tag_name().namespace().is_none()
                && seen.insert(name)
                && match name {
                    "sources" | "targets" | "view" => true,
                    "properties" => {
                        node.attributes().len() == 0
                            && !node.children().any(|child| {
                                child.is_element()
                                    || child.is_text()
                                        && child.text().is_some_and(|text| !text.trim().is_empty())
                            })
                    }
                    _ => false,
                }
        })
        && seen.contains("sources")
        && seen.contains("targets")
}

fn operator_pins(
    component: &roxmltree::Node<'_, '_>,
    tag: &str,
    count: usize,
    keyless: &[usize],
) -> Option<Vec<Option<u32>>> {
    let container = component.children().find(|node| node.has_tag_name(tag))?;
    if container.attributes().len() != 0
        || container
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return None;
    }
    let points = container
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if points.len() != count {
        return None;
    }
    let mut pins = Vec::with_capacity(count);
    for (index, point) in points.into_iter().enumerate() {
        if !point.has_tag_name("datapoint") {
            return None;
        }
        // Any written raw position must match the pin's index. A keyless pin
        // is saved with no other attribute.
        let keyed = point.attribute("key").is_some();
        let positioned = point.attribute("pos").is_some();
        let attributes: &[&str] = match (keyed, positioned) {
            (true, true) => &["pos", "key"],
            (true, false) => &["key"],
            (false, true) => &["pos"],
            (false, false) => &[],
        };
        if !exact_metadata_leaf(point, attributes)
            || positioned
                && parse_u32(point.attribute("pos"))
                    .and_then(|position| usize::try_from(position).ok())
                    != Some(index)
        {
            return None;
        }
        if !keyed {
            if !keyless.contains(&index) {
                return None;
            }
            pins.push(None);
            continue;
        }
        pins.push(Some(parse_u32(point.attribute("key"))?));
    }
    Some(pins)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(component: &str, extra: &str) -> String {
        format!(
            r#"<mapping><component name="map"><structure><children>{component}
<component name="constant" library="core" kind="2"><targets><datapoint key="1"/></targets><data><constant value="true" datatype="boolean"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="2"/></targets><data><constant value="x" datatype="string"/></data></component>
<component name="constant" library="core" kind="2"><targets><datapoint key="3"/></targets><data><constant value="y" datatype="string"/></data></component>
{extra}</children><connections><edge from="1" to="11"/><edge from="2" to="12"/><edge from="3" to="13"/></connections></structure></component></mapping>"#
        )
    }

    fn read_operator(text: &str) -> Option<RawPresenceOperator> {
        let document = roxmltree::Document::parse(text).unwrap();
        let component = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component")
                    && matches!(node.attribute("name"), Some("if-else" | "equal" | "empty"))
            })
            .unwrap();
        read(&component).presence_operator
    }

    const KEYLESS_ELSE: &str = r#"<component name="if-else" library="core" uid="4" kind="4"><properties/><sources><datapoint pos="0" key="11"/><datapoint pos="1" key="12"/><datapoint/></sources><targets><datapoint pos="0" key="19"/></targets><view ltx="1" lty="2" rbx="3" rby="4"/></component>"#;

    #[test]
    fn keyless_branches_keep_their_raw_positions() {
        assert_eq!(
            read_operator(&wrap(KEYLESS_ELSE, "")),
            Some(RawPresenceOperator {
                inputs: vec![Some(11), Some(12), None],
                output: 19,
                feeds: vec![Some(1), Some(2), None],
            })
        );
        let positioned = KEYLESS_ELSE.replace(
            "<datapoint/></sources>",
            r#"<datapoint pos="2"/></sources>"#,
        );
        assert_eq!(
            read_operator(&wrap(&positioned, "")).map(|operator| operator.inputs),
            Some(vec![Some(11), Some(12), None])
        );
        let keyless_then = KEYLESS_ELSE
            .replace(r#"<datapoint pos="1" key="12"/>"#, "<datapoint/>")
            .replace(
                "<datapoint/></sources>",
                r#"<datapoint pos="2" key="13"/></sources>"#,
            );
        assert_eq!(
            read_operator(&wrap(&keyless_then, "")).map(|operator| operator.inputs),
            Some(vec![Some(11), None, Some(13)])
        );
        // The strict scalar-assignment proof stays closed for keyless branches.
        let text = wrap(KEYLESS_ELSE, "");
        let document = roxmltree::Document::parse(&text).unwrap();
        let component = document
            .descendants()
            .find(|node| node.attribute("name") == Some("if-else"))
            .unwrap();
        assert!(!read(&component).bounded_scalar_pins_valid);
    }

    #[test]
    fn malformed_pins_metadata_and_owners_are_refused() {
        for malformed in [
            // Keyless condition.
            KEYLESS_ELSE.replace(r#"<datapoint pos="0" key="11"/>"#, "<datapoint/>"),
            // Mismatched raw position.
            KEYLESS_ELSE.replace(r#"pos="1" key="12""#, r#"pos="2" key="12""#),
            // Keyless pin at a mismatched raw position.
            KEYLESS_ELSE.replace(
                "<datapoint/></sources>",
                r#"<datapoint pos="1"/></sources>"#,
            ),
            // Missing branch pin.
            KEYLESS_ELSE.replace("<datapoint/></sources>", "</sources>"),
            // Extra pin.
            KEYLESS_ELSE.replace(
                "<datapoint/></sources>",
                r#"<datapoint/><datapoint/></sources>"#,
            ),
            // Keyless or duplicate output.
            KEYLESS_ELSE.replace(r#"<datapoint pos="0" key="19"/>"#, "<datapoint/>"),
            KEYLESS_ELSE.replace(
                r#"<datapoint pos="0" key="19"/>"#,
                r#"<datapoint pos="0" key="19"/><datapoint pos="1" key="20"/>"#,
            ),
            // Output reused as an input.
            KEYLESS_ELSE.replace(r#"key="19""#, r#"key="12""#),
            // Unknown pin attribute, component metadata and priority context.
            KEYLESS_ELSE.replace(r#"pos="1" key="12""#, r#"pos="1" key="12" type="x""#),
            KEYLESS_ELSE.replace("<properties/>", "<data/>"),
            KEYLESS_ELSE.replace(r#"kind="4">"#, r#"kind="4" prioritycontext="5">"#),
            KEYLESS_ELSE.replace("<properties/>", r#"<properties a="1"/>"#),
        ] {
            assert_eq!(read_operator(&wrap(&malformed, "")), None, "{malformed}");
        }
        // A competing raw producer of the output or of an operand feed.
        let competing = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="19"/></targets><data><constant value="z" datatype="string"/></data></component>"#;
        assert_eq!(read_operator(&wrap(KEYLESS_ELSE, competing)), None);
        let operand = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="2"/></targets><data><constant value="z" datatype="string"/></data></component>"#;
        assert_eq!(read_operator(&wrap(KEYLESS_ELSE, operand)), None);
        // A keyed but unconnected pin has no saved feed.
        let unconnected = KEYLESS_ELSE.replace(
            "<datapoint/></sources>",
            r#"<datapoint pos="2" key="14"/></sources>"#,
        );
        assert_eq!(read_operator(&wrap(&unconnected, "")), None);
    }

    #[test]
    fn equal_and_empty_require_every_operand_keyed() {
        let equal = r#"<component name="equal" library="core" uid="5" kind="5"><sources><datapoint pos="0" key="12"/><datapoint pos="1" key="13"/></sources><targets><datapoint pos="0" key="19"/></targets></component>"#;
        assert_eq!(
            read_operator(&wrap(equal, "")).map(|operator| operator.feeds),
            Some(vec![Some(2), Some(3)])
        );
        assert_eq!(
            read_operator(&wrap(
                &equal.replace(r#"<datapoint pos="1" key="13"/>"#, "<datapoint/>"),
                ""
            )),
            None
        );
        let empty = r#"<component name="empty" library="lang" uid="6" kind="5"><sources><datapoint pos="0" key="12"/></sources><targets><datapoint pos="0" key="19"/></targets></component>"#;
        assert_eq!(
            read_operator(&wrap(empty, "")).map(|operator| operator.inputs),
            Some(vec![Some(12)])
        );
        assert_eq!(
            read_operator(&wrap(
                &empty.replace(r#"library="lang""#, r#"library="core""#),
                ""
            )),
            None
        );
    }
}
