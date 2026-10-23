//! Preserve X12 layout occurrence minimums without changing ordinary schemas.
use super::{
    ConfigError,
    definitions::{Definitions, load_definitions},
    files::{Files, parse_document},
};
use mapping::MapForceEdiMinimum;
use std::path::Path;

pub(super) fn read(
    path: &Path,
    selected: &[String],
    mut files: Files,
) -> Result<Vec<MapForceEdiMinimum>, ConfigError> {
    if selected.is_empty() || selected.len() > 32 {
        return Err(ConfigError::Invalid(
            "writer requires selected X12 messages".into(),
        ));
    }
    let path = files.confined_file(path)?;
    let mut definitions = Definitions::default();
    load_definitions(&path, &mut files, &mut definitions)?;
    let text = files.read(&path)?;
    let doc = parse_document(&path, &text)?;
    let root = doc.root_element();
    if root
        .children()
        .find(|n| n.has_tag_name("Format"))
        .and_then(|n| n.attribute("standard"))
        != Some("X12")
    {
        return Err(ConfigError::Invalid("writer minimums require X12".into()));
    }
    let mut messages = Vec::new();
    for code in selected {
        let path = files.resolve_message(&path, code)?;
        load_definitions(&path, &mut files, &mut definitions)?;
        let text = files.read(&path)?;
        let doc = parse_document(&path, &text)?;
        let group = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("Message"))
            .and_then(|n| n.children().find(|n| n.has_tag_name("Group")))
            .ok_or_else(|| ConfigError::Invalid("writer message has no group".into()))?;
        let name = group
            .attribute("name")
            .ok_or_else(|| ConfigError::Invalid("unnamed message".into()))?;
        let mut minima = Vec::new();
        walk(
            group,
            &definitions,
            &[name.to_string()],
            &[],
            &mut minima,
            0,
        )?;
        if let Some(first) = minima.first_mut() {
            first.minimum = 0;
        }
        messages.push(minima);
    }
    let group = root
        .children()
        .find(|n| n.has_tag_name("Group"))
        .ok_or_else(|| ConfigError::Invalid("writer requires an envelope group".into()))?;
    let mut result = Vec::new();
    walk(group, &definitions, &[], &messages, &mut result, 0)?;
    let mut seen = std::collections::BTreeSet::new();
    if result.iter().any(|m| !seen.insert(&m.path)) {
        return Err(ConfigError::Invalid(
            "ambiguous writer occurrence paths".into(),
        ));
    }
    Ok(result)
}

fn walk(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    path: &[String],
    messages: &[Vec<MapForceEdiMinimum>],
    out: &mut Vec<MapForceEdiMinimum>,
    depth: usize,
) -> Result<(), ConfigError> {
    if depth > 64 || out.len() >= 10000 {
        return Err(ConfigError::Limit("writer occurrence tree"));
    }
    let minimum = node
        .attribute("minOccurs")
        .unwrap_or("1")
        .parse::<u32>()
        .map_err(|_| ConfigError::Invalid("invalid occurrence minimum".into()))?;
    if minimum > 1024 {
        return Err(ConfigError::Limit("writer occurrence minimum"));
    }
    out.push(MapForceEdiMinimum {
        path: path.to_vec(),
        minimum,
    });
    let content = if let Some(reference) = node.attribute("ref") {
        node.document()
            .descendants()
            .find(|n| n.has_tag_name("Group") && n.attribute("id") == Some(reference))
            .ok_or_else(|| ConfigError::Invalid("unknown writer group reference".into()))?
    } else {
        node
    };
    for child in content.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "Group" => {
                let name = child
                    .attribute("name")
                    .or_else(|| {
                        child
                            .attribute("ref")
                            .and_then(|id| {
                                child.document().descendants().find(|n| {
                                    n.has_tag_name("Group") && n.attribute("id") == Some(id)
                                })
                            })
                            .and_then(|n| n.attribute("name"))
                    })
                    .ok_or_else(|| ConfigError::Invalid("unnamed writer group".into()))?;
                let mut next = path.to_vec();
                next.push(name.into());
                walk(child, definitions, &next, messages, out, depth + 1)?;
            }
            "Segment" => {
                let name = child
                    .attribute("nodeName")
                    .or_else(|| {
                        child
                            .attribute("ref")
                            .and_then(|id| definitions.segments.get(id))
                            .map(|s| s.name.as_str())
                    })
                    .or_else(|| child.attribute("name"))
                    .ok_or_else(|| ConfigError::Invalid("unknown writer segment".into()))?;
                let minimum = child
                    .attribute("minOccurs")
                    .unwrap_or("1")
                    .parse::<u32>()
                    .map_err(|_| ConfigError::Invalid("invalid segment minimum".into()))?;
                if minimum > 1024 {
                    return Err(ConfigError::Limit("writer occurrence minimum"));
                }
                if out.len() >= 10000 {
                    return Err(ConfigError::Limit("writer occurrence tree"));
                }
                let mut next = path.to_vec();
                next.push(name.into());
                out.push(MapForceEdiMinimum {
                    path: next,
                    minimum,
                });
            }
            "Select" => {
                for message in messages {
                    for entry in message {
                        if out.len() >= 10000 {
                            return Err(ConfigError::Limit("writer occurrence tree"));
                        }
                        let mut next = path.to_vec();
                        next.extend(entry.path.iter().cloned());
                        if next.len() > 64 {
                            return Err(ConfigError::Limit("writer occurrence depth"));
                        }
                        out.push(MapForceEdiMinimum {
                            path: next,
                            minimum: entry.minimum,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
