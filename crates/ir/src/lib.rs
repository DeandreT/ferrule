//! Schema-agnostic in-memory IR shared by every format adapter: schema trees
//! (structure of a source/target format) and instance trees (actual data).
//!
//! Both are hierarchical: a node is either a scalar leaf or a named group of
//! children, and any node can be `repeating` (an XML element with
//! `maxOccurs > 1`, or -- external to this tree -- a CSV file's rows). This
//! is what lets the mapping engine implement the visual-mapper convention
//! that connecting two repeating groups implies a loop.

use serde::{Deserialize, Serialize};

/// The scalar types a field can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    String,
    Int,
    Float,
    Bool,
}

/// A single scalar value flowing through a mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
        }
    }
}

/// The declared shape of one level of a source/target document: either a
/// scalar leaf or a named group of children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaNode {
    pub name: String,
    #[serde(default)]
    pub repeating: bool,
    /// This node is an XML attribute of its parent group (always a scalar).
    /// Non-XML formats ignore it; in [`Instance`] trees an attribute is an
    /// ordinary named field of the parent group -- which means an attribute
    /// and a child element sharing a name collide (known limitation).
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub attribute: bool,
    /// A required literal value for a scalar node (XSD's `xs:fixed`, JSON
    /// Schema's `const`), compared against the raw text before parsing.
    /// Format adapters use it both to validate and to disambiguate --
    /// notably EDI qualifier elements, where e.g. two loops both starting
    /// with an `HL` segment are told apart by `HL03` being `20` vs `22`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    pub kind: SchemaKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaKind {
    Scalar { ty: ScalarType },
    Group { children: Vec<SchemaNode> },
}

impl SchemaNode {
    pub fn scalar(name: impl Into<String>, ty: ScalarType) -> Self {
        Self {
            name: name.into(),
            repeating: false,
            attribute: false,
            fixed: None,
            kind: SchemaKind::Scalar { ty },
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<SchemaNode>) -> Self {
        Self {
            name: name.into(),
            repeating: false,
            attribute: false,
            fixed: None,
            kind: SchemaKind::Group { children },
        }
    }

    /// Marks this node as repeating (builder-style, for constructing schemas by hand).
    pub fn repeating(mut self) -> Self {
        self.repeating = true;
        self
    }

    /// Marks this node as an XML attribute of its parent (builder-style).
    pub fn attribute(mut self) -> Self {
        self.attribute = true;
        self
    }

    /// Requires this scalar to hold `value` (builder-style).
    pub fn fixed(mut self, value: impl Into<String>) -> Self {
        self.fixed = Some(value.into());
        self
    }

    pub fn child(&self, name: &str) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children } => children.iter().find(|c| c.name == name),
            SchemaKind::Scalar { .. } => None,
        }
    }
}

/// An actual value tree, shaped by some [`SchemaNode`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instance {
    Scalar(Value),
    Group(Vec<(String, Instance)>),
    Repeated(Vec<Instance>),
}

impl Instance {
    pub fn field(&self, name: &str) -> Option<&Instance> {
        match self {
            Instance::Group(fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_scalar(&self) -> Option<&Value> {
        match self {
            Instance::Scalar(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_repeated(&self) -> Option<&[Instance]> {
        match self {
            Instance::Repeated(items) => Some(items),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_json_roundtrip_picks_the_right_variant() {
        assert_eq!(serde_json::from_str::<Value>("42").unwrap(), Value::Int(42));
        assert_eq!(
            serde_json::from_str::<Value>("1.5").unwrap(),
            Value::Float(1.5)
        );
        assert_eq!(
            serde_json::from_str::<Value>("true").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            serde_json::from_str::<Value>("\"hi\"").unwrap(),
            Value::String("hi".to_string())
        );
        assert_eq!(serde_json::from_str::<Value>("null").unwrap(), Value::Null);
    }

    #[test]
    fn group_field_lookup_and_scalar_extraction() {
        let instance = Instance::Group(vec![
            (
                "name".to_string(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            (
                "tags".to_string(),
                Instance::Repeated(vec![
                    Instance::Scalar(Value::String("a".into())),
                    Instance::Scalar(Value::String("b".into())),
                ]),
            ),
        ]);

        assert_eq!(
            instance.field("name").and_then(Instance::as_scalar),
            Some(&Value::String("Jane".into()))
        );
        assert_eq!(
            instance
                .field("tags")
                .and_then(Instance::as_repeated)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(instance.field("missing"), None);
    }

    #[test]
    fn schema_node_child_lookup() {
        let schema = SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("id", ScalarType::Int),
                SchemaNode::group(
                    "items",
                    vec![SchemaNode::scalar("item", ScalarType::String).repeating()],
                ),
            ],
        );
        assert!(schema.child("id").is_some());
        assert!(
            schema
                .child("items")
                .unwrap()
                .child("item")
                .unwrap()
                .repeating
        );
        assert!(schema.child("missing").is_none());
    }
}
