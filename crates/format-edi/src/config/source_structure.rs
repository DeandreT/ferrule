use super::{
    ConfigError,
    definitions::{Definitions, load_definitions},
    files::{Files, parse_document},
};
use mapping::X12SourceStructure;
use std::path::Path;

pub(super) fn read(
    path: &Path,
    selected: &[String],
    mut files: Files,
) -> Result<X12SourceStructure, ConfigError> {
    // Multiple alternatives require their own Select ownership contract.
    if selected.len() != 1 {
        return Err(ConfigError::Invalid(
            "native X12 source structure requires one message".into(),
        ));
    }
    let path = files.confined_file(path)?;
    let mut definitions = Definitions::default();
    load_definitions(&path, &mut files, &mut definitions)?;
    let main = files.read(&path)?;
    let doc = parse_document(&path, &main)?;
    let root = doc.root_element();
    if root
        .children()
        .find(|n| n.has_tag_name("Format"))
        .and_then(|n| n.attribute("standard"))
        != Some("X12")
    {
        return Err(ConfigError::Invalid("source structure requires X12".into()));
    }
    let message_path = files.resolve_message(&path, &selected[0])?;
    load_definitions(&message_path, &mut files, &mut definitions)?;
    let message_text = files.read(&message_path)?;
    let message_doc = parse_document(&message_path, &message_text)?;
    let message = message_doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("Message"))
        .and_then(|n| n.children().find(|n| n.has_tag_name("Group")))
        .ok_or_else(|| ConfigError::Invalid("missing source message group".into()))?;
    let envelope = root
        .children()
        .find(|n| n.has_tag_name("Group"))
        .ok_or_else(|| ConfigError::Invalid("missing source envelope group".into()))?;
    let mut count = 0;
    let mut result = walk(envelope, message, &definitions, 0, &mut count)?;
    result.known_segments = definitions
        .segments
        .values()
        .map(|segment| segment.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(result)
}

fn unsigned(node: roxmltree::Node<'_, '_>, name: &str, default: u32) -> Result<u32, ConfigError> {
    node.attribute(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| ConfigError::Invalid(format!("invalid source {name}")))
    })
}
fn walk(
    node: roxmltree::Node<'_, '_>,
    message: roxmltree::Node<'_, '_>,
    defs: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<X12SourceStructure, ConfigError> {
    *count += 1;
    if depth > 64 || *count > 10000 {
        return Err(ConfigError::Limit("source structure"));
    }
    let content = if node.has_tag_name("Select") {
        message
    } else if node.has_tag_name("Group") && node.attribute("ref").is_some() {
        let reference = node.attribute("ref").unwrap();
        node.document()
            .descendants()
            .find(|n| n.has_tag_name("Group") && n.attribute("id") == Some(reference))
            .ok_or_else(|| ConfigError::Invalid("unknown source group reference".into()))?
    } else {
        node
    };
    let segment = if node.has_tag_name("Segment") {
        let reference = node
            .attribute("ref")
            .ok_or_else(|| ConfigError::Invalid("source requires referenced segment".into()))?;
        Some(
            defs.segments
                .get(reference)
                .ok_or_else(|| ConfigError::Invalid("unknown source segment".into()))?
                .name
                .clone(),
        )
    } else {
        None
    };
    let name = node
        .attribute("nodeName")
        .or_else(|| content.attribute("name"))
        .map(str::to_string)
        .or_else(|| segment.clone())
        .ok_or_else(|| ConfigError::Invalid("unnamed source particle".into()))?;
    let minimum = unsigned(node, "minOccurs", 1)?;
    let maximum = match node.attribute("maxOccurs") {
        Some("unbounded") => None,
        _ => Some(unsigned(node, "maxOccurs", 1)?),
    };
    let merged_entries = unsigned(node, "mergedEntries", 1)?;
    let respect_maximum = match node.attribute("respectMaxOccurs") {
        None | Some("false") => false,
        Some("true") => true,
        _ => {
            return Err(ConfigError::Invalid(
                "invalid source respectMaxOccurs".into(),
            ));
        }
    };
    if minimum > 1024
        || merged_entries == 0
        || merged_entries > 1024
        || maximum.is_some_and(|m| m < minimum)
    {
        return Err(ConfigError::Invalid(
            "invalid source particle bounds".into(),
        ));
    }
    let mut children = Vec::new();
    if segment.is_none() {
        for child in content.children().filter(roxmltree::Node::is_element) {
            match child.tag_name().name() {
                "Group" | "Segment" | "Select" => {
                    children.push(walk(child, message, defs, depth + 1, count)?)
                }
                _ => {
                    return Err(ConfigError::Invalid(
                        "unsupported source structure child".into(),
                    ));
                }
            }
        }
    }
    Ok(X12SourceStructure {
        name,
        segment,
        minimum,
        maximum,
        respect_maximum,
        merged_entries,
        children,
        selection: node.has_tag_name("Select"),
        known_segments: vec![],
    })
}
