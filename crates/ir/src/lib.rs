//! Schema-agnostic in-memory IR shared by every format adapter: schema trees
//! (structure of a source/target format) and instance trees (actual data).
//!
//! Both are hierarchical: a node is either a scalar leaf or a named group of
//! children, and any node can be `repeating` (an XML element with
//! `maxOccurs > 1`, or -- external to this tree -- a CSV file's rows). This
//! is what lets the mapping engine implement the visual-mapper convention
//! that connecting two repeating groups implies a loop.

use serde::{Deserialize, Serialize};

/// Instance-field name used for an XML element's simple text content.
pub const XML_TEXT_FIELD: &str = "#text";

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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// This scalar node is the text content of its parent XML element rather
    /// than a nested element. XSD `simpleContent` uses one text child plus
    /// zero or more attribute children. Non-XML formats treat it as an
    /// ordinary named scalar field.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub text: bool,
    /// A required literal value for a scalar node (XSD's `xs:fixed`, JSON
    /// Schema's `const`), compared against the raw text before parsing.
    /// Format adapters use it both to validate and to disambiguate --
    /// notably EDI qualifier elements, where e.g. two loops both starting
    /// with an `HL` segment are told apart by `HL03` being `20` vs `22`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    pub kind: SchemaKind,
}

impl<'de> Deserialize<'de> for SchemaNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            name: String,
            #[serde(default)]
            repeating: bool,
            #[serde(default)]
            attribute: bool,
            #[serde(default)]
            text: bool,
            #[serde(default)]
            fixed: Option<String>,
            kind: SchemaKind,
        }

        let repr = Repr::deserialize(deserializer)?;
        let node = Self {
            name: repr.name,
            repeating: repr.repeating,
            attribute: repr.attribute,
            text: repr.text,
            fixed: repr.fixed,
            kind: repr.kind,
        };
        if !node.alternatives_are_valid() {
            return Err(serde::de::Error::custom(
                "group alternative metadata has duplicate or unknown names, members, or required fields",
            ));
        }
        Ok(node)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaKind {
    Scalar {
        ty: ScalarType,
    },
    Group {
        children: Vec<SchemaNode>,
        /// Explicit compatible object/type alternatives represented by the
        /// merged `children` projection. Empty for ordinary groups.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alternatives: Vec<GroupAlternative>,
        /// Schema shared by computed object fields whose names are supplied
        /// at mapping run time. Closed groups leave this unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<Box<SchemaNode>>,
    },
}

/// One structurally compatible alternative of a group projection.
///
/// Every member and required name must identify a child in the enclosing
/// group. Overlapping members share that one child schema, so importers must
/// reject alternatives that declare incompatible shapes for the same name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupAlternative {
    pub name: String,
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
}

impl SchemaNode {
    pub fn scalar(name: impl Into<String>, ty: ScalarType) -> Self {
        Self {
            name: name.into(),
            repeating: false,
            attribute: false,
            text: false,
            fixed: None,
            kind: SchemaKind::Scalar { ty },
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<SchemaNode>) -> Self {
        Self {
            name: name.into(),
            repeating: false,
            attribute: false,
            text: false,
            fixed: None,
            kind: SchemaKind::Group {
                children,
                alternatives: Vec::new(),
                dynamic: None,
            },
        }
    }

    /// Declares a homogeneous computed-field value schema for this group.
    /// Object alternatives and open fields are intentionally exclusive: an
    /// open object cannot be matched to one closed alternative exactly.
    pub fn with_dynamic_fields(mut self, value: SchemaNode) -> Option<Self> {
        self.set_dynamic_fields(Some(value)).then_some(self)
    }

    pub fn set_dynamic_fields(&mut self, value: Option<SchemaNode>) -> bool {
        let SchemaKind::Group {
            alternatives,
            dynamic,
            ..
        } = &mut self.kind
        else {
            return false;
        };
        if value.is_some() && !alternatives.is_empty() {
            return false;
        }
        *dynamic = value.map(Box::new);
        true
    }

    pub fn dynamic_fields(&self) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { dynamic, .. } => dynamic.as_deref(),
            SchemaKind::Scalar { .. } => None,
        }
    }

    /// Attaches validated alternative membership to a group node.
    pub fn with_alternatives(mut self, alternatives: Vec<GroupAlternative>) -> Option<Self> {
        self.set_alternatives(alternatives).then_some(self)
    }

    /// Replaces alternative membership when it is valid for this group.
    pub fn set_alternatives(&mut self, alternatives: Vec<GroupAlternative>) -> bool {
        let SchemaKind::Group {
            children,
            alternatives: target,
            dynamic,
        } = &mut self.kind
        else {
            return false;
        };
        if dynamic.is_some() || !valid_group_alternatives(children, &alternatives) {
            return false;
        }
        *target = alternatives;
        true
    }

    /// Checks metadata that may have entered through direct deserialization.
    pub fn alternatives_are_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group {
                children,
                alternatives,
                dynamic,
            } => {
                (alternatives.is_empty() || dynamic.is_none())
                    && (alternatives.is_empty() || valid_group_alternatives(children, alternatives))
            }
            SchemaKind::Scalar { .. } => true,
        }
    }

    pub fn alternatives(&self) -> &[GroupAlternative] {
        match &self.kind {
            SchemaKind::Group { alternatives, .. } => alternatives,
            SchemaKind::Scalar { .. } => &[],
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

    /// Marks this scalar as its parent XML element's text content.
    pub fn text(mut self) -> Self {
        self.text = true;
        self
    }

    /// Requires this scalar to hold `value` (builder-style).
    pub fn fixed(mut self, value: impl Into<String>) -> Self {
        self.fixed = Some(value.into());
        self
    }

    pub fn child(&self, name: &str) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children, .. } => children.iter().find(|c| c.name == name),
            SchemaKind::Scalar { .. } => None,
        }
    }

    pub fn text_child(&self) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children, .. } => children.iter().find(|child| child.text),
            SchemaKind::Scalar { .. } => None,
        }
    }
}

fn valid_group_alternatives(children: &[SchemaNode], alternatives: &[GroupAlternative]) -> bool {
    alternatives.len() >= 2
        && children.iter().enumerate().all(|(index, child)| {
            !children[..index]
                .iter()
                .any(|previous| previous.name == child.name)
        })
        && alternatives.iter().enumerate().all(|(index, alternative)| {
            !alternative.name.is_empty()
                && !alternatives[..index]
                    .iter()
                    .any(|previous| previous.name == alternative.name)
                && alternative
                    .members
                    .iter()
                    .enumerate()
                    .all(|(member_index, member)| {
                        !alternative.members[..member_index].contains(member)
                            && children.iter().any(|child| child.name == *member)
                    })
                && alternative
                    .required
                    .iter()
                    .enumerate()
                    .all(|(required_index, required)| {
                        !alternative.required[..required_index].contains(required)
                            && alternative.members.contains(required)
                    })
        })
}

/// An actual value tree, shaped by some [`SchemaNode`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instance {
    Scalar(Value),
    Group(Vec<(String, Instance)>),
    Repeated(Vec<Instance>),
    /// Mapping-produced XML element occurrences whose cardinality is
    /// independent of the schema node's declared repetition.
    MappedSequence(Vec<Instance>),
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

    pub fn as_mapped_sequence(&self) -> Option<&[Instance]> {
        match self {
            Instance::MappedSequence(items) => Some(items),
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
    fn mapped_sequence_roundtrips_without_becoming_schema_repetition() {
        let instance = Instance::MappedSequence(vec![
            Instance::Group(Vec::new()),
            Instance::Group(Vec::new()),
        ]);
        let encoded = serde_json::to_string(&instance).unwrap();
        let decoded: Instance = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, instance);
        assert_eq!(decoded.as_mapped_sequence().map(<[_]>::len), Some(2));
        assert!(decoded.as_repeated().is_none());
    }

    #[test]
    fn group_alternatives_are_explicit_validated_and_serde_defaulted() {
        let group = SchemaNode::group(
            "Address",
            vec![
                SchemaNode::scalar("state", ScalarType::String),
                SchemaNode::scalar("postcode", ScalarType::String),
            ],
        );
        assert!(group.clone().with_alternatives(Vec::new()).is_none());
        assert!(
            group
                .clone()
                .with_alternatives(vec![
                    GroupAlternative {
                        name: "domestic".into(),
                        members: vec!["missing".into()],
                        required: Vec::new(),
                    },
                    GroupAlternative {
                        name: "international".into(),
                        members: vec!["postcode".into()],
                        required: vec!["postcode".into()],
                    },
                ])
                .is_none()
        );

        let old_json = r#"{
          "name":"Address",
          "repeating":false,
          "kind":{"kind":"group","children":[]}
        }"#;
        let decoded: SchemaNode = serde_json::from_str(old_json).unwrap();
        assert!(decoded.alternatives().is_empty());
        assert!(
            !serde_json::to_string(&decoded)
                .unwrap()
                .contains("alternatives")
        );

        let invalid_json = r#"{
          "name":"Address",
          "kind":{"kind":"group","children":[],"alternatives":[{
            "name":"only","members":["missing"],"required":["missing"]
          }]}
        }"#;
        assert!(serde_json::from_str::<SchemaNode>(invalid_json).is_err());
    }

    #[test]
    fn dynamic_group_metadata_is_typed_exclusive_and_serde_defaulted() {
        let value = SchemaNode::scalar("value", ScalarType::String);
        let open = SchemaNode::group("Object", Vec::new())
            .with_dynamic_fields(value.clone())
            .unwrap();
        assert_eq!(open.dynamic_fields(), Some(&value));

        let encoded = serde_json::to_string(&open).unwrap();
        assert!(encoded.contains("\"dynamic\""));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded).unwrap(), open);

        let closed: SchemaNode =
            serde_json::from_str(r#"{"name":"Object","kind":{"kind":"group","children":[]}}"#)
                .unwrap();
        assert!(closed.dynamic_fields().is_none());

        let alternatives = vec![
            GroupAlternative {
                name: "one".into(),
                members: Vec::new(),
                required: Vec::new(),
            },
            GroupAlternative {
                name: "two".into(),
                members: Vec::new(),
                required: Vec::new(),
            },
        ];
        let alternative = SchemaNode::group("Object", Vec::new())
            .with_alternatives(alternatives)
            .unwrap();
        assert!(alternative.with_dynamic_fields(value).is_none());
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

    #[test]
    fn xml_text_marker_roundtrips_and_defaults_off() {
        let text = SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text();
        let json = serde_json::to_string(&text).unwrap();
        assert!(json.contains("\"text\":true"));
        assert_eq!(serde_json::from_str::<SchemaNode>(&json).unwrap(), text);

        let old_json = r#"{"name":"value","kind":{"kind":"scalar","ty":"string"}}"#;
        let old = serde_json::from_str::<SchemaNode>(old_json).unwrap();
        assert!(!old.text);
    }
}
