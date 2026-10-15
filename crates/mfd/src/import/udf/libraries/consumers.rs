//! Raw consumer roles checked before schema port normalization. No transitive
//! forwarding is admitted: an intermediate is a refusal, not a terminal target
//! merely because a walk through its outputs could eventually reach one.
use std::collections::BTreeMap;

use crate::import::schema::{
    normalize_xml_entry_name, parse_u32, xml_content_entry, xml_file_instance_input,
};

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
                "runtime filename/FileInstance input collision (including namespace/position aliases) requires separate proof"
            }
            Self::PassThrough => {
                "PassThrough XML owner is an intermediate variable, not an ordinary final target; transitive consumer forwarding is unsupported"
            }
            Self::ForwardedOwner => {
                "XML consumer owner declares output ports; source/variable/transitive consumer forwarding is unsupported"
            }
            Self::AmbiguousInput => {
                "consumer input has multiple raw port declarations; normalized role/owner aliases are unsupported"
            }
            Self::NonOrdinaryTarget => {
                "only direct ordinary XML scalar target entries are supported; failure/predicate/filename/forwarded calls require separate proof"
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
            // Count ALL raw declarations, not just candidates that already look
            // ordinary: a protocol/other owner sharing the numeric key must not
            // vanish from this census. No allocation or recursive graph walk.
            let mut declarations = children
                .children()
                .filter(|n| n.has_tag_name("component"))
                .flat_map(|owner| {
                    owner
                        .descendants()
                        .filter(move |node| raw_input(*node) == Some(*input))
                        .map(move |node| (owner, node))
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
        (node.has_tag_name("datapoint") && node.parent().is_some_and(|p| p.has_tag_name("sources")))
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
    // Same ANY direct-properties rule as schema::read_schema_component_resolved,
    // not a first-properties lookup or a check on the leaf entry alone.
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
    // Match the downstream *selected numeric key*, even if the selected entry
    // is elsewhere in the tree or has a different namespace/position/type.
    // Also conservatively refuse legacy decimal-slot/@ normalized names: those
    // are not proof of ordinary content rather than a reserved-role alias.
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
        || entry.attribute("outkey").is_some()
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
        || !super::ordinary_content_entry(entry)
    {
        return Err(Refusal::NonOrdinaryTarget);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
