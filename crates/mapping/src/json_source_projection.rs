//! Explicit typed selection over ordered JSON, not schema validation.

use ir::{ScalarType, SchemaNode};
use serde::{Deserialize, Serialize};

pub const MAX_JSON_PROJECTION_DEPTH: usize = 24;
pub const MAX_JSON_PROJECTION_NODES: usize = 4096;
pub const MAX_JSON_PROJECTION_NAME_BYTES: usize = 65536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JsonSourceShape {
    Object { properties: Vec<JsonSourceProperty> },
    Array { item: Box<JsonSourceShape> },
    String,
    Number,
    Boolean,
    Null,
}

impl<'de> Deserialize<'de> for JsonSourceShape {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Empty struct variants, unlike serde's tagged unit variants, reject
        // extra fields rather than silently treating them as future options.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Object { properties: Vec<JsonSourceProperty> },
            Array { item: Box<JsonSourceShape> },
            String {},
            Number {},
            Boolean {},
            Null {},
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Object { properties } => Self::Object { properties },
            Wire::Array { item } => Self::Array { item },
            Wire::String {} => Self::String,
            Wire::Number {} => Self::Number,
            Wire::Boolean {} => Self::Boolean,
            Wire::Null {} => Self::Null,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonSourceProperty {
    pub name: String,
    pub shape: JsonSourceShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonProjectionPlanError {
    SourceView,
    Depth,
    Nodes,
    Names,
    DuplicateProperty,
    InvalidName,
}

impl std::fmt::Display for JsonProjectionPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid JSON source projection: {self:?}")
    }
}
impl std::error::Error for JsonProjectionPlanError {}

impl JsonSourceShape {
    pub fn validate(&self) -> Result<(), JsonProjectionPlanError> {
        fn visit(
            shape: &JsonSourceShape,
            depth: usize,
            nodes: &mut usize,
            names: &mut usize,
        ) -> Result<(), JsonProjectionPlanError> {
            if depth > MAX_JSON_PROJECTION_DEPTH {
                return Err(JsonProjectionPlanError::Depth);
            }
            *nodes += 1;
            if *nodes > MAX_JSON_PROJECTION_NODES {
                return Err(JsonProjectionPlanError::Nodes);
            }
            match shape {
                JsonSourceShape::Object { properties } => {
                    let mut seen = std::collections::BTreeSet::new();
                    for property in properties {
                        if property.name.is_empty() || property.name.contains('\0') {
                            return Err(JsonProjectionPlanError::InvalidName);
                        }
                        *names = names.saturating_add(property.name.len());
                        if *names > MAX_JSON_PROJECTION_NAME_BYTES {
                            return Err(JsonProjectionPlanError::Names);
                        }
                        if !seen.insert(&property.name) {
                            return Err(JsonProjectionPlanError::DuplicateProperty);
                        }
                        visit(&property.shape, depth + 1, nodes, names)?;
                    }
                }
                JsonSourceShape::Array { item } => visit(item, depth + 1, nodes, names)?,
                _ => {}
            }
            Ok(())
        }
        visit(self, 0, &mut 0, &mut 0)
    }

    /// One selected value. Every property and array item retains its own
    /// repeated occurrence collection; Number values remain String lexicals.
    pub fn value_schema(&self, name: &str) -> Result<SchemaNode, JsonProjectionPlanError> {
        self.validate()?;
        Ok(self.schema(name))
    }

    fn schema(&self, name: &str) -> SchemaNode {
        let children = match self {
            Self::Object { properties } => properties
                .iter()
                .map(|property| property.shape.schema(&property.name).repeating())
                .collect(),
            Self::Array { item } => vec![item.schema("Items").repeating()],
            Self::String | Self::Number => {
                vec![SchemaNode::scalar("Value", ScalarType::String)]
            }
            Self::Boolean => vec![SchemaNode::scalar("Value", ScalarType::Bool)],
            Self::Null => vec![],
        };
        SchemaNode::group(name, children)
    }

    /// File metadata is outside the selected root. A wrong-kind root produces
    /// an empty Root collection, not a missing physical document.
    pub fn document_schema(&self) -> Result<SchemaNode, JsonProjectionPlanError> {
        Ok(SchemaNode::group(
            "Document",
            vec![
                SchemaNode::scalar("Path", ScalarType::String),
                self.value_schema("Root")?.repeating(),
            ],
        ))
    }

    pub fn document_set_schema(&self) -> Result<SchemaNode, JsonProjectionPlanError> {
        let mut document = self.document_schema()?;
        document.name = "Documents".into();
        Ok(SchemaNode::group(
            "JsonDocuments",
            vec![document.repeating()],
        ))
    }
}
