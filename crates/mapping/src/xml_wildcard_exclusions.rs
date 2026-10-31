//! File-capability gate for XSD 1.1 wildcard exclusion metadata.

use ir::SchemaNode;

use crate::Project;

/// Required by files whose schemas use negated wildcard namespaces or
/// excluded names. Older readers ignore unknown schema members, so without
/// this capability they would drop the exclusions and widen the wildcard.
pub const XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY: &str = "xml-wildcard-exclusions-v1";

impl Project {
    /// Whether any schema stored in this project uses wildcard exclusions:
    /// the boundary schemas, XML serializer schemas in the main graph and
    /// every function body, and external request schemas.
    pub fn uses_xml_wildcard_exclusions(&self) -> bool {
        self.any_stored_schema(SchemaNode::uses_xml_wildcard_exclusions)
    }
}

#[cfg(test)]
#[path = "xml_wildcard_exclusions_tests.rs"]
mod tests;
