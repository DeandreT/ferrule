use super::parse_u32;

const CSV_SINGLETON_BEFORE: &str = "\u{1f}ferrule-csv-singleton-before";
const CSV_SINGLETON_AFTER: &str = "\u{1f}ferrule-csv-singleton-after";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SingletonPosition {
    Before(usize),
    After(usize),
}

pub(super) struct SingletonRow<'a, 'input> {
    pub(super) position: SingletonPosition,
    pub(super) entry: roxmltree::Node<'a, 'input>,
}

pub(super) fn singleton_port_path(position: SingletonPosition, field: &str) -> Vec<String> {
    let (marker, index) = match position {
        SingletonPosition::Before(index) => (CSV_SINGLETON_BEFORE, index),
        SingletonPosition::After(index) => (CSV_SINGLETON_AFTER, index),
    };
    vec![marker.to_string(), index.to_string(), field.to_string()]
}

pub(crate) fn split_singleton_port(path: &[String]) -> Option<(SingletonPosition, &str)> {
    let [marker, index, field] = path else {
        return None;
    };
    let index = index.parse().ok()?;
    let position = match marker.as_str() {
        CSV_SINGLETON_BEFORE => SingletonPosition::Before(index),
        CSV_SINGLETON_AFTER => SingletonPosition::After(index),
        _ => return None,
    };
    Some((position, field))
}

pub(super) fn select_block<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    configured_block: Option<&str>,
    component_name: &str,
    format_name: &str,
    warnings: &mut Vec<String>,
) -> Option<roxmltree::Node<'a, 'input>> {
    let document = root
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"));
    let blocks = document
        .map(|document| {
            document
                .children()
                .filter(|node| {
                    node.has_tag_name("entry")
                        && configured_block.is_none_or(|name| node.attribute("name") == Some(name))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let fallback = || {
        let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
        while matches!(
            entry.attribute("name"),
            Some("FileInstance") | Some("document")
        ) {
            entry = entry.children().find(|node| node.has_tag_name("entry"))?;
        }
        Some(entry)
    };
    let first = blocks.first().copied().or_else(fallback)?;
    let Some(repeated) = blocks.iter().copied().find(|candidate| {
        candidate.attribute("clone") == Some("1")
            && parse_u32(candidate.attribute("inpkey")).is_some()
    }) else {
        return Some(first);
    };
    if blocks.len() > 1 && format_name != "csv" {
        let target_label = if format_name == "csv" {
            "CSV"
        } else {
            format_name
        };
        warnings.push(format!(
            "{format_name} target component `{component_name}` contains singleton rows alongside repeated block `{}`; singleton rows were skipped because ferrule {target_label} targets represent one repeated row shape",
            repeated.attribute("name").unwrap_or_default()
        ));
    }
    Some(repeated)
}

pub(super) fn singleton_rows<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    configured_block: Option<&str>,
    selected: roxmltree::Node<'a, 'input>,
) -> Vec<SingletonRow<'a, 'input>> {
    let Some(document) = root
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"))
    else {
        return Vec::new();
    };
    let blocks = document
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && configured_block.is_none_or(|name| node.attribute("name") == Some(name))
        })
        .collect::<Vec<_>>();
    let Some(selected_index) = blocks.iter().position(|block| block.id() == selected.id()) else {
        return Vec::new();
    };
    if selected.attribute("clone") != Some("1") || parse_u32(selected.attribute("inpkey")).is_none()
    {
        return Vec::new();
    }
    blocks
        .into_iter()
        .enumerate()
        .filter(|(_, block)| block.id() != selected.id())
        .map(|(index, entry)| SingletonRow {
            position: if index < selected_index {
                SingletonPosition::Before(index)
            } else {
                SingletonPosition::After(index)
            },
            entry,
        })
        .collect()
}
