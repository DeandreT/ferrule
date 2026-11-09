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

mod decimal;
mod decimal_value;
mod exact_float;
mod flat_rows;
mod instance;
mod instance_group;
mod primary_root;
mod scalar_lexical;
mod schema;
mod schema_node;
mod source_origin;
mod xml_scalar_output;
mod xml_schema_hints;
pub use xml_scalar_output::{
    MAX_XML_SCALAR_OUTPUT_DETAIL_BYTES, MAX_XML_SCALAR_OUTPUT_DOCUMENT_DEPTH,
    MAX_XML_SCALAR_OUTPUT_DOCUMENT_NODES, MAX_XML_SCALAR_OUTPUT_FIELDS,
    MAX_XML_SCALAR_OUTPUT_NAME_BYTES, MAX_XML_SCALAR_OUTPUT_NAMES_BYTES, XmlDoubleFields,
    XmlScalarOutputError, XmlScalarOutputPolicy, XmlScalarOutputUnresolved,
    XmlScalarOutputUnresolvedReason, XmlScalarPreservation,
};
mod xml_structured_input;
mod xml_type_origin;
mod xml_wildcard_exclusions;
pub use xml_schema_hints::{
    MAX_XML_SCHEMA_HINT_PAIRS, MAX_XML_SCHEMA_HINT_TOKEN_BYTES, XmlSchemaHintDerivation,
    XmlSchemaHints, XmlSchemaHintsError, XmlSchemaLocation,
};

pub use decimal::{
    Decimal96, DecimalError, MAX_DECIMAL_COEFFICIENT, MAX_DECIMAL_LEXICAL_BYTES, MAX_DECIMAL_SCALE,
};
pub use flat_rows::{flat_row_schema_is_supported, is_flat_row};
pub use instance::{DocumentMember, Instance};
pub use instance_group::InstanceGroup;
pub use primary_root::{
    MAX_PRIMARY_ROOT_FIELDS, MAX_PRIMARY_ROOT_IDENTITY_BYTES, MAX_PRIMARY_ROOT_PATH_BYTES,
    MAX_PRIMARY_ROOT_PATH_SEGMENTS, PrimaryRootError, primary_root_ncname_is_valid,
    primary_root_padded_type_origin_is_valid, primary_root_scalar,
    primary_root_scalar_path_is_valid, primary_root_scalar_with_requirement,
    primary_root_schema_has_scalar, primary_root_schema_has_type, primary_root_schema_is_supported,
    primary_root_type_identity_is_valid, primary_root_xml_type_equals,
    xml_inactive_root_type_members_are_supported, xml_root_view_read_policy_is_supported,
};
pub use schema_node::SchemaNode;
pub(crate) use schema_node::schema_deserialization_is_active;
pub use xml_structured_input::{
    MAX_STRUCTURED_XML_DEPTH, MAX_STRUCTURED_XML_DOCUMENT_BYTES,
    MAX_STRUCTURED_XML_MATERIALIZED_BYTES, MAX_STRUCTURED_XML_MATERIALIZED_NODES,
    MAX_STRUCTURED_XML_NAMESPACE_REFERENCES, MAX_STRUCTURED_XML_NAMESPACE_REGISTRY_BYTES,
    MAX_STRUCTURED_XML_PARSER_RESERVATION_SLOTS, MAX_STRUCTURED_XML_PARSER_WORK,
    MAX_STRUCTURED_XML_PHYSICAL_NODES, MAX_STRUCTURED_XML_PROJECTION_WORK,
    MAX_STRUCTURED_XML_SCHEMA_NODES, xml_structured_document_input_is_supported,
};
pub use xml_type_origin::{XmlTypeOrigin, XmlTypeOriginError};
pub use xml_wildcard_exclusions::{
    MAX_XML_WILDCARD_EXCLUDED_NAMES, XmlExpandedName, XmlWildcardExcludedNames,
    XmlWildcardExcludedNamesError,
};

pub use exact_float::i64_to_exact_f64;
pub use scalar_lexical::parse_exact_decimal_i64;
pub use source_origin::{
    MAX_SOURCE_ORIGIN_PATH_BYTES, SourceDocumentOrigin, SourceOriginError, SourceRowOrigin,
};

pub use schema::{
    EffectiveCardinality, EffectiveNullability, EffectivePresence, EffectiveSchemaView,
    EffectiveViewError, EffectiveXmlView, IntegerRange, ItemCountRange, JSON_TUPLE_MEMBER_PREFIX,
    JSON_TUPLE_REST_FIELD, JsonAllowedValue, JsonAllowedValues, JsonAllowedValuesError,
    JsonArrayWrapperFrameError, JsonContainsConstraint, JsonContainsConstraints,
    JsonContainsPredicate, JsonDependentSchemaConstraint, JsonDependentSchemaConstraints,
    JsonFormatAnnotations, JsonFormatAnnotationsError, JsonMultipleOf, JsonMultipleOfConstraints,
    JsonMultipleOfConstraintsError, JsonNullability, JsonPatternConstraints,
    JsonPatternConstraintsError, JsonPatternPropertyNames, JsonPropertyDependencies,
    JsonPropertyDependenciesError, JsonPropertyNameConstraints, JsonPropertyNameSet,
    JsonPropertyNameSetError, JsonSchemaPredicate, JsonTupleFrame, JsonTupleFrameError,
    JsonTupleLayout, JsonTupleTail, JsonUndeclaredPropertyError, MAX_DISTINCT_JSON_PATTERNS,
    MAX_JSON_ALLOWED_VALUE_STRING_BYTES, MAX_JSON_ALLOWED_VALUE_TOTAL_STRING_BYTES,
    MAX_JSON_ALLOWED_VALUES, MAX_JSON_CONTAINS_CONSTRAINTS, MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS,
    MAX_JSON_FORMAT_ANNOTATION_BYTES, MAX_JSON_FORMAT_ANNOTATION_TOTAL_BYTES,
    MAX_JSON_FORMAT_ANNOTATIONS, MAX_JSON_MULTIPLE_OF_ALTERNATIVES, MAX_JSON_MULTIPLE_OF_TERMS,
    MAX_JSON_PATTERN_ALTERNATIVES, MAX_JSON_PATTERN_INSTRUCTIONS, MAX_JSON_PATTERN_SOURCE_BYTES,
    MAX_JSON_PATTERN_TERMS, MAX_JSON_PROPERTY_DEPENDENCY_EDGES,
    MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES, MAX_JSON_PROPERTY_DEPENDENCY_TRIGGERS,
    MAX_JSON_PROPERTY_NAME_BYTES, MAX_JSON_PROPERTY_NAME_TOTAL_BYTES, MAX_JSON_PROPERTY_NAMES,
    MAX_JSON_TUPLE_MEMBERS, NumberBound, NumberRange, NumericRange, PropertyCountRange,
    RepeatedItems, StringLengthRange, XmlOccurrence, json_tuple_member_name,
};

/// Instance-field name used for an XML element's simple text content.
pub const XML_TEXT_FIELD: &str = "#text";

/// Reserved instance-group field carrying the selected XML type alternative.
/// Readers may infer this identity from content; writers use it to select the
/// output type. It does not prove that the source had an `xsi:type` attribute.
pub const XML_TYPE_FIELD: &str = "\u{1f}ferrule-xml-type";

/// Historical origin-marker spelling, retained for ordinary-data compatibility.
/// A field with this name is ordinary data and never supplies XML origin facts.
pub const XML_TYPE_ORIGIN_FIELD: &str = "\u{1f}ferrule-xml-type-origin";

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
pub const XML_NAMESPACE_URI_FIELD: &str = "NamespaceURI";

/// The scalar types a field can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    String,
    Int,
    Float,
    Bool,
}

impl ScalarType {
    const ALL: [Self; 4] = [Self::String, Self::Int, Self::Float, Self::Bool];

    const fn bit(self) -> u8 {
        match self {
            Self::String => 1 << 0,
            Self::Int => 1 << 1,
            Self::Float => 1 << 2,
            Self::Bool => 1 << 3,
        }
    }
}

/// A canonical set of at least two distinct scalar types.
///
/// Single scalar types remain [`SchemaKind::Scalar`]. Keeping this type's
/// representation private prevents heterogeneous schemas from carrying an
/// empty, singleton, duplicate, or order-dependent type declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalarTypeSet(u8);

impl ScalarTypeSet {
    pub fn new(types: impl IntoIterator<Item = ScalarType>) -> Option<Self> {
        let mut bits = 0_u8;
        for ty in types {
            let bit = ty.bit();
            if bits & bit != 0 {
                return None;
            }
            bits |= bit;
        }
        (bits.count_ones() >= 2).then_some(Self(bits))
    }

    pub const fn contains(self, ty: ScalarType) -> bool {
        self.0 & ty.bit() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = ScalarType> {
        ScalarType::ALL
            .into_iter()
            .filter(move |ty| self.contains(*ty))
    }
}

impl Serialize for ScalarTypeSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.iter().collect::<Vec<_>>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ScalarTypeSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let types = Vec::<ScalarType>::deserialize(deserializer)?;
        Self::new(types).ok_or_else(|| {
            serde::de::Error::custom(
                "scalar union types must contain at least two distinct scalar types",
            )
        })
    }
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

/// A nonempty, duplicate-free set of exact XML namespace identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct XmlWildcardNamespaceList(Vec<XmlNamespace>);

impl XmlWildcardNamespaceList {
    pub fn new(namespaces: impl IntoIterator<Item = XmlNamespace>) -> Option<Self> {
        let mut unique = Vec::new();
        for namespace in namespaces {
            if unique.contains(&namespace) {
                return None;
            }
            unique.push(namespace);
        }
        (!unique.is_empty()).then_some(Self(unique))
    }

    pub fn iter(&self) -> impl Iterator<Item = &XmlNamespace> {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for XmlWildcardNamespaceList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let namespaces = Vec::<XmlNamespace>::deserialize(deserializer)?;
        Self::new(namespaces).ok_or_else(|| {
            serde::de::Error::custom(
                "XML wildcard namespace lists must be nonempty and duplicate-free",
            )
        })
    }
}

/// Exact namespace predicate for one XSD element wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum XmlWildcardNamespaceConstraint {
    Any,
    Other {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_namespace: Option<XmlNamespaceUri>,
    },
    List {
        namespaces: XmlWildcardNamespaceList,
    },
    /// An XSD 1.1 `notNamespace` set. It stays distinct from `Other`, which
    /// models `##other`, even when both reject the same namespaces.
    Not {
        namespaces: XmlWildcardNamespaceList,
    },
}

impl XmlWildcardNamespaceConstraint {
    pub fn list(namespaces: impl IntoIterator<Item = XmlNamespace>) -> Option<Self> {
        Some(Self::List {
            namespaces: XmlWildcardNamespaceList::new(namespaces)?,
        })
    }

    pub fn allows(&self, namespace: Option<&str>) -> bool {
        match self {
            Self::Any => true,
            Self::Other { target_namespace } => namespace.is_some_and(|namespace| {
                !namespace.is_empty()
                    && target_namespace
                        .as_ref()
                        .is_none_or(|target| namespace != target.as_str())
            }),
            Self::List { namespaces } => namespaces
                .iter()
                .any(|candidate| candidate.matches(namespace)),
            Self::Not { namespaces } => !namespaces
                .iter()
                .any(|candidate| candidate.matches(namespace)),
        }
    }

    pub fn is_local_only(&self) -> bool {
        matches!(
            self,
            Self::List { namespaces }
                if namespaces.0.as_slice() == [XmlNamespace::Unqualified]
        )
    }
}

/// Validation policy for names selected by an XML wildcard.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XmlWildcardProcessContents {
    #[default]
    Skip,
    Lax,
    Strict,
}

impl XmlWildcardProcessContents {
    pub fn is_skip(&self) -> bool {
        *self == Self::Skip
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
    /// Explicit bounded decimal identity, distinct from integer and binary64.
    /// The box preserves the existing scalar/instance union layout.
    Decimal(#[serde(with = "decimal_value")] Box<Decimal96>),
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
            Value::Decimal(_) => "decimal",
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

    pub fn decimal(value: Decimal96) -> Self {
        Self::Decimal(Box::new(value))
    }

    pub fn as_decimal(&self) -> Option<&Decimal96> {
        match self {
            Self::Decimal(value) => Some(value),
            _ => None,
        }
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

fn default_xml_choice_repeating() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// One `xs:choice` whose occurrences are projected onto named child ports.
/// Each occurrence selects exactly one member. Repeating choices project
/// repeating children; singular choices project non-repeating children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlRepeatingChoice {
    #[serde(default)]
    pub required: bool,
    #[serde(
        default = "default_xml_choice_repeating",
        skip_serializing_if = "is_true"
    )]
    pub repeating: bool,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlSequenceMember {
    pub name: String,
    pub required: bool,
    pub repeating: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaKind {
    Scalar {
        ty: ScalarType,
    },
    ScalarUnion {
        types: ScalarTypeSet,
    },
    Group {
        children: Vec<SchemaNode>,
        /// Explicit compatible object/type alternatives represented by the
        /// merged `children` projection. Empty for ordinary groups.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alternatives: Vec<GroupAlternative>,
        /// JSON object property names whose presence is mandatory.
        ///
        /// Names must identify a declared child unless `dynamic` is present,
        /// in which case a required runtime-named property is also valid.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        required: Vec<String>,
        /// `xsi:type` alternatives declared through `xs:restriction`.
        ///
        /// Names are exact alternative identities. Keeping this distinction
        /// prevents XSD export from guessing whether nested member sets were
        /// formed by extension or restriction.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        xml_restricted_alternatives: Vec<String>,
        /// Schema shared by computed object fields whose names are supplied
        /// at mapping run time. Closed groups leave this unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<Box<SchemaNode>>,
    },
}

/// Maximum number of items inspected by one exact JSON `uniqueItems`
/// assertion.
pub const MAX_JSON_UNIQUE_ITEMS: usize = 1_000_000;
/// The declared item role of an explicit nested JSON array frame.
pub const JSON_ARRAY_ITEM_FIELD: &str = "item";
/// Maximum number of scalar/container nodes canonicalized by one exact JSON
/// `uniqueItems` assertion.
pub const MAX_JSON_UNIQUE_KEY_NODES: usize = 1_000_000;
/// Maximum cumulative UTF-8 bytes retained in canonical unique-item keys.
pub const MAX_JSON_UNIQUE_KEY_BYTES: usize = 64 * 1024 * 1024;

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
    /// Scalar values that constrain this alternative when their member is
    /// present. Members listed in `required` must also be present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<GroupAlternativeConstraint>,
}

/// One exact scalar value used to select a group alternative when present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupAlternativeConstraint {
    pub member: String,
    pub value: GroupAlternativeConstraintValue,
}

/// A JSON-compatible scalar discriminator whose value can survive the IR
/// without losing its declared type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GroupAlternativeConstraintValue {
    String(String),
    Int(i64),
    Float(FiniteF64),
    Bool(bool),
    JsonNull,
}

impl GroupAlternativeConstraintValue {
    fn scalar_type(&self) -> Option<ScalarType> {
        match self {
            Self::String(_) => Some(ScalarType::String),
            Self::Int(_) => Some(ScalarType::Int),
            Self::Float(_) => Some(ScalarType::Float),
            Self::Bool(_) => Some(ScalarType::Bool),
            Self::JsonNull => None,
        }
    }

    fn is_valid_for(&self, ty: ScalarType, nullable: bool) -> bool {
        matches!(
            (self, ty),
            (Self::String(_), ScalarType::String)
                | (Self::Int(_), ScalarType::Int)
                | (Self::Bool(_), ScalarType::Bool)
        ) || matches!((self, ty), (Self::Float(_), ScalarType::Float))
            || matches!(self, Self::JsonNull) && nullable
    }
}

/// One finite 64-bit float. Construction and deserialization reject infinities
/// and NaN so scalar constraints are always JSON-serializable.
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

#[cfg(test)]
mod tests;
