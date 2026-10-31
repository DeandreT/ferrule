//! The schemas a project file stores, for file-capability detection.

use ir::SchemaNode;

use crate::{ExternalSourceOrigin, FormatOptions, Graph, Node, Project};

impl Project {
    /// Whether `predicate` holds for any schema the project file stores: the
    /// boundary schemas, XML serializer schemas in the main graph and every
    /// function body, and external request schemas.
    pub(crate) fn any_stored_schema(&self, predicate: impl Fn(&SchemaNode) -> bool) -> bool {
        let options_use = |options: &FormatOptions| {
            options
                .external_source
                .as_ref()
                .is_some_and(|external| match external.origin() {
                    ExternalSourceOrigin::HttpPost {
                        request_schema: Some(schema),
                        ..
                    } => predicate(schema),
                    ExternalSourceOrigin::HttpPost {
                        request_schema: None,
                        ..
                    }
                    | ExternalSourceOrigin::UserFunction { .. } => false,
                })
        };
        let graph_uses = |graph: &Graph| {
            graph.nodes.values().any(|node| match node {
                Node::XmlSerialize { schema, .. } => predicate(schema),
                Node::OwnedXmlSerialize(serialize) => predicate(&serialize.schema),
                _ => false,
            })
        };
        predicate(&self.source)
            || predicate(&self.target)
            || options_use(&self.source_options)
            || options_use(&self.target_options)
            || self
                .extra_sources
                .iter()
                .any(|source| predicate(&source.schema) || options_use(&source.options))
            || self
                .extra_targets
                .iter()
                .any(|target| predicate(&target.schema) || options_use(&target.options))
            || graph_uses(&self.graph)
            || self
                .user_functions
                .values()
                .any(|function| graph_uses(&function.body))
    }
}
