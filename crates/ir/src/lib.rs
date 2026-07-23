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

/// Reserved instance-group field carrying the selected expanded element QName
/// for one XSD substitution-group occurrence.
pub const XML_SUBSTITUTION_FIELD: &str = "\u{1f}ferrule-xml-substitution";

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

/// Validated namespace URI used by an XML expanded name.
///
/// The inner string is private so a qualified namespace can never carry the
/// empty URI; an absent namespace is represented by
/// [`XmlNamespace::Unqualified`] instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct XmlNamespaceUri(String);

impl XmlNamespaceUri {
    pub fn new(uri: impl Into<String>) -> Option<Self> {
        let uri = uri.into();
        (!uri.is_empty()).then_some(Self(uri))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for XmlNamespaceUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let uri = String::deserialize(deserializer)?;
        Self::new(uri).ok_or_else(|| serde::de::Error::custom("XML namespace URI cannot be empty"))
    }
}

/// Exact namespace identity for one XML element or attribute name.
///
/// `SchemaNode::xml_namespace == None` remains the legacy, format-agnostic
/// behavior. Explicit metadata distinguishes a truly unqualified name from a
/// name in a non-empty namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "uri", rename_all = "snake_case")]
pub enum XmlNamespace {
    Unqualified,
    Qualified(XmlNamespaceUri),
}

impl XmlNamespace {
    pub fn qualified(uri: impl Into<String>) -> Option<Self> {
        XmlNamespaceUri::new(uri).map(Self::Qualified)
    }

    pub fn uri(&self) -> Option<&str> {
        match self {
            Self::Unqualified => None,
            Self::Qualified(uri) => Some(uri.as_str()),
        }
    }

    pub fn matches(&self, namespace: Option<&str>) -> bool {
        match self {
            Self::Unqualified => namespace.is_none_or(str::is_empty),
            Self::Qualified(uri) => namespace == Some(uri.as_str()),
        }
    }
}

/// Which table owns the foreign-key column for a declared database relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseForeignKeySide {
    Parent,
    Child,
}

/// Exact columns for a nested relational database group when the mapping
/// design declares a relation that is not present in the physical database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseRelation {
    pub parent_column: String,
    pub child_column: String,
    pub foreign_key_side: DatabaseForeignKeySide,
}

/// A single scalar value flowing through a mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    JsonNull(JsonNull),
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    XmlNil(XmlNil),
}

/// Marker for an XML element that is present with `xsi:nil="true"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlNil;

/// Marker for an explicit JSON `null`.
///
/// [`Value::Null`] remains boundary-level absence. Keeping the two values
/// distinct lets optional nullable object properties round-trip without
/// turning an omitted property into an explicit null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonNull;

impl Serialize for JsonNull {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("JsonNull", 1)?;
        state.serialize_field("$json_null", &true)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for JsonNull {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            #[serde(rename = "$json_null")]
            json_null: bool,
        }

        let repr = Repr::deserialize(deserializer)?;
        if !repr.json_null {
            return Err(serde::de::Error::custom("$json_null must be true"));
        }
        Ok(Self)
    }
}

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
            Value::JsonNull(_) => "json null",
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

    pub fn json_null() -> Self {
        Self::JsonNull(JsonNull)
    }

    pub fn is_json_null(&self) -> bool {
        matches!(self, Self::JsonNull(_))
    }

    pub fn is_xml_nil(&self) -> bool {
        matches!(self, Self::XmlNil(_))
    }

    pub fn is_null_like(&self) -> bool {
        matches!(self, Self::Null | Self::JsonNull(_) | Self::XmlNil(_))
    }
}

/// The declared shape of one level of a source/target document: either a
/// scalar leaf or a named group of children.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaNode {
    pub name: String,
    /// Exact XML namespace identity for this local name. `None` preserves the
    /// legacy behavior: readers match by local name and writers inherit the
    /// current default namespace. Non-XML formats ignore this metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml_namespace: Option<XmlNamespace>,
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
    /// This JSON scalar may be the explicit `null` value.
    ///
    /// Missing object properties remain boundary-level absence and do not
    /// require this flag. Repeating scalar nodes apply it to each array item,
    /// not to the array itself.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub nullable: bool,
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
    /// How XML boundaries encode this group's exclusive alternatives.
    #[serde(default, skip_serializing_if = "XmlAlternativeKind::is_xsi_type")]
    pub xml_alternative_kind: XmlAlternativeKind,
    /// Repeating anonymous XML sequences flattened into this group's named
    /// children for mapping-port compatibility. XML adapters use this metadata
    /// to retain document order and recreate the original compositor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xml_repeating_sequences: Vec<XmlRepeatingSequence>,
    /// Explicit join endpoints for a nested repeating database relation.
    /// When absent, database adapters resolve the relation from physical FK
    /// metadata. Non-database adapters ignore this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_relation: Option<DatabaseRelation>,
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
            xml_namespace: Option<XmlNamespace>,
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
            nullable: bool,
            #[serde(default)]
            fixed: Option<String>,
            #[serde(default)]
            value_generation: Option<ValueGeneration>,
            #[serde(default)]
            alternative_mode: GroupAlternativeMode,
            #[serde(default)]
            xml_alternative_kind: XmlAlternativeKind,
            #[serde(default)]
            xml_repeating_sequences: Vec<XmlRepeatingSequence>,
            #[serde(default)]
            database_relation: Option<DatabaseRelation>,
            kind: SchemaKind,
        }

        let repr = Repr::deserialize(deserializer)?;
        let node = Self {
            name: repr.name,
            xml_namespace: repr.xml_namespace,
            repeating: repr.repeating,
            recursive_ref: repr.recursive_ref,
            attribute: repr.attribute,
            text: repr.text,
            nillable: repr.nillable,
            nullable: repr.nullable,
            fixed: repr.fixed,
            value_generation: repr.value_generation,
            alternative_mode: repr.alternative_mode,
            xml_alternative_kind: repr.xml_alternative_kind,
            xml_repeating_sequences: repr.xml_repeating_sequences,
            database_relation: repr.database_relation,
            kind: repr.kind,
        };
        if !node.alternatives_are_valid()
            || !node.recursive_ref_is_valid()
            || !node.value_generation_is_valid()
            || !node.alternative_mode_is_valid()
            || !node.xml_alternative_kind_is_valid()
            || !node.xml_repeating_sequences_are_valid()
            || !node.database_relation_is_valid()
            || !node.nullable_is_valid()
        {
            return Err(serde::de::Error::custom(
                "schema metadata contains invalid alternatives, recursion, value generation, alternative mode, XML alternative kind, XML repeating sequences, database relation, or JSON nullability",
            ));
        }
        Ok(node)
    }
}

/// One anonymous `xs:sequence` whose repetitions are projected onto named
/// child ports. Member occurrence flags describe one sequence iteration;
/// every projected child remains repeating in the ordinary schema view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlRepeatingSequence {
    #[serde(default)]
    pub required: bool,
    pub members: Vec<XmlSequenceMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlSequenceMember {
    pub name: String,
    pub required: bool,
    pub repeating: bool,
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
    /// Required scalar values that distinguish this alternative from other
    /// structurally identical projections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<GroupAlternativeConstraint>,
}

/// One exact required scalar value used to select a group alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupAlternativeConstraint {
    pub member: String,
    pub value: GroupAlternativeConstraintValue,
}

/// A JSON-compatible scalar discriminator whose value can survive the IR
/// without losing its declared type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GroupAlternativeConstraintValue {
    String(String),
    Int(i64),
    Float(FiniteF64),
    Bool(bool),
}

impl GroupAlternativeConstraintValue {
    fn is_valid_for(&self, ty: ScalarType) -> bool {
        matches!(
            (self, ty),
            (Self::String(_), ScalarType::String)
                | (Self::Int(_), ScalarType::Int)
                | (Self::Bool(_), ScalarType::Bool)
        ) || matches!((self, ty), (Self::Float(_), ScalarType::Float))
    }
}

/// One finite 64-bit float. Construction and deserialization reject infinities
/// and NaN so scalar discriminator values are always JSON-serializable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteF64(f64);

impl Eq for FiniteF64 {}

impl FiniteF64 {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl Serialize for FiniteF64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for FiniteF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| serde::de::Error::custom("float must be finite"))
    }
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

/// XML wire representation for one exclusive group-alternative set.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XmlAlternativeKind {
    #[default]
    XsiType,
    SubstitutionGroup,
}

impl XmlAlternativeKind {
    fn is_xsi_type(&self) -> bool {
        matches!(self, Self::XsiType)
    }
}

impl SchemaNode {
    pub fn scalar(name: impl Into<String>, ty: ScalarType) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            nullable: false,
            fixed: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_repeating_sequences: Vec::new(),
            database_relation: None,
            kind: SchemaKind::Scalar { ty },
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<SchemaNode>) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            nullable: false,
            fixed: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_repeating_sequences: Vec::new(),
            database_relation: None,
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
            && self.xml_alternative_kind.is_xsi_type()
    }

    /// Marks this XML name as explicitly unqualified.
    pub fn xml_unqualified(mut self) -> Self {
        self.xml_namespace = Some(XmlNamespace::Unqualified);
        self
    }

    /// Marks this XML name as belonging to a non-empty namespace URI.
    pub fn xml_qualified(mut self, uri: impl Into<String>) -> Option<Self> {
        self.xml_namespace = Some(XmlNamespace::qualified(uri)?);
        Some(self)
    }

    /// Checks that generated-value metadata remains scalar-only and cannot
    /// conflict with repetition or a fixed literal.
    pub fn value_generation_is_valid(&self) -> bool {
        self.value_generation.is_none()
            || (!self.repeating
                && self.fixed.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Checks that explicit JSON nullability remains scalar-only.
    pub fn nullable_is_valid(&self) -> bool {
        !self.nullable || matches!(self.kind, SchemaKind::Scalar { .. })
    }

    /// Checks that declared database relation metadata belongs to this nested table.
    pub fn database_relation_is_valid(&self) -> bool {
        let Some(relation) = &self.database_relation else {
            return true;
        };
        let Some((table, join_column)) = self.name.split_once('|') else {
            return false;
        };
        if table.is_empty()
            || join_column.is_empty()
            || join_column.contains('|')
            || relation.parent_column.is_empty()
            || relation.child_column.is_empty()
        {
            return false;
        }
        let join_matches_owner = match relation.foreign_key_side {
            DatabaseForeignKeySide::Parent => {
                join_column.eq_ignore_ascii_case(&relation.parent_column)
            }
            DatabaseForeignKeySide::Child => {
                join_column.eq_ignore_ascii_case(&relation.child_column)
            }
        };
        join_matches_owner
            && self.repeating
            && self.recursive_ref.is_none()
            && matches!(self.kind, SchemaKind::Group { .. })
    }

    /// Attaches exact relation endpoints to a nested repeating database group.
    pub fn with_database_relation(mut self, relation: DatabaseRelation) -> Option<Self> {
        self.database_relation = Some(relation);
        self.database_relation_is_valid().then_some(self)
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
        self.set_group_alternatives(
            alternatives,
            GroupAlternativeMode::Inclusive,
            XmlAlternativeKind::XsiType,
        )
        .then_some(self)
    }

    /// Replaces alternative membership when it is valid for this group.
    pub fn set_alternatives(&mut self, alternatives: Vec<GroupAlternative>) -> bool {
        self.set_group_alternatives(
            alternatives,
            GroupAlternativeMode::Exclusive,
            XmlAlternativeKind::XsiType,
        )
    }

    /// Attaches exclusive alternatives represented by concrete XML element
    /// names from one XSD substitution group.
    pub fn with_substitution_group_alternatives(
        mut self,
        alternatives: Vec<GroupAlternative>,
    ) -> Option<Self> {
        self.set_substitution_group_alternatives(alternatives)
            .then_some(self)
    }

    pub fn set_substitution_group_alternatives(
        &mut self,
        alternatives: Vec<GroupAlternative>,
    ) -> bool {
        self.set_group_alternatives(
            alternatives,
            GroupAlternativeMode::Exclusive,
            XmlAlternativeKind::SubstitutionGroup,
        )
    }

    fn set_group_alternatives(
        &mut self,
        alternatives: Vec<GroupAlternative>,
        mode: GroupAlternativeMode,
        xml_kind: XmlAlternativeKind,
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
        self.xml_alternative_kind = xml_kind;
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

    /// Checks that element-name alternatives stay exclusive and group-scoped.
    pub fn xml_alternative_kind_is_valid(&self) -> bool {
        match self.xml_alternative_kind {
            XmlAlternativeKind::XsiType => true,
            XmlAlternativeKind::SubstitutionGroup => {
                self.alternative_mode.is_exclusive()
                    && self.recursive_ref.is_none()
                    && !self.attribute
                    && !self.text
                    && matches!(
                        &self.kind,
                        SchemaKind::Group { alternatives, .. } if !alternatives.is_empty()
                    )
            }
        }
    }

    pub fn alternative_mode(&self) -> GroupAlternativeMode {
        self.alternative_mode
    }

    pub fn xml_repeating_sequences_are_valid(&self) -> bool {
        let SchemaKind::Group { children, .. } = &self.kind else {
            return self.xml_repeating_sequences.is_empty();
        };
        let mut used = std::collections::BTreeSet::new();
        self.xml_repeating_sequences.iter().all(|sequence| {
            let positions = sequence
                .members
                .iter()
                .map(|member| {
                    let mut matches = children.iter().enumerate().filter(|(_, child)| {
                        child.name == member.name
                            && child.repeating
                            && !child.attribute
                            && !child.text
                    });
                    let position = matches.next().map(|(position, _)| position)?;
                    matches.next().is_none().then_some(position)
                })
                .collect::<Option<Vec<_>>>();
            sequence.members.len() > 1
                && sequence
                    .members
                    .iter()
                    .all(|member| !member.name.is_empty() && used.insert(member.name.as_str()))
                && positions.is_some_and(|positions| {
                    positions.windows(2).all(|pair| pair[1] == pair[0] + 1)
                })
        })
    }

    pub fn set_xml_repeating_sequences(&mut self, sequences: Vec<XmlRepeatingSequence>) -> bool {
        let previous = std::mem::replace(&mut self.xml_repeating_sequences, sequences);
        if self.xml_repeating_sequences_are_valid() {
            true
        } else {
            self.xml_repeating_sequences = previous;
            false
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

    pub fn nillable(mut self) -> Self {
        self.nillable = true;
        self
    }

    /// Marks this scalar as accepting an explicit JSON `null`.
    pub fn nullable(mut self) -> Option<Self> {
        self.nullable = true;
        self.nullable_is_valid().then_some(self)
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
    !alternatives.is_empty()
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
                                        SchemaKind::Scalar { ty }
                                            if constraint.value.is_valid_for(ty)
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
        let json_null = serde_json::to_string(&Value::json_null()).unwrap();
        assert_eq!(json_null, r#"{"$json_null":true}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&json_null).unwrap(),
            Value::json_null()
        );
        assert!(serde_json::from_str::<Value>(r#"{"$json_null":false}"#).is_err());
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
        let singleton = group
            .clone()
            .with_alternatives(vec![GroupAlternative {
                name: "domestic".into(),
                members: vec!["state".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            }])
            .unwrap();
        assert_eq!(singleton.alternatives().len(), 1);
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
                    value: GroupAlternativeConstraintValue::String("created".into()),
                }],
            },
            GroupAlternative {
                name: "deleted".into(),
                members: vec!["kind".into(), "value".into()],
                required: vec!["kind".into(), "value".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: GroupAlternativeConstraintValue::String("deleted".into()),
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

        let typed_discriminators = SchemaNode::group(
            "Typed",
            vec![
                SchemaNode::scalar("code", ScalarType::Int),
                SchemaNode::scalar("ratio", ScalarType::Float),
                SchemaNode::scalar("active", ScalarType::Bool),
            ],
        )
        .with_alternatives(vec![
            GroupAlternative {
                name: "first".into(),
                members: vec!["code".into(), "ratio".into(), "active".into()],
                required: vec!["code".into(), "ratio".into(), "active".into()],
                constraints: vec![
                    GroupAlternativeConstraint {
                        member: "code".into(),
                        value: GroupAlternativeConstraintValue::Int(1),
                    },
                    GroupAlternativeConstraint {
                        member: "ratio".into(),
                        value: GroupAlternativeConstraintValue::Float(FiniteF64::new(1.5).unwrap()),
                    },
                    GroupAlternativeConstraint {
                        member: "active".into(),
                        value: GroupAlternativeConstraintValue::Bool(true),
                    },
                ],
            },
            GroupAlternative {
                name: "second".into(),
                members: vec!["code".into(), "ratio".into(), "active".into()],
                required: vec!["code".into(), "ratio".into(), "active".into()],
                constraints: vec![
                    GroupAlternativeConstraint {
                        member: "code".into(),
                        value: GroupAlternativeConstraintValue::Int(2),
                    },
                    GroupAlternativeConstraint {
                        member: "ratio".into(),
                        value: GroupAlternativeConstraintValue::Float(FiniteF64::new(2.5).unwrap()),
                    },
                    GroupAlternativeConstraint {
                        member: "active".into(),
                        value: GroupAlternativeConstraintValue::Bool(false),
                    },
                ],
            },
        ])
        .unwrap();
        assert_eq!(
            serde_json::from_str::<SchemaNode>(
                &serde_json::to_string(&typed_discriminators).unwrap()
            )
            .unwrap(),
            typed_discriminators
        );

        let mut wrong_type = typed_discriminators.alternatives().to_vec();
        wrong_type[0].constraints[0].value = GroupAlternativeConstraintValue::String("1".into());
        assert!(
            SchemaNode::group(
                "Typed",
                vec![
                    SchemaNode::scalar("code", ScalarType::Int),
                    SchemaNode::scalar("ratio", ScalarType::Float),
                    SchemaNode::scalar("active", ScalarType::Bool),
                ],
            )
            .with_alternatives(wrong_type)
            .is_none()
        );

        assert!(FiniteF64::new(f64::NAN).is_none());
        assert!(FiniteF64::new(f64::INFINITY).is_none());
    }

    #[test]
    fn xml_substitution_alternatives_are_typed_validated_and_serde_defaulted() {
        let substitution = SchemaNode::group(
            "Creature",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        )
        .with_substitution_group_alternatives(vec![GroupAlternative {
            name: "{urn:ferrule:creatures}Cat".into(),
            members: vec!["name".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }])
        .unwrap();
        assert_eq!(
            substitution.xml_alternative_kind,
            XmlAlternativeKind::SubstitutionGroup
        );
        let encoded = serde_json::to_string(&substitution).unwrap();
        assert!(encoded.contains(r#""xml_alternative_kind":"substitution_group""#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            substitution
        );

        let legacy: SchemaNode =
            serde_json::from_str(r#"{"name":"Legacy","kind":{"kind":"group","children":[]}}"#)
                .unwrap();
        assert_eq!(legacy.xml_alternative_kind, XmlAlternativeKind::XsiType);
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"Invalid","xml_alternative_kind":"substitution_group","kind":{"kind":"scalar","ty":"string"}}"#
            )
            .is_err()
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

    #[test]
    fn json_nullability_is_scalar_only_and_serde_defaulted() {
        let nullable = SchemaNode::scalar("value", ScalarType::String)
            .nullable()
            .unwrap();
        let encoded = serde_json::to_string(&nullable).unwrap();
        assert!(encoded.contains("\"nullable\":true"));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            nullable
        );

        let old_json = r#"{"name":"value","kind":{"kind":"scalar","ty":"string"}}"#;
        let old = serde_json::from_str::<SchemaNode>(old_json).unwrap();
        assert!(!old.nullable);
        assert!(SchemaNode::group("object", Vec::new()).nullable().is_none());
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"object","nullable":true,"kind":{"kind":"group","children":[]}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn xml_namespace_identity_is_validated_and_serde_defaulted() {
        let qualified = SchemaNode::scalar("Code", ScalarType::String)
            .xml_qualified("urn:ferrule:test")
            .unwrap();
        let encoded = serde_json::to_string(&qualified).unwrap();
        assert!(encoded.contains(r#""kind":"qualified""#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            qualified
        );

        let unqualified = SchemaNode::scalar("Plain", ScalarType::String).xml_unqualified();
        assert!(
            unqualified
                .xml_namespace
                .as_ref()
                .is_some_and(|namespace| namespace.matches(None))
        );
        assert!(
            SchemaNode::scalar("Invalid", ScalarType::String)
                .xml_qualified("")
                .is_none()
        );

        let legacy: SchemaNode =
            serde_json::from_str(r#"{"name":"Code","kind":{"kind":"scalar","ty":"string"}}"#)
                .unwrap();
        assert!(legacy.xml_namespace.is_none());
        assert!(serde_json::from_str::<SchemaNode>(
            r#"{"name":"Code","xml_namespace":{"kind":"qualified","uri":""},"kind":{"kind":"scalar","ty":"string"}}"#,
        )
        .is_err());
    }

    #[test]
    fn xml_repeating_sequences_are_group_scoped_and_serde_validated() {
        let sequence = XmlRepeatingSequence {
            required: true,
            members: vec![
                XmlSequenceMember {
                    name: "Date".into(),
                    required: true,
                    repeating: false,
                },
                XmlSequenceMember {
                    name: "Note".into(),
                    required: false,
                    repeating: false,
                },
            ],
        };
        let mut schema = SchemaNode::group(
            "Rows",
            vec![
                SchemaNode::scalar("Date", ScalarType::String).repeating(),
                SchemaNode::scalar("Note", ScalarType::String).repeating(),
            ],
        );
        assert!(schema.set_xml_repeating_sequences(vec![sequence]));
        let encoded = serde_json::to_string(&schema).unwrap();
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            schema
        );

        let invalid = r#"{
          "name":"Rows",
          "xml_repeating_sequences":[{"required":true,"members":[
            {"name":"Date","required":true,"repeating":false},
            {"name":"Missing","required":false,"repeating":false}
          ]}],
          "kind":{"kind":"group","children":[
            {"name":"Date","repeating":true,"kind":{"kind":"scalar","ty":"string"}}
          ]}
        }"#;
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());

        let misplaced = r#"{
          "name":"Rows",
          "xml_repeating_sequences":[{"members":[
            {"name":"Date","required":true,"repeating":false},
            {"name":"Note","required":false,"repeating":false}
          ]}],
          "kind":{"kind":"group","children":[
            {"name":"Date","repeating":true,"kind":{"kind":"scalar","ty":"string"}},
            {"name":"Other","kind":{"kind":"scalar","ty":"string"}},
            {"name":"Note","repeating":true,"kind":{"kind":"scalar","ty":"string"}}
          ]}
        }"#;
        assert!(serde_json::from_str::<SchemaNode>(misplaced).is_err());
    }

    #[test]
    fn database_relations_are_nested_group_scoped_and_serde_validated() {
        let relation = DatabaseRelation {
            parent_column: "id".into(),
            child_column: "parent_id".into(),
            foreign_key_side: DatabaseForeignKeySide::Child,
        };
        let child = SchemaNode::group("children|parent_id", Vec::new())
            .repeating()
            .with_database_relation(relation.clone())
            .unwrap();
        let encoded = serde_json::to_string(&child).unwrap();
        assert!(encoded.contains(r#""database_relation""#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded).unwrap(), child);

        assert!(
            SchemaNode::group("children|wrong", Vec::new())
                .repeating()
                .with_database_relation(relation.clone())
                .is_none()
        );
        assert!(
            SchemaNode::scalar("children|parent_id", ScalarType::String)
                .repeating()
                .with_database_relation(relation)
                .is_none()
        );
        let legacy: SchemaNode = serde_json::from_str(
            r#"{"name":"children|parent_id","repeating":true,"kind":{"kind":"group","children":[]}}"#,
        )
        .unwrap();
        assert!(legacy.database_relation.is_none());
    }
}
