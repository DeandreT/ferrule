use super::parse_u32;

pub(super) fn select_block<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    configured_block: Option<&str>,
    component_name: &str,
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
    if blocks.len() > 1 {
        warnings.push(format!(
            "csv target component `{component_name}` contains singleton rows alongside repeated block `{}`; singleton rows were skipped because ferrule CSV targets represent one repeated row shape",
            repeated.attribute("name").unwrap_or_default()
        ));
    }
    Some(repeated)
}
