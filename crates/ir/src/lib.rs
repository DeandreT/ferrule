//! Schema-agnostic in-memory IR shared by every format adapter: schema trees
//! (structure of a source/target format) and instance trees (actual data).
//!
//! Both are hierarchical: a node is either a scalar leaf or a named group of
//! children, and any node can be `repeating` (an XML element with
//! `maxOccurs > 1`, or -- external to this tree -- a CSV file's rows). This
//! is what lets the mapping engine implement the visual-mapper convention
//! that connecting two repeating groups implies a loop.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

/// Instance-field name used for an XML element's simple text content.
pub const XML_TEXT_FIELD: &str = "#text";

/// Reserved instance-group field carrying one validated expanded `xsi:type`
/// QName. XML readers and writers preserve it as format metadata; it is not
/// an ordinary schema child.
pub const XML_TYPE_FIELD: &str = "\u{1f}ferrule-xml-type";

/// Reserved instance-group field retaining the direct text and element nodes
/// of mixed XML content in document order. The field is format metadata and
/// is deliberately absent from [`SchemaNode`] trees.
pub const XML_MIXED_CONTENT_FIELD: &str = "\u{1f}ferrule-xml-mixed-content";

/// Reserved field holding the typed source value for one item in
/// [`XML_MIXED_CONTENT_FIELD`].
pub const XML_MIXED_CONTENT_VALUE_FIELD: &str = "\u{1f}ferrule-xml-mixed-value";

/// Virtual repeating group used to expose arbitrary direct XML child
/// elements while retaining their document order.
pub const XML_ELEMENTS_FIELD: &str = "element()";

/// Virtual repeating group used to expose arbitrary XML attributes on a
/// generic element. Each item contains `LocalName` and `#text` scalars.
pub const XML_ATTRIBUTES_FIELD: &str = "attribute()";

/// Synthetic fields available on items in [`XML_ELEMENTS_FIELD`].
pub const XML_LOCAL_NAME_FIELD: &str = "LocalName";
pub const XML_NODE_NAME_FIELD: &str = "NodeName";

/// The scalar types a field can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    String,
    Int,
    Float,
    Bool,
}

/// A value supplied by the owning format boundary instead of a graph binding.
///
/// This metadata is valid only on non-repeating scalar nodes. `MaxNumber`
/// models database target columns whose value is the next positive integer in
/// the replaced row set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueGeneration {
    MaxNumber,
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
    XmlNil(XmlNil),
}

/// Marker for an XML element that is present with `xsi:nil="true"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlNil;

impl Serialize for XmlNil {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("XmlNil", 1)?;
        state.serialize_field("$xml_nil", &true)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for XmlNil {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            #[serde(rename = "$xml_nil")]
            xml_nil: bool,
        }

        let repr = Repr::deserialize(deserializer)?;
        if !repr.xml_nil {
            return Err(serde::de::Error::custom("$xml_nil must be true"));
        }
        Ok(Self)
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::XmlNil(_) => "xml nil",
        }
    }

    pub fn xml_nil() -> Self {
        Self::XmlNil(XmlNil)
    }

    pub fn is_xml_nil(&self) -> bool {
        matches!(self, Self::XmlNil(_))
    }
}

/// The declared shape of one level of a source/target document: either a
/// scalar leaf or a named group of children.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaNode {
    pub name: String,
    #[serde(default)]
    pub repeating: bool,
    /// Reuses the shape of the nearest concrete group with this name.
    ///
    /// XSD recursive element/type declarations cannot be expanded into a
    /// finite tree. A recursive reference is therefore represented as an
    /// empty group whose occurrence metadata remains local while its child
    /// shape is resolved from this named anchor by recursive-aware formats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recursive_ref: Option<String>,
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
    /// This XML element may be present with `xsi:nil="true"`.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub nillable: bool,
    /// A required literal value for a scalar node (XSD's `xs:fixed`, JSON
    /// Schema's `const`), compared against the raw text before parsing.
    /// Format adapters use it both to validate and to disambiguate --
    /// notably EDI qualifier elements, where e.g. two loops both starting
    /// with an `HL` segment are told apart by `HL03` being `20` vs `22`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    /// The owning format generates this scalar when no mapped value is
    /// supplied. Generated values and fixed literals are mutually exclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_generation: Option<ValueGeneration>,
    /// How this group's alternatives compose. Exclusive alternatives model
    /// XML derived types and JSON Schema `oneOf`; inclusive alternatives
    /// model the bounded object-only JSON Schema `anyOf` subset.
    #[serde(default, skip_serializing_if = "GroupAlternativeMode::is_exclusive")]
    pub alternative_mode: GroupAlternativeMode,
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
            recursive_ref: Option<String>,
            #[serde(default)]
            attribute: bool,
            #[serde(default)]
            text: bool,
            #[serde(default)]
            nillable: bool,
            #[serde(default)]
            fixed: Option<String>,
            #[serde(default)]
            value_generation: Option<ValueGeneration>,
            #[serde(default)]
            alternative_mode: GroupAlternativeMode,
            kind: SchemaKind,
        }

        let repr = Repr::deserialize(deserializer)?;
        let node = Self {
            name: repr.name,
            repeating: repr.repeating,
            recursive_ref: repr.recursive_ref,
            attribute: repr.attribute,
            text: repr.text,
            nillable: repr.nillable,
            fixed: repr.fixed,
            value_generation: repr.value_generation,
            alternative_mode: repr.alternative_mode,
            kind: repr.kind,
        };
        if !node.alternatives_are_valid()
            || !node.recursive_ref_is_valid()
            || !node.value_generation_is_valid()
            || !node.alternative_mode_is_valid()
        {
            return Err(serde::de::Error::custom(
                "schema metadata contains invalid alternatives, recursion, value generation, or alternative mode",
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
    /// Required string values that distinguish this alternative from other
    /// structurally identical projections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<GroupAlternativeConstraint>,
}

/// One exact required string value used to select a group alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupAlternativeConstraint {
    pub member: String,
    pub value: String,
}

/// Whether exactly one or at least one declared group alternative must match.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupAlternativeMode {
    #[default]
    Exclusive,
    Inclusive,
}

impl GroupAlternativeMode {
    fn is_exclusive(&self) -> bool {
        matches!(self, Self::Exclusive)
    }
}

impl SchemaNode {
    pub fn scalar(name: impl Into<String>, ty: ScalarType) -> Self {
        Self {
            name: name.into(),
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            fixed: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            kind: SchemaKind::Scalar { ty },
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<SchemaNode>) -> Self {
        Self {
            name: name.into(),
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            fixed: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            kind: SchemaKind::Group {
                children,
                alternatives: Vec::new(),
                dynamic: None,
            },
        }
    }

    /// Creates a finite marker for an element whose group shape recursively
    /// references `anchor`.
    pub fn recursive_group(name: impl Into<String>, anchor: impl Into<String>) -> Self {
        let mut node = Self::group(name, Vec::new());
        node.recursive_ref = Some(anchor.into());
        node
    }

    pub fn recursive_ref_is_valid(&self) -> bool {
        let Some(anchor) = &self.recursive_ref else {
            return true;
        };
        !anchor.is_empty()
            && !self.attribute
            && !self.text
            && matches!(
                &self.kind,
                SchemaKind::Group {
                    children,
                    alternatives,
                    dynamic,
                } if children.is_empty() && alternatives.is_empty() && dynamic.is_none()
            )
            && self.alternative_mode.is_exclusive()
    }

    /// Checks that generated-value metadata remains scalar-only and cannot
    /// conflict with repetition or a fixed literal.
    pub fn value_generation_is_valid(&self) -> bool {
        self.value_generation.is_none()
            || (!self.repeating
                && self.fixed.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Marks a non-repeating scalar as format-generated.
    pub fn with_value_generation(mut self, generation: ValueGeneration) -> Option<Self> {
        self.value_generation = Some(generation);
        self.value_generation_is_valid().then_some(self)
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

    /// Attaches validated inclusive alternative membership to a group node.
    pub fn with_inclusive_alternatives(
        mut self,
        alternatives: Vec<GroupAlternative>,
    ) -> Option<Self> {
        self.set_group_alternatives(alternatives, GroupAlternativeMode::Inclusive)
            .then_some(self)
    }

    /// Replaces alternative membership when it is valid for this group.
    pub fn set_alternatives(&mut self, alternatives: Vec<GroupAlternative>) -> bool {
        self.set_group_alternatives(alternatives, GroupAlternativeMode::Exclusive)
    }

    fn set_group_alternatives(
        &mut self,
        alternatives: Vec<GroupAlternative>,
        mode: GroupAlternativeMode,
    ) -> bool {
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
        self.alternative_mode = mode;
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

    /// Checks that inclusive semantics cannot exist without group
    /// alternatives or leak onto scalar nodes.
    pub fn alternative_mode_is_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group { alternatives, .. } => {
                !alternatives.is_empty() || self.alternative_mode.is_exclusive()
            }
            SchemaKind::Scalar { .. } => self.alternative_mode.is_exclusive(),
        }
    }

    pub fn alternative_mode(&self) -> GroupAlternativeMode {
        self.alternative_mode
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

    pub fn nillable(mut self) -> Self {
        self.nillable = true;
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
                && alternative.constraints.iter().enumerate().all(
                    |(constraint_index, constraint)| {
                        !alternative.constraints[..constraint_index]
                            .iter()
                            .any(|previous| previous.member == constraint.member)
                            && alternative.required.contains(&constraint.member)
                            && children.iter().any(|child| {
                                child.name == constraint.member
                                    && !child.repeating
                                    && matches!(
                                        child.kind,
                                        SchemaKind::Scalar {
                                            ty: ScalarType::String
                                        }
                                    )
                            })
                    },
                )
        })
}

/// An actual value tree, shaped by some [`SchemaNode`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instance {
    Scalar(Value),
    Group(Vec<(String, Instance)>),
    Repeated(Vec<Instance>),
    /// Ordered documents. Each member retains a portable path and may also
    /// retain its resolved source location while its value remains an ordinary
    /// schema-shaped tree. Host-specific path validation belongs to the I/O
    /// boundary.
    DocumentSet(Vec<DocumentMember>),
    /// Mapping-produced XML element occurrences whose cardinality is
    /// independent of the schema node's declared repetition.
    MappedSequence(Vec<Instance>),
}

/// One structurally valid member of an [`Instance::DocumentSet`].
///
/// The portable path is non-empty but otherwise opaque here; filesystem
/// boundaries validate and confine it for their host before performing I/O.
/// A source member may additionally retain the non-empty resolved location
/// used by current-document-path expressions. Output boundaries continue to
/// consume only the portable path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentMember {
    path: String,
    #[serde(skip)]
    resolved_source_path: Option<String>,
    value: Box<Instance>,
}

impl DocumentMember {
    pub fn new(path: impl Into<String>, value: Instance) -> Option<Self> {
        Self::new_with_source_path(path, None, value)
    }

    pub fn new_source(
        path: impl Into<String>,
        source_path: impl Into<String>,
        value: Instance,
    ) -> Option<Self> {
        Self::new_with_source_path(path, Some(source_path.into()), value)
    }

    fn new_with_source_path(
        path: impl Into<String>,
        resolved_source_path: Option<String>,
        value: Instance,
    ) -> Option<Self> {
        let path = path.into();
        (!path.is_empty()
            && resolved_source_path
                .as_ref()
                .is_none_or(|path| !path.is_empty())
            && !matches!(value, Instance::DocumentSet(_)))
        .then(|| Self {
            path,
            resolved_source_path,
            value: Box::new(value),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn source_path(&self) -> &str {
        self.resolved_source_path.as_deref().unwrap_or(&self.path)
    }

    pub fn value(&self) -> &Instance {
        &self.value
    }
}

impl<'de> Deserialize<'de> for DocumentMember {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            path: String,
            value: Instance,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.path, wire.value).ok_or_else(|| {
            serde::de::Error::custom(
                "document-set members require non-empty paths and a non-document-set value",
            )
        })
    }
}

impl Instance {
    pub fn field(&self, name: &str) -> Option<&Instance> {
        match self {
            Instance::Group(fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            Instance::DocumentSet(documents) => documents.first()?.value().field(name),
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

    pub fn as_document_set(&self) -> Option<&[DocumentMember]> {
        match self {
            Instance::DocumentSet(documents) => Some(documents),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_members_validate_paths_and_keep_schema_traversal_transparent() {
        let value = Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("first".into())),
        )]);
        assert!(DocumentMember::new("", value.clone()).is_none());
        assert!(DocumentMember::new("nested.xml", Instance::DocumentSet(Vec::new())).is_none());
        assert!(DocumentMember::new_source("first.xml", "", value.clone()).is_none());
        let Some(member) = DocumentMember::new("first.xml", value) else {
            panic!("valid document member")
        };
        assert_eq!(member.source_path(), "first.xml");
        let documents = Instance::DocumentSet(vec![member]);

        assert_eq!(
            documents.field("Value").and_then(Instance::as_scalar),
            Some(&Value::String("first".into()))
        );
        assert!(
            serde_json::from_str::<DocumentMember>(r#"{"path":"","value":{"Group":[]}}"#).is_err()
        );

        let Some(source) = DocumentMember::new_source(
            "first.xml",
            "/inputs/first.xml",
            Instance::Group(Vec::new()),
        ) else {
            panic!("valid source document member")
        };
        assert_eq!(source.path(), "first.xml");
        assert_eq!(source.source_path(), "/inputs/first.xml");
        let encoded = serde_json::to_string(&source).unwrap();
        assert!(!encoded.contains("/inputs/first.xml"));
        let decoded = serde_json::from_str::<DocumentMember>(&encoded).unwrap();
        assert_eq!(decoded.path(), "first.xml");
        assert_eq!(decoded.source_path(), "first.xml");
    }

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
        let nil = serde_json::to_string(&Value::xml_nil()).unwrap();
        assert_eq!(nil, r#"{"$xml_nil":true}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&nil).unwrap(),
            Value::xml_nil()
        );
        assert!(serde_json::from_str::<Value>(r#"{"$xml_nil":false}"#).is_err());
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
                        constraints: Vec::new(),
                    },
                    GroupAlternative {
                        name: "international".into(),
                        members: vec!["postcode".into()],
                        required: vec!["postcode".into()],
                        constraints: Vec::new(),
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

        let inclusive = group
            .with_inclusive_alternatives(vec![
                GroupAlternative {
                    name: "domestic".into(),
                    members: vec!["state".into()],
                    required: Vec::new(),
                    constraints: Vec::new(),
                },
                GroupAlternative {
                    name: "international".into(),
                    members: vec!["postcode".into()],
                    required: Vec::new(),
                    constraints: Vec::new(),
                },
            ])
            .unwrap();
        assert_eq!(
            inclusive.alternative_mode(),
            GroupAlternativeMode::Inclusive
        );
        let encoded = serde_json::to_string(&inclusive).unwrap();
        assert!(encoded.contains(r#""alternative_mode":"inclusive""#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            inclusive
        );

        let discriminated = SchemaNode::group(
            "Event",
            vec![
                SchemaNode::scalar("kind", ScalarType::String),
                SchemaNode::scalar("value", ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            GroupAlternative {
                name: "created".into(),
                members: vec!["kind".into(), "value".into()],
                required: vec!["kind".into(), "value".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: "created".into(),
                }],
            },
            GroupAlternative {
                name: "deleted".into(),
                members: vec!["kind".into(), "value".into()],
                required: vec!["kind".into(), "value".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: "deleted".into(),
                }],
            },
        ])
        .unwrap();
        let encoded = serde_json::to_string(&discriminated).unwrap();
        assert!(encoded.contains(r#""constraints""#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            discriminated
        );

        let mut invalid = discriminated.alternatives().to_vec();
        invalid[0].required.retain(|field| field != "kind");
        assert!(
            SchemaNode::group(
                "Event",
                vec![
                    SchemaNode::scalar("kind", ScalarType::String),
                    SchemaNode::scalar("value", ScalarType::String),
                ],
            )
            .with_alternatives(invalid)
            .is_none()
        );

        let mut duplicate = discriminated.alternatives().to_vec();
        let duplicate_constraint = duplicate[0].constraints[0].clone();
        duplicate[0].constraints.push(duplicate_constraint);
        assert!(
            SchemaNode::group(
                "Event",
                vec![
                    SchemaNode::scalar("kind", ScalarType::String),
                    SchemaNode::scalar("value", ScalarType::String),
                ],
            )
            .with_alternatives(duplicate)
            .is_none()
        );
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
                constraints: Vec::new(),
            },
            GroupAlternative {
                name: "two".into(),
                members: Vec::new(),
                required: Vec::new(),
                constraints: Vec::new(),
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
    fn value_generation_is_scalar_only_and_roundtrips() {
        let generated = SchemaNode::scalar("Id", ScalarType::Int)
            .with_value_generation(ValueGeneration::MaxNumber)
            .unwrap();
        let encoded = serde_json::to_string(&generated).unwrap();
        assert!(encoded.contains(r#""value_generation":"max_number""#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            generated
        );

        assert!(
            SchemaNode::group("Rows", Vec::new())
                .with_value_generation(ValueGeneration::MaxNumber)
                .is_none()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"Rows","value_generation":"max_number","kind":{"kind":"group","children":[]}}"#
            )
            .is_err()
        );
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
