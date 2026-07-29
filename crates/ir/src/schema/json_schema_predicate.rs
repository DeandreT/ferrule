use serde::{Deserialize, Serialize};

use crate::SchemaNode;

/// One exactly represented JSON Schema predicate.
///
/// Array `contains` and object `dependentSchemas` share this representation
/// and the corresponding bounded native boundary matcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JsonSchemaPredicate {
    /// A predicate that matches no JSON value.
    Never,
    /// One ordinary, exactly represented JSON schema.
    Schema { schema: Box<SchemaNode> },
}

impl JsonSchemaPredicate {
    pub fn never() -> Self {
        Self::Never
    }

    pub fn schema(schema: SchemaNode) -> Self {
        Self::Schema {
            schema: Box::new(schema),
        }
    }

    pub fn as_schema(&self) -> Option<&SchemaNode> {
        match self {
            Self::Never => None,
            Self::Schema { schema } => Some(schema),
        }
    }

    pub fn is_never(&self) -> bool {
        matches!(self, Self::Never)
    }
}
