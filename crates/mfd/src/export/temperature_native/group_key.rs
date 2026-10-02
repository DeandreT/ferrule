use super::super::schema::KeyAlloc;
use crate::MfdError;

// Confined to the closed matched Year/partition-key shape above. The key reads
// the first current member's pinned month,so it equals the partition key. A
// global node alias would also change raw/control consumers and is unsafe.
pub(super) fn connect_owned(
    row: u32,
    year: u32,
    raw: u32,
    driver: u32,
    keys: &mut KeyAlloc,
    components: &mut String,
    edges: &mut [(u32, u32)],
) -> Result<(), MfdError> {
    let rejected =
        || MfdError::Unsupported("native temperature group-key ownership is inconsistent".into());
    let mut rows = edges.iter().filter(|(_, to)| *to == row);
    let groups = rows
        .next()
        .filter(|_| rows.next().is_none())
        .ok_or_else(rejected)?
        .0;
    let mut years = edges.iter().enumerate().filter(|(_, (_, to))| *to == year);
    let (year_index, &(year_feed, _)) = years
        .next()
        .filter(|_| years.next().is_none())
        .ok_or_else(rejected)?;
    if year_feed != raw {
        return Err(rejected());
    }
    let prefix = "<components>";
    let wrapped = format!("{prefix}{components}</components>");
    let document = roxmltree::Document::parse(&wrapped).map_err(|_| rejected())?;
    let port = |node: roxmltree::Node<'_, '_>| {
        node.attribute("key")
            .and_then(|key| key.parse::<u32>().ok())
    };
    let mut owners = document.root_element().children().filter(|node| {
        node.has_tag_name("component")
            && node.attribute("name") == Some("group-by")
            && node.attribute("library") == Some("core")
            && node.attribute("kind") == Some("5")
            && node
                .children()
                .find(|n| n.has_tag_name("targets"))
                .and_then(|n| n.children().find(|n| n.has_tag_name("datapoint")))
                .and_then(port)
                == Some(groups)
    });
    let owner = owners
        .next()
        .filter(|_| owners.next().is_none())
        .ok_or_else(rejected)?;
    let sources: Vec<_> = owner
        .children()
        .find(|n| n.has_tag_name("sources"))
        .ok_or_else(rejected)?
        .children()
        .filter(|n| n.has_tag_name("datapoint"))
        .collect();
    let targets: Vec<_> = owner
        .children()
        .find(|n| n.has_tag_name("targets"))
        .ok_or_else(rejected)?
        .children()
        .filter(|n| n.has_tag_name("datapoint"))
        .collect();
    if sources.len() != 2
        || targets.len() != 2
        || sources[0].attribute("pos") != Some("0")
        || sources[1].attribute("pos") != Some("1")
        || targets[0].attribute("pos") != Some("0")
        || port(targets[1]).is_some()
        || targets[1].attributes().len() != 0
        || targets[1].children().next().is_some()
    {
        return Err(rejected());
    }
    let nodes_input = port(sources[0]).ok_or_else(rejected)?;
    let mut drivers = edges.iter().filter(|(_, to)| *to == nodes_input);
    if drivers
        .next()
        .filter(|_| drivers.next().is_none())
        .map(|&(from, _)| from)
        != Some(driver)
    {
        return Err(rejected());
    }
    let key_input = port(sources[1]).ok_or_else(rejected)?;
    let mut feeds = edges.iter().filter(|(_, to)| *to == key_input);
    if feeds
        .next()
        .filter(|_| feeds.next().is_none())
        .map(|&(from, _)| from)
        != Some(raw)
    {
        return Err(rejected());
    }
    let range = targets[1].range();
    let output = keys.next();
    components.replace_range(
        (range.start - prefix.len())..(range.end - prefix.len()),
        &format!("<datapoint pos=\"1\" key=\"{output}\"/>"),
    );
    edges[year_index].0 = output;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn component() -> String {
        r#"<component name="group-by" library="core" kind="5" uid="114"><sources><datapoint pos="0" key="36"/><datapoint pos="1" key="37"/></sources><targets><datapoint pos="0" key="38"/><datapoint/></targets></component>"#.into()
    }
    fn edges() -> Vec<(u32, u32)> {
        vec![(2, 36), (29, 37), (38, 7), (29, 11), (29, 90)]
    }
    #[test]
    fn key_rewire_preserves_partition_input_group_driver_and_other_raw_consumers() {
        let mut xml = component();
        let mut edges = edges();
        let mut keys = KeyAlloc { next: 39 };
        connect_owned(7, 11, 29, 2, &mut keys, &mut xml, &mut edges).unwrap();
        assert_eq!(keys.next, 40);
        assert!(xml.contains(r#"<datapoint pos="1" key="39"/>"#));
        assert_eq!(edges, [(2, 36), (29, 37), (38, 7), (39, 11), (29, 90)]);
    }
    #[test]
    fn ambiguous_missing_occupied_or_different_owned_ports_reject_without_mutation() {
        for i in 0..9 {
            let mut xml = component();
            let mut edges = edges();
            match i {
                0 => edges.push((29, 11)),
                1 => edges.push((99, 7)),
                2 => edges[1] = (99, 37),
                3 => edges[3] = (99, 11),
                4 => xml = xml.replace("<datapoint/>", r#"<datapoint pos="1" key="99"/>"#),
                5 => xml.push_str(&component()),
                6 => xml = xml.replace(r#"name="group-by""#, r#"name="group-adjacent""#),
                7 => edges[0] = (99, 36),
                _ => edges.push((2, 36)),
            }
            let before = (xml.clone(), edges.clone());
            let mut keys = KeyAlloc { next: 39 };
            assert!(
                connect_owned(7, 11, 29, 2, &mut keys, &mut xml, &mut edges).is_err(),
                "{i}"
            );
            assert_eq!(keys.next, 39);
            assert_eq!((xml, edges), before, "{i}");
        }
    }
}
