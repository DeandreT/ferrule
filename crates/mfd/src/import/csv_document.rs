//! Opt-in filename retention. This is not an executable document profile.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use mapping::{DocumentProject, NodeId, PrimarySourcePlan};

use super::{
    ImportOptions, StageSelection,
    graph::GraphBuilder,
    schema::{ComponentFormat, SchemaComponent},
};
use crate::MfdError;

/// A repair-oriented document import. Warnings still describe omitted or
/// unsupported behavior; this result does not authorize document execution.
pub struct ImportedDocument {
    pub document: DocumentProject,
    pub warnings: Vec<String>,
    pub mapping_path: PathBuf,
}

/// Retain a connected primary CSV filename using the ordinary resource policy.
/// No input instance is opened, and no saved preview substitutes for the feed.
pub fn import_csv_document(path: &Path) -> Result<ImportedDocument, MfdError> {
    import_csv_document_with_options(path, &ImportOptions::default())
}

pub fn import_csv_document_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedDocument, MfdError> {
    let resources = super::resolved_resources(path, options)?;
    let result = super::import_resolved(
        &resources,
        StageSelection::Ordinary,
        options.item_ordered_exceptions,
        true,
    );
    resources.finish_bindings()?;
    let lowered = result?;
    let filename = lowered
        .primary_filename
        .ok_or_else(|| unsupported("missing retained filename"))?;
    Ok(ImportedDocument {
        document: DocumentProject::new(lowered.imported.project, PrimarySourcePlan { filename }),
        warnings: lowered.imported.warnings,
        mapping_path: lowered.imported.mapping_path,
    })
}

fn unsupported(reason: &str) -> MfdError {
    MfdError::UnsupportedImport(format!("CSV document filename retention: {reason}"))
}

pub(super) fn retain_filename(
    structure: roxmltree::Node<'_, '_>,
    primary: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
) -> Result<NodeId, MfdError> {
    if primary.format != ComponentFormat::Csv || primary.ports.is_empty() {
        return Err(unsupported(
            "requires a primary CSV component with data ports",
        ));
    }
    let children = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or_else(|| unsupported("missing component declarations"))?;
    // Bind the retained wrapper to the selected primary's data ports, not a
    // label or the first CSV component. Duplicate raw declarations stay refused.
    let candidates = children
        .children()
        .filter(|node| node.has_tag_name("component"))
        .filter(|component| {
            component.attribute("library") == Some("text")
                && component.attribute("name").unwrap_or_default() == primary.name
        })
        .filter(|component| {
            let keys = component
                .children()
                .filter(|node| node.has_tag_name("data"))
                .flat_map(|data| data.children().filter(|node| node.has_tag_name("root")))
                .flat_map(|root| root.descendants().filter(|node| node.has_tag_name("entry")))
                .filter_map(|entry| {
                    entry
                        .attribute("outkey")
                        .and_then(|key| key.parse::<u32>().ok())
                })
                .collect::<BTreeSet<_>>();
            primary.ports.keys().all(|key| keys.contains(key))
        })
        .collect::<Vec<_>>();
    let [component] = candidates.as_slice() else {
        return Err(unsupported(
            "primary component declaration is absent or ambiguous",
        ));
    };
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return Err(unsupported("requires one data declaration"));
    };
    let roots = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(unsupported("requires one root declaration"));
    };
    if root
        .children()
        .filter(|node| node.has_tag_name("header"))
        .count()
        > 1
    {
        return Err(unsupported("requires at most one namespace header"));
    }
    let entries = root
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|node| !protocol_namespace_header(*node))
        .collect::<Vec<_>>();
    let [wrapper] = entries.as_slice() else {
        return Err(unsupported("requires one FileInstance wrapper"));
    };
    if !wrapper.has_tag_name("entry")
        || wrapper.attribute("name") != Some("FileInstance")
        || wrapper.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(
                    attribute.name(),
                    "name" | "ns" | "inpkey" | "outkey" | "expanded"
                )
        })
    {
        return Err(unsupported("unsupported FileInstance wrapper metadata"));
    }
    let contents = wrapper
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if !matches!(contents.as_slice(), [document] if document.has_tag_name("entry")
        && document.attribute("name") == Some("document"))
    {
        return Err(unsupported("unsupported FileInstance wrapper content"));
    }
    if root.children().any(|node| node.has_tag_name("header"))
        && !super::schema::is_xml_document_wrapper(&contents[0], *root)
    {
        return Err(unsupported(
            "FileInstance namespace is not a protocol namespace",
        ));
    }
    let input = wrapper
        .attribute("inpkey")
        .and_then(|key| key.parse::<u32>().ok())
        .ok_or_else(|| unsupported("requires a connected FileInstance input"))?;
    let feed = builder
        .edge_from
        .get(&input)
        .copied()
        .ok_or_else(|| unsupported("requires a connected FileInstance input"))?;
    builder
        .binding_node(feed, &[])
        .ok_or_else(|| unsupported("filename feed cannot be retained"))
}

fn protocol_namespace_header(node: roxmltree::Node<'_, '_>) -> bool {
    if !node.has_tag_name("header") || node.attributes().len() != 0 {
        return false;
    }
    let children = node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [namespaces] = children.as_slice() else {
        return false;
    };
    if !namespaces.has_tag_name("namespaces") || namespaces.attributes().len() != 0 {
        return false;
    }
    let declarations = namespaces
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let supported = declarations.is_empty()
        || matches!(declarations.as_slice(), [empty, protocol]
        if empty.has_tag_name("namespace") && empty.attributes().len() == 0
            && protocol.has_tag_name("namespace") && protocol.attributes().len() == 1
            && protocol.attribute("uid") == Some("http://www.altova.com/mapforce")
            && [empty, protocol].into_iter().all(|entry| entry.children().all(|child| child.is_text()
                && child.text().is_none_or(|text| text.trim().is_empty()))));
    supported
        && namespaces
            .children()
            .filter(|child| child.is_text())
            .all(|child| child.text().is_none_or(|text| text.trim().is_empty()))
        && node
            .children()
            .filter(|child| child.is_text())
            .all(|child| child.text().is_none_or(|text| text.trim().is_empty()))
}
