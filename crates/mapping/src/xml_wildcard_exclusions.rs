//! File-capability gate for XSD 1.1 wildcard exclusion metadata.

use crate::{ExternalSourceOrigin, FormatOptions, Graph, Node, Project};

/// Required by files whose schemas use negated wildcard namespaces or
/// excluded names. Older readers ignore unknown schema members, so without
/// this capability they would drop the exclusions and widen the wildcard.
pub const XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY: &str = "xml-wildcard-exclusions-v1";

impl Project {
    /// Whether any schema stored in this project uses wildcard exclusions:
    /// the boundary schemas, XML serializer schemas in the main graph and
    /// every function body, and external request schemas.
    pub fn uses_xml_wildcard_exclusions(&self) -> bool {
        let options_use = |options: &FormatOptions| {
            options
                .external_source
                .as_ref()
                .is_some_and(|external| match external.origin() {
                    ExternalSourceOrigin::HttpPost {
                        request_schema: Some(schema),
                        ..
                    } => schema.uses_xml_wildcard_exclusions(),
                    ExternalSourceOrigin::HttpPost {
                        request_schema: None,
                        ..
                    }
                    | ExternalSourceOrigin::UserFunction { .. } => false,
                })
        };
        self.source.uses_xml_wildcard_exclusions()
            || self.target.uses_xml_wildcard_exclusions()
            || options_use(&self.source_options)
            || options_use(&self.target_options)
            || self.extra_sources.iter().any(|source| {
                source.schema.uses_xml_wildcard_exclusions() || options_use(&source.options)
            })
            || self.extra_targets.iter().any(|target| {
                target.schema.uses_xml_wildcard_exclusions() || options_use(&target.options)
            })
            || graph_uses(&self.graph)
            || self
                .user_functions
                .values()
                .any(|function| graph_uses(&function.body))
    }
}

fn graph_uses(graph: &Graph) -> bool {
    graph.nodes.values().any(|node| match node {
        Node::XmlSerialize { schema, .. } => schema.uses_xml_wildcard_exclusions(),
        Node::OwnedXmlSerialize(serialize) => serialize.schema.uses_xml_wildcard_exclusions(),
        _ => false,
    })
}

#[cfg(test)]
#[path = "xml_wildcard_exclusions_tests.rs"]
mod tests;
