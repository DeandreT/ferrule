//! Census raw downstream roles before the schema reader normalizes port names.
use crate::import::schema::{
    normalize_xml_entry_name, parse_u32, xml_content_entry, xml_file_instance_input,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Refusal {
    RuntimeFilename,
    PassThrough,
    ForwardedOwner,
    AmbiguousInput,
    NonOrdinaryTarget,
    UnusedOutput,
}
impl Refusal {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::RuntimeFilename => {
                "runtime filename/FileInstance input collision requires separate proof"
            }
            Self::PassThrough => {
                "PassThrough XML owner is an intermediate variable; transitive forwarding is unsupported"
            }
            Self::ForwardedOwner => {
                "XML consumer owner declares output ports; forwarding is unsupported"
            }
            Self::AmbiguousInput => "consumer input has multiple raw port declarations",
            Self::NonOrdinaryTarget => {
                "only direct ordinary XML scalar target entries are supported; failure/predicate/filename/forwarded roles require separate proof"
            }
            Self::UnusedOutput => "unused call output has no proven eager consumption boundary",
        }
    }
}

pub(super) fn validate(
    call: &roxmltree::Node<'_, '_>,
    children: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
) -> Result<(), Refusal> {
    for output in call
        .descendants()
        .filter_map(|n| parse_u32(n.attribute("outkey")))
    {
        let mut consumed = false;
        for (input, _) in feeds.iter().filter(|(_, from)| **from == output) {
            consumed = true;
            let mut declarations = children
                .children()
                .filter(|n| n.has_tag_name("component"))
                .flat_map(|owner| {
                    owner
                        .descendants()
                        .filter(move |n| raw_input(*n) == Some(*input))
                        .map(move |n| (owner, n))
                });
            let Some((owner, entry)) = declarations.next() else {
                return Err(Refusal::NonOrdinaryTarget);
            };
            if declarations.next().is_some() {
                return Err(Refusal::AmbiguousInput);
            }
            validate_target(owner, entry, *input)?;
        }
        if !consumed {
            return Err(Refusal::UnusedOutput);
        }
    }
    Ok(())
}

fn raw_input(node: roxmltree::Node<'_, '_>) -> Option<u32> {
    parse_u32(node.attribute("inpkey")).or_else(|| {
        (node.has_tag_name("datapoint") && node.parent().is_some_and(|n| n.has_tag_name("sources")))
            .then(|| parse_u32(node.attribute("key")))
            .flatten()
    })
}

fn validate_target(
    owner: roxmltree::Node<'_, '_>,
    entry: roxmltree::Node<'_, '_>,
    input: u32,
) -> Result<(), Refusal> {
    if owner.attribute("library") != Some("xml") || owner.attribute("kind") != Some("14") {
        return Err(Refusal::NonOrdinaryTarget);
    }
    if owner
        .children()
        .any(|n| n.has_tag_name("properties") && n.attribute("PassThrough") == Some("1"))
    {
        return Err(Refusal::PassThrough);
    }
    let mut data_nodes = owner.children().filter(|n| n.has_tag_name("data"));
    let data = data_nodes.next().ok_or(Refusal::NonOrdinaryTarget)?;
    if data_nodes.next().is_some() {
        return Err(Refusal::NonOrdinaryTarget);
    }
    let root = data
        .children()
        .find(|n| n.has_tag_name("root"))
        .ok_or(Refusal::NonOrdinaryTarget)?;
    if xml_file_instance_input(root) == Some(input)
        || entry
            .attribute("name")
            .is_some_and(|name| normalize_xml_entry_name(name).0 == "FileInstance")
    {
        return Err(Refusal::RuntimeFilename);
    }
    if owner
        .descendants()
        .any(|n| n.has_tag_name("entry") && n.attribute("outkey").is_some())
    {
        return Err(Refusal::ForwardedOwner);
    }
    let selected = xml_content_entry(root).ok_or(Refusal::NonOrdinaryTarget)?;
    if !entry.has_tag_name("entry")
        || entry.children().any(|n| n.has_tag_name("entry"))
        || !entry.ancestors().skip(1).any(|n| n == selected)
        || entry.ancestors().take_while(|n| *n != selected).any(|n| {
            n.has_tag_name("entry")
                && matches!(
                    n.attribute("type"),
                    Some(
                        "xml-type"
                            | "comment-before"
                            | "comment-after"
                            | "processing-instruction-before"
                            | "processing-instruction-after"
                    )
                )
        })
        || !ordinary_content_entry(entry)
    {
        return Err(Refusal::NonOrdinaryTarget);
    }
    Ok(())
}

fn ordinary_content_entry(entry: roxmltree::Node<'_, '_>) -> bool {
    let Some(data) = entry.ancestors().find(|n| n.has_tag_name("data")) else {
        return false;
    };
    if data.children().filter(|n| n.has_tag_name("root")).count() != 1 {
        return false;
    }
    let Some(root) = data.children().find(|n| n.has_tag_name("root")) else {
        return false;
    };
    if root.attribute("rootindex").is_some_and(|s| s != "0") {
        return false;
    }
    let namespaces = root
        .children()
        .find(|n| n.has_tag_name("header"))
        .and_then(|n| n.children().find(|n| n.has_tag_name("namespaces")));
    if let Some(namespaces) = namespaces
        && namespaces
            .children()
            .find(|n| n.has_tag_name("namespace"))
            .is_none_or(|n| n.attribute("uid").is_some_and(|s| !s.is_empty()))
    {
        return false;
    }
    let mut documents = data.children().filter(|n| n.has_tag_name("document"));
    let Some(document) = documents.next() else {
        return false;
    };
    let Some(name) = document
        .attribute("instanceroot")
        .and_then(|s| s.strip_prefix("{}"))
    else {
        return false;
    };
    if documents.next().is_some()
        || name.is_empty()
        || document.attribute("schema").is_none_or(str::is_empty)
        || document
            .attribute("inputinstance")
            .is_some_and(|s| !s.is_empty())
    {
        return false;
    }
    let Some(content) = entry.ancestors().find(|n| {
        n.has_tag_name("entry")
            && n.attribute("name") == Some(name)
            && matches!(n.attribute("ns"), None | Some("0"))
    }) else {
        return false;
    };
    if content == entry {
        return false;
    }
    let Some(parent) = content.parent() else {
        return false;
    };
    if parent == root {
        return true;
    }
    let Some(file) = parent.parent() else {
        return false;
    };
    parent.has_tag_name("entry")
        && parent.attribute("name") == Some("document")
        && parent.attribute("ns") == Some("1")
        && file.has_tag_name("entry")
        && file.attribute("name") == Some("FileInstance")
        && file.attribute("ns") == Some("1")
        && file.parent() == Some(root)
        && namespaces
            .and_then(|n| n.children().filter(|n| n.has_tag_name("namespace")).nth(1))
            .and_then(|n| n.attribute("uid"))
            == Some("http://www.altova.com/mapforce")
}
