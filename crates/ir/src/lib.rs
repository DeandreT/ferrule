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

mod schema;

pub use schema::{
    IntegerRange, ItemCountRange, JsonAllowedValue, JsonAllowedValues, JsonAllowedValuesError,
    JsonFormatAnnotations, JsonFormatAnnotationsError, JsonMultipleOf, JsonMultipleOfConstraints,
    JsonMultipleOfConstraintsError, JsonPatternConstraints, JsonPatternConstraintsError,
    JsonPropertyDependencies, JsonPropertyDependenciesError, MAX_DISTINCT_JSON_PATTERNS,
    MAX_JSON_ALLOWED_VALUE_STRING_BYTES, MAX_JSON_ALLOWED_VALUE_TOTAL_STRING_BYTES,
    MAX_JSON_ALLOWED_VALUES, MAX_JSON_FORMAT_ANNOTATION_BYTES,
    MAX_JSON_FORMAT_ANNOTATION_TOTAL_BYTES, MAX_JSON_FORMAT_ANNOTATIONS,
    MAX_JSON_MULTIPLE_OF_ALTERNATIVES, MAX_JSON_MULTIPLE_OF_TERMS, MAX_JSON_PATTERN_ALTERNATIVES,
    MAX_JSON_PATTERN_INSTRUCTIONS, MAX_JSON_PATTERN_SOURCE_BYTES, MAX_JSON_PATTERN_TERMS,
    MAX_JSON_PROPERTY_DEPENDENCY_EDGES, MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES,
    MAX_JSON_PROPERTY_DEPENDENCY_TRIGGERS, NumberBound, NumberRange, NumericRange,
    PropertyCountRange, StringLengthRange,
};

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
    /// Additional exact XML namespace identities accepted for this local
    /// element name.
    ///
    /// Strict wildcards can expose global declarations from different
    /// namespaces that intentionally share one mapping-port name and shape.
    /// The first identity remains in [`Self::xml_namespace`]; XML boundaries
    /// retain the selected identity for each occurrence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xml_name_alternatives: Vec<XmlNamespace>,
    /// Exact namespace predicate for a generic `element()` or `attribute()`
    /// group imported from an XML Schema wildcard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml_wildcard_namespace: Option<XmlWildcardNamespaceConstraint>,
    /// Validation policy for one generic XML wildcard group.
    #[serde(default, skip_serializing_if = "XmlWildcardProcessContents::is_skip")]
    pub xml_wildcard_process_contents: XmlWildcardProcessContents,
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
    /// This JSON object or array may itself be the explicit `null` value.
    ///
    /// This is separate from [`Self::nullable`]: on a repeating scalar node,
    /// that flag applies to each array item while this flag applies to the
    /// array wrapper.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub container_nullable: bool,
    /// This string scalar stores one arbitrary JSON value as canonical JSON
    /// text. JSON boundaries use it for unconstrained dynamic object fields;
    /// other formats and the mapping graph continue to see an ordinary string.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub json_any: bool,
    /// A required literal value for a scalar node (XSD's `xs:fixed`, JSON
    /// Schema's `const`), compared against the raw text before parsing.
    /// Format adapters use it both to validate and to disambiguate --
    /// notably EDI qualifier elements, where e.g. two loops both starting
    /// with an `HL` segment are told apart by `HL03` being `20` vs `22`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<String>,
    /// A canonical finite set of exact JSON scalar values accepted by this
    /// node. This is distinct from [`Self::fixed`], whose lexical semantics
    /// are also used by XML and EDI adapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_allowed_values: Option<JsonAllowedValues>,
    /// An exact numeric interval for a JSON scalar.
    ///
    /// Integer bounds are normalized to an inclusive `i64` interval. Number
    /// bounds retain finite values and endpoint exclusivity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric_range: Option<NumericRange>,
    /// Exact JSON Schema `multipleOf` constraints for a numeric-capable
    /// scalar. Outer alternatives are ORed; every divisor inside one
    /// alternative must divide the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_multiple_of: Option<JsonMultipleOfConstraints>,
    /// Exact cardinality bounds for a repeating JSON array node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_count_range: Option<ItemCountRange>,
    /// Exact cardinality bounds for the properties of a JSON object node.
    ///
    /// On a repeating group these bounds apply to each object item, while
    /// [`Self::item_count_range`] applies to the enclosing array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property_count_range: Option<PropertyCountRange>,
    /// Exact JSON Schema property-presence implications for this object.
    ///
    /// When one trigger property is present, every property named by its rule
    /// must also be present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_property_dependencies: Option<JsonPropertyDependencies>,
    /// Whether one repeating JSON array requires pairwise-distinct items
    /// under JSON Schema's structural equality rules.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub json_unique_items: bool,
    /// Exact length bounds for a JSON string scalar in Unicode scalar values.
    ///
    /// On a repeating scalar node these bounds apply to each array item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_length_range: Option<StringLengthRange>,
    /// Exact JSON Schema pattern constraints for this string-capable scalar.
    ///
    /// The outer alternatives are ORed; every pattern inside one alternative
    /// must match. On a repeating scalar node these constraints apply to each
    /// array item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_patterns: Option<JsonPatternConstraints>,
    /// Ordered JSON Schema `format` annotations for this string-capable scalar
    /// value, or for each item when this node is repeating.
    #[serde(default, skip_serializing_if = "JsonFormatAnnotations::is_empty")]
    pub json_formats: JsonFormatAnnotations,
    /// An XML Schema default lexical value for a scalar element, simple
    /// content value, or ordinary attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
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
    /// Repeating XML choices flattened into independently addressable named
    /// children. XML adapters retain the original cross-member occurrence
    /// order and export the fields under one repeating `xs:choice`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xml_repeating_choices: Vec<XmlRepeatingChoice>,
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
            xml_name_alternatives: Vec<XmlNamespace>,
            #[serde(default)]
            xml_wildcard_namespace: Option<XmlWildcardNamespaceConstraint>,
            #[serde(default)]
            xml_wildcard_process_contents: XmlWildcardProcessContents,
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
            container_nullable: bool,
            #[serde(default)]
            json_any: bool,
            #[serde(default)]
            fixed: Option<String>,
            #[serde(default)]
            json_allowed_values: Option<JsonAllowedValues>,
            #[serde(default)]
            numeric_range: Option<NumericRange>,
            #[serde(default)]
            json_multiple_of: Option<JsonMultipleOfConstraints>,
            #[serde(default)]
            item_count_range: Option<ItemCountRange>,
            #[serde(default)]
            property_count_range: Option<PropertyCountRange>,
            #[serde(default)]
            json_property_dependencies: Option<JsonPropertyDependencies>,
            #[serde(default)]
            json_unique_items: bool,
            #[serde(default)]
            string_length_range: Option<StringLengthRange>,
            #[serde(default)]
            json_patterns: Option<JsonPatternConstraints>,
            #[serde(default)]
            json_formats: JsonFormatAnnotations,
            #[serde(default)]
            default: Option<String>,
            #[serde(default)]
            value_generation: Option<ValueGeneration>,
            #[serde(default)]
            alternative_mode: GroupAlternativeMode,
            #[serde(default)]
            xml_alternative_kind: XmlAlternativeKind,
            #[serde(default)]
            xml_repeating_sequences: Vec<XmlRepeatingSequence>,
            #[serde(default)]
            xml_repeating_choices: Vec<XmlRepeatingChoice>,
            #[serde(default)]
            database_relation: Option<DatabaseRelation>,
            kind: SchemaKind,
        }

        let repr = Repr::deserialize(deserializer)?;
        let node = Self {
            name: repr.name,
            xml_namespace: repr.xml_namespace,
            xml_name_alternatives: repr.xml_name_alternatives,
            xml_wildcard_namespace: repr.xml_wildcard_namespace,
            xml_wildcard_process_contents: repr.xml_wildcard_process_contents,
            repeating: repr.repeating,
            recursive_ref: repr.recursive_ref,
            attribute: repr.attribute,
            text: repr.text,
            nillable: repr.nillable,
            nullable: repr.nullable,
            container_nullable: repr.container_nullable,
            json_any: repr.json_any,
            fixed: repr.fixed,
            json_allowed_values: repr.json_allowed_values,
            numeric_range: repr.numeric_range,
            json_multiple_of: repr.json_multiple_of,
            item_count_range: repr.item_count_range,
            property_count_range: repr.property_count_range,
            json_property_dependencies: repr.json_property_dependencies,
            json_unique_items: repr.json_unique_items,
            string_length_range: repr.string_length_range,
            json_patterns: repr.json_patterns,
            json_formats: repr.json_formats,
            default: repr.default,
            value_generation: repr.value_generation,
            alternative_mode: repr.alternative_mode,
            xml_alternative_kind: repr.xml_alternative_kind,
            xml_repeating_sequences: repr.xml_repeating_sequences,
            xml_repeating_choices: repr.xml_repeating_choices,
            database_relation: repr.database_relation,
            kind: repr.kind,
        };
        if !node.metadata_is_valid() {
            return Err(serde::de::Error::custom(
                "schema metadata contains invalid alternatives, required fields, recursion, fixed or JSON allowed values, numeric range, JSON multipleOf constraints, item-count, property-count, property-dependency, or unique-items constraints, string-length range, JSON pattern constraints or format annotations, value generation, default value, alternative mode, XML alternative kind, XML name alternatives, XML repeating sequences or choices, XML wildcard namespace or process policy, database relation, or JSON nullability",
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl SchemaNode {
    /// Checks every cross-field schema metadata invariant.
    pub fn metadata_is_valid(&self) -> bool {
        self.alternatives_are_valid()
            && self.required_fields_are_valid()
            && self.xml_name_alternatives_are_valid()
            && self.recursive_ref_is_valid()
            && self.fixed_is_valid()
            && self.json_allowed_values_are_valid()
            && self.numeric_range_is_valid()
            && self.json_multiple_of_is_valid()
            && self.item_count_range_is_valid()
            && self.property_count_range_is_valid()
            && self.json_property_dependencies_are_valid()
            && self.json_unique_items_is_valid()
            && self.string_length_range_is_valid()
            && self.json_patterns_are_valid()
            && self.json_formats_are_valid()
            && self.value_generation_is_valid()
            && self.default_is_valid()
            && self.alternative_mode_is_valid()
            && self.xml_alternative_kind_is_valid()
            && self.xml_repeating_sequences_are_valid()
            && self.xml_repeating_choices_are_valid()
            && self.database_relation_is_valid()
            && self.nullable_is_valid()
            && self.container_nullable_is_valid()
            && self.json_any_is_valid()
            && self.xml_wildcard_namespace_is_valid()
            && self.xml_wildcard_process_contents_is_valid()
    }

    pub fn scalar(name: impl Into<String>, ty: ScalarType) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            xml_name_alternatives: Vec::new(),
            xml_wildcard_namespace: None,
            xml_wildcard_process_contents: XmlWildcardProcessContents::Skip,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            nullable: false,
            container_nullable: false,
            json_any: false,
            fixed: None,
            json_allowed_values: None,
            numeric_range: None,
            json_multiple_of: None,
            item_count_range: None,
            property_count_range: None,
            json_property_dependencies: None,
            json_unique_items: false,
            string_length_range: None,
            json_patterns: None,
            json_formats: JsonFormatAnnotations::default(),
            default: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_repeating_sequences: Vec::new(),
            xml_repeating_choices: Vec::new(),
            database_relation: None,
            kind: SchemaKind::Scalar { ty },
        }
    }

    pub fn scalar_fixed(name: impl Into<String>, ty: ScalarType, value: impl Into<String>) -> Self {
        let mut node = Self::scalar(name, ty);
        node.fixed = Some(value.into());
        node
    }

    pub fn scalar_union(name: impl Into<String>, types: ScalarTypeSet) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            xml_name_alternatives: Vec::new(),
            xml_wildcard_namespace: None,
            xml_wildcard_process_contents: XmlWildcardProcessContents::Skip,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            nullable: false,
            container_nullable: false,
            json_any: false,
            fixed: None,
            json_allowed_values: None,
            numeric_range: None,
            json_multiple_of: None,
            item_count_range: None,
            property_count_range: None,
            json_property_dependencies: None,
            json_unique_items: false,
            string_length_range: None,
            json_patterns: None,
            json_formats: JsonFormatAnnotations::default(),
            default: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_repeating_sequences: Vec::new(),
            xml_repeating_choices: Vec::new(),
            database_relation: None,
            kind: SchemaKind::ScalarUnion { types },
        }
    }

    pub fn is_scalar(&self) -> bool {
        matches!(
            self.kind,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }
        )
    }

    pub fn accepts_scalar_type(&self, ty: ScalarType) -> bool {
        match self.kind {
            SchemaKind::Scalar { ty: expected } => expected == ty,
            SchemaKind::ScalarUnion { types } => types.contains(ty),
            SchemaKind::Group { .. } => false,
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<SchemaNode>) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            xml_name_alternatives: Vec::new(),
            xml_wildcard_namespace: None,
            xml_wildcard_process_contents: XmlWildcardProcessContents::Skip,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            nullable: false,
            container_nullable: false,
            json_any: false,
            fixed: None,
            json_allowed_values: None,
            numeric_range: None,
            json_multiple_of: None,
            item_count_range: None,
            property_count_range: None,
            json_property_dependencies: None,
            json_unique_items: false,
            string_length_range: None,
            json_patterns: None,
            json_formats: JsonFormatAnnotations::default(),
            default: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_repeating_sequences: Vec::new(),
            xml_repeating_choices: Vec::new(),
            database_relation: None,
            kind: SchemaKind::Group {
                children,
                alternatives: Vec::new(),
                required: Vec::new(),
                xml_restricted_alternatives: Vec::new(),
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
                    required,
                    xml_restricted_alternatives,
                    dynamic,
                } if children.is_empty()
                    && alternatives.is_empty()
                    && required.is_empty()
                    && xml_restricted_alternatives.is_empty()
                    && dynamic.is_none()
            )
            && self.alternative_mode.is_exclusive()
            && self.xml_alternative_kind.is_xsi_type()
    }

    /// Marks this XML name as explicitly unqualified.
    pub fn xml_unqualified(mut self) -> Self {
        self.xml_namespace = Some(XmlNamespace::Unqualified);
        self.xml_name_alternatives.clear();
        self
    }

    /// Marks this XML name as belonging to a non-empty namespace URI.
    pub fn xml_qualified(mut self, uri: impl Into<String>) -> Option<Self> {
        self.xml_namespace = Some(XmlNamespace::qualified(uri)?);
        self.xml_name_alternatives.clear();
        Some(self)
    }

    /// Adds exact namespace identities for XML elements that share this
    /// mapping-port local name and complete typed shape.
    pub fn with_xml_name_alternatives(mut self, alternatives: Vec<XmlNamespace>) -> Option<Self> {
        let previous = std::mem::replace(&mut self.xml_name_alternatives, alternatives);
        if self.xml_name_alternatives_are_valid() {
            Some(self)
        } else {
            self.xml_name_alternatives = previous;
            None
        }
    }

    pub fn xml_name_alternatives_are_valid(&self) -> bool {
        if self.xml_name_alternatives.is_empty() {
            return true;
        }
        let Some(primary) = &self.xml_namespace else {
            return false;
        };
        !self.attribute
            && !self.text
            && self.recursive_ref.is_none()
            && self.xml_alternative_kind != XmlAlternativeKind::SubstitutionGroup
            && self
                .xml_name_alternatives
                .iter()
                .enumerate()
                .all(|(index, namespace)| {
                    namespace != primary && !self.xml_name_alternatives[..index].contains(namespace)
                })
    }

    /// Returns whether one runtime XML namespace is an exact accepted name
    /// identity for this local element.
    pub fn xml_namespace_matches(&self, namespace: Option<&str>) -> bool {
        self.xml_namespace
            .iter()
            .chain(&self.xml_name_alternatives)
            .any(|candidate| candidate.matches(namespace))
    }

    pub fn with_xml_wildcard_namespace(
        mut self,
        constraint: XmlWildcardNamespaceConstraint,
    ) -> Option<Self> {
        self.xml_wildcard_namespace = Some(constraint);
        self.xml_wildcard_namespace_is_valid().then_some(self)
    }

    pub fn xml_wildcard_namespace_is_valid(&self) -> bool {
        self.xml_wildcard_namespace.is_none()
            || (matches!(
                self.name.as_str(),
                XML_ELEMENTS_FIELD | XML_ATTRIBUTES_FIELD
            ) && self.recursive_ref.is_none()
                && !self.attribute
                && !self.text
                && matches!(
                    &self.kind,
                    SchemaKind::Group { children, .. }
                        if [XML_LOCAL_NAME_FIELD, XML_NAMESPACE_URI_FIELD]
                            .into_iter()
                            .all(|name| children.iter().any(|child| {
                                child.name == name
                                    && !child.repeating
                                    && !child.attribute
                                    && !child.text
                                    && matches!(
                                        child.kind,
                                        SchemaKind::Scalar {
                                            ty: ScalarType::String
                                        }
                                    )
                            }))
                            && (self.name != XML_ATTRIBUTES_FIELD
                                || children.iter().any(|child| {
                                    child.name == XML_TEXT_FIELD
                                        && child.text
                                        && !child.repeating
                                        && !child.attribute
                                        && matches!(
                                            child.kind,
                                            SchemaKind::Scalar {
                                                ty: ScalarType::String
                                            }
                                        )
                                }))
                ))
    }

    pub fn xml_wildcard_process_contents_is_valid(&self) -> bool {
        self.xml_wildcard_process_contents.is_skip()
            || (self.xml_wildcard_namespace.is_some() && self.xml_wildcard_namespace_is_valid())
    }

    /// Checks that fixed-value metadata remains limited to one scalar type.
    pub fn fixed_is_valid(&self) -> bool {
        self.fixed.is_none()
            || (self.json_allowed_values.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Checks that JSON allowed-value metadata is compatible with this exact
    /// scalar domain and is the sole authority for JSON null membership.
    pub fn json_allowed_values_are_valid(&self) -> bool {
        let Some(values) = &self.json_allowed_values else {
            return true;
        };
        if self.json_any
            || self.fixed.is_some()
            || !self.is_scalar()
            || self.nullable != values.contains_json_null()
        {
            return false;
        }
        values.values().iter().all(|value| match value {
            JsonAllowedValue::String(_) => self.accepts_scalar_type(ScalarType::String),
            JsonAllowedValue::Int(_) => {
                self.accepts_scalar_type(ScalarType::Int)
                    || self.accepts_scalar_type(ScalarType::Float)
            }
            JsonAllowedValue::Float(_) => self.accepts_scalar_type(ScalarType::Float),
            JsonAllowedValue::Bool(_) => self.accepts_scalar_type(ScalarType::Bool),
            JsonAllowedValue::JsonNull => self.nullable,
        })
    }

    /// Recursively checks JSON allowed-value metadata for one schema tree.
    pub fn json_allowed_values_tree_is_valid(&self) -> bool {
        self.json_allowed_values_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_allowed_values_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_allowed_values_tree_is_valid)
                }
            }
    }

    /// Checks that numeric-range metadata matches one concrete numeric scalar
    /// and that an optional fixed lexical value lies inside the interval.
    pub fn numeric_range_is_valid(&self) -> bool {
        let Some(range) = self.numeric_range else {
            return true;
        };
        match (range, &self.kind) {
            (
                NumericRange::Integer(range),
                SchemaKind::Scalar {
                    ty: ScalarType::Int,
                },
            ) => self.fixed.as_deref().is_none_or(|fixed| {
                fixed
                    .parse::<i64>()
                    .is_ok_and(|value| range.contains(value))
            }),
            (
                NumericRange::Number(range),
                SchemaKind::Scalar {
                    ty: ScalarType::Float,
                },
            ) => self.fixed.as_deref().is_none_or(|fixed| {
                fixed
                    .parse::<f64>()
                    .is_ok_and(|value| range.contains(value))
            }),
            _ => false,
        }
    }

    /// Checks that JSON `multipleOf` metadata is attached to a numeric-capable
    /// scalar and that an optional fixed lexical value is exactly divisible.
    pub fn json_multiple_of_is_valid(&self) -> bool {
        let Some(constraints) = &self.json_multiple_of else {
            return true;
        };
        if self.json_any
            || !(self.accepts_scalar_type(ScalarType::Int)
                || self.accepts_scalar_type(ScalarType::Float))
        {
            return false;
        }
        self.fixed.as_deref().is_none_or(|fixed| match self.kind {
            SchemaKind::Scalar {
                ty: ScalarType::Int,
            } => fixed
                .parse::<i64>()
                .is_ok_and(|value| constraints.matches_i64(value)),
            SchemaKind::Scalar {
                ty: ScalarType::Float,
            } => fixed
                .parse::<f64>()
                .is_ok_and(|value| constraints.matches_f64(value)),
            _ => false,
        })
    }

    /// Checks JSON `multipleOf` metadata throughout one complete schema tree.
    pub fn json_multiple_of_tree_is_valid(&self) -> bool {
        self.json_multiple_of_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_multiple_of_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_multiple_of_tree_is_valid)
                }
            }
    }

    /// Checks that item-count metadata remains attached to an array wrapper.
    pub fn item_count_range_is_valid(&self) -> bool {
        self.item_count_range.is_none() || self.repeating
    }

    /// Checks that property-count metadata remains attached to an object and
    /// is compatible with its required fields and closed property capacity.
    pub fn property_count_range_is_valid(&self) -> bool {
        let Some(range) = self.property_count_range else {
            return true;
        };
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } = &self.kind
        else {
            return false;
        };
        let fits = |required: &[String], capacity: Option<&[String]>| {
            let required_count =
                effective_required_fields(required, self.json_property_dependencies.as_ref()).len();
            let required_fits = u64::try_from(required_count).map_or_else(
                |_| range.maximum().is_none(),
                |required_count| {
                    range
                        .maximum()
                        .is_none_or(|maximum| required_count <= maximum)
                },
            );
            if !required_fits {
                return false;
            }
            capacity.is_none_or(|capacity| {
                u64::try_from(capacity.len()).map_or(true, |capacity| range.minimum() <= capacity)
            })
        };
        let child_names = children
            .iter()
            .map(|child| child.name.clone())
            .collect::<Vec<_>>();
        if !fits(
            required,
            dynamic.is_none().then_some(child_names.as_slice()),
        ) {
            return false;
        }
        alternatives.iter().all(|alternative| {
            let mut required = required.clone();
            for field in &alternative.required {
                if !required.contains(field) {
                    required.push(field.clone());
                }
            }
            fits(&required, Some(&alternative.members))
        })
    }

    /// Recursively checks JSON object property-count placement and
    /// feasibility for one schema tree.
    pub fn property_count_range_tree_is_valid(&self) -> bool {
        self.property_count_range_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::property_count_range_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::property_count_range_tree_is_valid)
                }
            }
    }

    /// Checks that JSON property dependencies belong to an object and that
    /// every unconditionally implied property remains possible.
    pub fn json_property_dependencies_are_valid(&self) -> bool {
        let Some(dependencies) = &self.json_property_dependencies else {
            return true;
        };
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } = &self.kind
        else {
            return false;
        };
        let ordinary_allowed = dynamic.is_none().then(|| {
            children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<std::collections::BTreeSet<_>>()
        });
        let ordinary_required = effective_required_fields(required, Some(dependencies));
        if ordinary_allowed.as_ref().is_some_and(|allowed| {
            ordinary_required
                .iter()
                .any(|required| !allowed.contains(required.as_str()))
        }) {
            return false;
        }
        alternatives.iter().all(|alternative| {
            let mut initial = required.clone();
            for field in &alternative.required {
                if !initial.contains(field) {
                    initial.push(field.clone());
                }
            }
            let effective = effective_required_fields(&initial, Some(dependencies));
            effective
                .iter()
                .all(|required| alternative.members.contains(required))
        })
    }

    /// Recursively checks JSON property-dependency placement and feasibility.
    pub fn json_property_dependencies_tree_is_valid(&self) -> bool {
        self.json_property_dependencies_are_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_property_dependencies_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_property_dependencies_tree_is_valid)
                }
            }
    }

    /// Checks that JSON `uniqueItems` metadata remains attached to an array
    /// wrapper rather than its item shape.
    pub fn json_unique_items_is_valid(&self) -> bool {
        !self.json_unique_items || self.repeating
    }

    /// Recursively checks JSON `uniqueItems` placement for one schema tree.
    pub fn json_unique_items_tree_is_valid(&self) -> bool {
        self.json_unique_items_is_valid()
            && match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .all(SchemaNode::json_unique_items_tree_is_valid)
                        && dynamic
                            .as_deref()
                            .is_none_or(SchemaNode::json_unique_items_tree_is_valid)
                }
            }
    }

    /// Checks that string-length metadata matches a string-capable scalar
    /// domain and that an optional fixed lexical value lies inside the interval.
    pub fn string_length_range_is_valid(&self) -> bool {
        let Some(range) = self.string_length_range else {
            return true;
        };
        !self.json_any
            && self.accepts_scalar_type(ScalarType::String)
            && self
                .fixed
                .as_deref()
                .is_none_or(|fixed| range.contains_str(fixed))
    }

    /// Checks that pattern constraints describe a string-capable scalar domain.
    pub fn json_patterns_are_valid(&self) -> bool {
        if self.json_patterns.is_none() {
            return true;
        }
        !self.json_any && self.accepts_scalar_type(ScalarType::String)
    }

    /// Checks one optional fixed lexical value against this node's pattern DNF.
    ///
    /// Whole-schema validation uses [`Self::json_pattern_budget_is_valid`] so
    /// fixed values across descendants share one work budget.
    pub fn json_pattern_fixed_value_is_valid(&self) -> bool {
        self.json_patterns_are_valid()
            && self.fixed.as_deref().is_none_or(|fixed| {
                self.json_patterns
                    .as_ref()
                    .is_none_or(|patterns| patterns.matches(fixed))
            })
    }

    /// Validates the aggregate pattern-program budget for one complete schema.
    ///
    /// Exact sources shared by multiple nodes count once. This is intentionally
    /// separate from [`Self::metadata_is_valid`] so callers can validate one
    /// root in linear time rather than rescanning descendants at every node.
    pub fn json_pattern_budget_is_valid(&self) -> bool {
        let mut programs = std::collections::BTreeMap::new();
        let mut source_bytes = 0_usize;
        let mut instructions = 0_usize;
        let mut fixed_work = json_pattern::DEFAULT_MATCH_WORK_LIMIT;
        self.accumulate_json_pattern_budget(
            &mut programs,
            &mut source_bytes,
            &mut instructions,
            &mut fixed_work,
        )
    }

    fn accumulate_json_pattern_budget<'a>(
        &'a self,
        programs: &mut std::collections::BTreeMap<&'a str, json_pattern::PortableJsonPattern>,
        source_bytes: &mut usize,
        instructions: &mut usize,
        fixed_work: &mut u64,
    ) -> bool {
        if !self.json_patterns_are_valid() {
            return false;
        }
        if let Some(patterns) = &self.json_patterns {
            for source in patterns.any_of().iter().flatten().map(String::as_str) {
                if programs.contains_key(source) {
                    continue;
                }
                if programs.len() == MAX_DISTINCT_JSON_PATTERNS {
                    return false;
                }
                let Some(next_source_bytes) = source_bytes.checked_add(source.len()) else {
                    return false;
                };
                if next_source_bytes > MAX_JSON_PATTERN_SOURCE_BYTES {
                    return false;
                }
                let Ok(compiled) = json_pattern::PortableJsonPattern::compile(source) else {
                    return false;
                };
                let Some(next_instructions) =
                    instructions.checked_add(compiled.instruction_count())
                else {
                    return false;
                };
                if next_instructions > MAX_JSON_PATTERN_INSTRUCTIONS {
                    return false;
                }
                programs.insert(source, compiled);
                *source_bytes = next_source_bytes;
                *instructions = next_instructions;
            }
            if let Some(fixed) = self.fixed.as_deref() {
                let mut matched = false;
                for alternative in patterns.any_of() {
                    let mut alternative_matched = true;
                    for source in alternative {
                        let Some(program) = programs.get(source.as_str()) else {
                            return false;
                        };
                        match program.is_match_with_budget(fixed, fixed_work) {
                            Ok(true) => {}
                            Ok(false) => {
                                alternative_matched = false;
                                break;
                            }
                            Err(_) => return false,
                        }
                    }
                    if alternative_matched {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return false;
                }
            }
        }

        match &self.kind {
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
            SchemaKind::Group {
                children, dynamic, ..
            } => {
                children.iter().all(|child| {
                    child.accumulate_json_pattern_budget(
                        programs,
                        source_bytes,
                        instructions,
                        fixed_work,
                    )
                }) && dynamic.as_deref().is_none_or(|child| {
                    child.accumulate_json_pattern_budget(
                        programs,
                        source_bytes,
                        instructions,
                        fixed_work,
                    )
                })
            }
        }
    }

    /// Checks that JSON format annotations describe a string-capable scalar
    /// value or each scalar item of a repeating node.
    pub fn json_formats_are_valid(&self) -> bool {
        self.json_formats.is_empty()
            || (!self.json_any && self.accepts_scalar_type(ScalarType::String))
    }

    /// Checks that generated-value metadata remains scalar-only and cannot
    /// conflict with repetition or a fixed literal.
    pub fn value_generation_is_valid(&self) -> bool {
        self.value_generation.is_none()
            || (!self.repeating
                && self.fixed.is_none()
                && self.default.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Checks that XML default metadata remains non-repeating, scalar, and
    /// mutually exclusive with fixed and generated values.
    pub fn default_is_valid(&self) -> bool {
        self.default.is_none()
            || (!self.repeating
                && self.fixed.is_none()
                && self.value_generation.is_none()
                && matches!(self.kind, SchemaKind::Scalar { .. }))
    }

    /// Checks that explicit JSON nullability remains scalar-only.
    pub fn nullable_is_valid(&self) -> bool {
        (!self.nullable || self.is_scalar()) && self.json_allowed_values_are_valid()
    }

    /// Checks that JSON container nullability belongs to an object or array.
    pub fn container_nullable_is_valid(&self) -> bool {
        !self.container_nullable || self.repeating || matches!(self.kind, SchemaKind::Group { .. })
    }

    /// Checks that arbitrary JSON values use the graph's canonical string domain.
    pub fn json_any_is_valid(&self) -> bool {
        !self.json_any
            || (!self.repeating
                && !self.attribute
                && !self.text
                && !self.nullable
                && !self.container_nullable
                && self.fixed.is_none()
                && self.json_allowed_values.is_none()
                && self.numeric_range.is_none()
                && self.json_multiple_of.is_none()
                && self.item_count_range.is_none()
                && !self.json_unique_items
                && self.string_length_range.is_none()
                && self.json_patterns.is_none()
                && self.json_formats.is_empty()
                && self.default.is_none()
                && self.value_generation.is_none()
                && matches!(
                    self.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                ))
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
        (self.value_generation_is_valid() && self.json_any_is_valid()).then_some(self)
    }

    /// Declares a homogeneous computed-field value schema for this group.
    /// Object alternatives and open fields are intentionally exclusive: an
    /// open object cannot be matched to one closed alternative exactly.
    pub fn with_dynamic_fields(mut self, value: SchemaNode) -> Option<Self> {
        self.set_dynamic_fields(Some(value)).then_some(self)
    }

    pub fn set_dynamic_fields(&mut self, value: Option<SchemaNode>) -> bool {
        let previous = {
            let SchemaKind::Group {
                children,
                alternatives,
                required,
                dynamic,
                ..
            } = &mut self.kind
            else {
                return false;
            };
            if value.is_some() && !alternatives.is_empty() {
                return false;
            }
            if value.is_none()
                && required
                    .iter()
                    .any(|name| !children.iter().any(|child| child.name == *name))
            {
                return false;
            }
            std::mem::replace(dynamic, value.map(Box::new))
        };
        if self.property_count_range_is_valid() && self.json_property_dependencies_are_valid() {
            true
        } else {
            let SchemaKind::Group { dynamic, .. } = &mut self.kind else {
                return false;
            };
            *dynamic = previous;
            false
        }
    }

    pub fn dynamic_fields(&self) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { dynamic, .. } => dynamic.as_deref(),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
        }
    }

    /// Attaches exact JSON object-property presence requirements.
    pub fn with_required_fields(mut self, required: Vec<String>) -> Option<Self> {
        self.set_required_fields(required).then_some(self)
    }

    pub fn set_required_fields(&mut self, required: Vec<String>) -> bool {
        let previous = {
            let SchemaKind::Group {
                children,
                required: target,
                dynamic,
                ..
            } = &mut self.kind
            else {
                return false;
            };
            if !valid_required_fields(children, dynamic.as_deref(), &required) {
                return false;
            }
            std::mem::replace(target, required)
        };
        if self.property_count_range_is_valid() && self.json_property_dependencies_are_valid() {
            true
        } else {
            let SchemaKind::Group {
                required: target, ..
            } = &mut self.kind
            else {
                return false;
            };
            *target = previous;
            false
        }
    }

    pub fn required_fields(&self) -> &[String] {
        match &self.kind {
            SchemaKind::Group { required, .. } => required,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => &[],
        }
    }

    /// Checks ordinary JSON object-property presence metadata.
    pub fn required_fields_are_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group {
                children,
                required,
                dynamic,
                ..
            } => valid_required_fields(children, dynamic.as_deref(), required),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
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
        let (previous, previous_restricted) = {
            let SchemaKind::Group {
                children,
                alternatives: target,
                required: _,
                xml_restricted_alternatives,
                dynamic,
            } = &mut self.kind
            else {
                return false;
            };
            if dynamic.is_some() || !valid_group_alternatives(children, &alternatives) {
                return false;
            }
            (
                std::mem::replace(target, alternatives),
                std::mem::take(xml_restricted_alternatives),
            )
        };
        let previous_mode = std::mem::replace(&mut self.alternative_mode, mode);
        let previous_xml_kind = std::mem::replace(&mut self.xml_alternative_kind, xml_kind);
        if self.property_count_range_is_valid() && self.json_property_dependencies_are_valid() {
            true
        } else {
            let SchemaKind::Group {
                alternatives,
                xml_restricted_alternatives,
                ..
            } = &mut self.kind
            else {
                return false;
            };
            *alternatives = previous;
            *xml_restricted_alternatives = previous_restricted;
            self.alternative_mode = previous_mode;
            self.xml_alternative_kind = previous_xml_kind;
            false
        }
    }

    pub fn set_xml_restricted_alternatives(&mut self, restricted: Vec<String>) -> bool {
        let SchemaKind::Group {
            alternatives,
            xml_restricted_alternatives,
            ..
        } = &mut self.kind
        else {
            return false;
        };
        if !self.alternative_mode.is_exclusive()
            || !self.xml_alternative_kind.is_xsi_type()
            || restricted.iter().enumerate().any(|(index, name)| {
                restricted[..index].contains(name)
                    || !alternatives
                        .iter()
                        .any(|alternative| alternative.name == *name)
            })
        {
            return false;
        }
        *xml_restricted_alternatives = restricted;
        true
    }

    pub fn xml_restricted_alternatives(&self) -> &[String] {
        match &self.kind {
            SchemaKind::Group {
                xml_restricted_alternatives,
                ..
            } => xml_restricted_alternatives,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => &[],
        }
    }

    /// Checks metadata that may have entered through direct deserialization.
    pub fn alternatives_are_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group {
                children,
                alternatives,
                required: _,
                xml_restricted_alternatives,
                dynamic,
            } => {
                (alternatives.is_empty() || dynamic.is_none())
                    && (alternatives.is_empty() || valid_group_alternatives(children, alternatives))
                    && xml_restricted_alternatives
                        .iter()
                        .enumerate()
                        .all(|(index, name)| {
                            !xml_restricted_alternatives[..index].contains(name)
                                && alternatives
                                    .iter()
                                    .any(|alternative| alternative.name == *name)
                        })
                    && (xml_restricted_alternatives.is_empty()
                        || self.alternative_mode.is_exclusive()
                            && self.xml_alternative_kind.is_xsi_type())
            }
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => true,
        }
    }

    /// Checks that inclusive semantics cannot exist without group
    /// alternatives or leak onto scalar nodes.
    pub fn alternative_mode_is_valid(&self) -> bool {
        match &self.kind {
            SchemaKind::Group { alternatives, .. } => {
                !alternatives.is_empty() || self.alternative_mode.is_exclusive()
            }
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => {
                self.alternative_mode.is_exclusive()
            }
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

    pub fn xml_repeating_choices_are_valid(&self) -> bool {
        let SchemaKind::Group { children, .. } = &self.kind else {
            return self.xml_repeating_choices.is_empty();
        };
        let sequence_members = self
            .xml_repeating_sequences
            .iter()
            .flat_map(|sequence| sequence.members.iter().map(|member| member.name.as_str()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut used = std::collections::BTreeSet::new();
        self.xml_repeating_choices.iter().all(|choice| {
            let positions = choice
                .members
                .iter()
                .map(|member| {
                    let mut matches = children.iter().enumerate().filter(|(_, child)| {
                        child.name == *member
                            && child.repeating == choice.repeating
                            && !child.attribute
                            && !child.text
                    });
                    let position = matches.next().map(|(position, _)| position)?;
                    matches.next().is_none().then_some(position)
                })
                .collect::<Option<Vec<_>>>();
            choice.members.len() > 1
                && choice.members.iter().all(|member| {
                    !member.is_empty()
                        && !sequence_members.contains(member.as_str())
                        && used.insert(member.as_str())
                })
                && positions.is_some_and(|positions| {
                    positions.windows(2).all(|pair| pair[1] == pair[0] + 1)
                })
        })
    }

    pub fn set_xml_repeating_choices(&mut self, choices: Vec<XmlRepeatingChoice>) -> bool {
        let previous = std::mem::replace(&mut self.xml_repeating_choices, choices);
        if self.xml_repeating_choices_are_valid() {
            true
        } else {
            self.xml_repeating_choices = previous;
            false
        }
    }

    pub fn alternatives(&self) -> &[GroupAlternative] {
        match &self.kind {
            SchemaKind::Group { alternatives, .. } => alternatives,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => &[],
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
        (self.nullable_is_valid()
            && self.json_allowed_values_are_valid()
            && self.json_any_is_valid())
        .then_some(self)
    }

    /// Marks this JSON object or array wrapper as accepting explicit `null`.
    pub fn nullable_container(mut self) -> Option<Self> {
        self.container_nullable = true;
        self.container_nullable_is_valid().then_some(self)
    }

    /// Marks this string scalar as the canonical encoding of arbitrary JSON.
    pub fn json_any(mut self) -> Option<Self> {
        self.json_any = true;
        self.json_any_is_valid().then_some(self)
    }

    /// Requires this scalar to hold `value` (builder-style).
    pub fn with_fixed(mut self, value: impl Into<String>) -> Option<Self> {
        self.fixed = Some(value.into());
        (self.fixed_is_valid()
            && self.json_allowed_values_are_valid()
            && self.numeric_range_is_valid()
            && self.json_multiple_of_is_valid()
            && self.string_length_range_is_valid()
            && self.json_pattern_fixed_value_is_valid()
            && self.default_is_valid()
            && self.value_generation_is_valid()
            && self.json_any_is_valid())
        .then_some(self)
    }

    /// Restricts this JSON scalar to one canonical finite value set.
    ///
    /// Nullability is derived from the set so an allowed `null` cannot drift
    /// out of sync with the scalar boundary contract.
    pub fn with_json_allowed_values(mut self, values: JsonAllowedValues) -> Option<Self> {
        self.nullable = values.contains_json_null();
        self.json_allowed_values = Some(values);
        (self.fixed_is_valid() && self.json_allowed_values_are_valid() && self.json_any_is_valid())
            .then_some(self)
    }

    pub fn with_numeric_range(mut self, range: NumericRange) -> Option<Self> {
        self.numeric_range = Some(range);
        self.numeric_range_is_valid().then_some(self)
    }

    pub fn with_json_multiple_of(mut self, constraints: JsonMultipleOfConstraints) -> Option<Self> {
        self.json_multiple_of = Some(constraints);
        self.json_multiple_of_is_valid().then_some(self)
    }

    pub fn with_item_count_range(mut self, range: ItemCountRange) -> Option<Self> {
        self.item_count_range = Some(range);
        self.item_count_range_is_valid().then_some(self)
    }

    pub fn with_property_count_range(mut self, range: PropertyCountRange) -> Option<Self> {
        self.property_count_range = Some(range);
        (self.property_count_range_is_valid() && self.json_property_dependencies_are_valid())
            .then_some(self)
    }

    pub fn with_json_property_dependencies(
        mut self,
        dependencies: JsonPropertyDependencies,
    ) -> Option<Self> {
        self.json_property_dependencies = Some(dependencies);
        (self.json_property_dependencies_are_valid() && self.property_count_range_is_valid())
            .then_some(self)
    }

    /// Requires pairwise-distinct JSON array items under JSON Schema equality.
    pub fn with_json_unique_items(mut self) -> Option<Self> {
        self.json_unique_items = true;
        self.json_unique_items_is_valid().then_some(self)
    }

    pub fn with_string_length_range(mut self, range: StringLengthRange) -> Option<Self> {
        self.string_length_range = Some(range);
        self.string_length_range_is_valid().then_some(self)
    }

    pub fn with_json_patterns(mut self, patterns: JsonPatternConstraints) -> Option<Self> {
        self.json_patterns = Some(patterns);
        self.json_pattern_fixed_value_is_valid().then_some(self)
    }

    pub fn with_json_formats(mut self, formats: JsonFormatAnnotations) -> Option<Self> {
        self.json_formats = formats;
        self.json_formats_are_valid().then_some(self)
    }

    pub fn with_default(mut self, value: impl Into<String>) -> Option<Self> {
        self.default = Some(value.into());
        (self.default_is_valid() && self.json_any_is_valid()).then_some(self)
    }

    pub fn child(&self, name: &str) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children, .. } => children.iter().find(|c| c.name == name),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
        }
    }

    pub fn text_child(&self) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children, .. } => children.iter().find(|child| child.text),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
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
                            && alternative.members.contains(&constraint.member)
                            && children.iter().any(|child| {
                                child.name == constraint.member
                                    && !child.repeating
                                    && match child.kind {
                                        SchemaKind::Scalar { ty } => {
                                            constraint.value.is_valid_for(ty, child.nullable)
                                        }
                                        SchemaKind::ScalarUnion { types } => {
                                            constraint
                                                .value
                                                .scalar_type()
                                                .is_some_and(|ty| types.contains(ty))
                                                || matches!(
                                                    constraint.value,
                                                    GroupAlternativeConstraintValue::JsonNull
                                                ) && child.nullable
                                        }
                                        SchemaKind::Group { .. } => false,
                                    }
                            })
                    },
                )
        })
}

fn valid_required_fields(
    children: &[SchemaNode],
    dynamic: Option<&SchemaNode>,
    required: &[String],
) -> bool {
    required.iter().enumerate().all(|(index, name)| {
        !name.is_empty()
            && !required[..index].contains(name)
            && (dynamic.is_some() || children.iter().any(|child| child.name == *name))
    })
}

fn effective_required_fields(
    required: &[String],
    dependencies: Option<&JsonPropertyDependencies>,
) -> Vec<String> {
    let mut effective = required
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let Some(dependencies) = dependencies else {
        return effective.into_iter().collect();
    };
    loop {
        let previous_len = effective.len();
        for (trigger, requirements) in dependencies.rules() {
            if effective.contains(trigger) {
                effective.extend(requirements.iter().cloned());
            }
        }
        if effective.len() == previous_len {
            return effective.into_iter().collect();
        }
    }
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
    fn scalar_union_types_are_canonical_validated_and_backward_compatible()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(types) =
            ScalarTypeSet::new([ScalarType::Bool, ScalarType::String, ScalarType::Int])
        else {
            panic!("test scalar union must contain distinct types");
        };
        assert!(types.contains(ScalarType::String));
        assert!(types.contains(ScalarType::Int));
        assert!(types.contains(ScalarType::Bool));
        assert!(!types.contains(ScalarType::Float));
        assert_eq!(
            types.iter().collect::<Vec<_>>(),
            vec![ScalarType::String, ScalarType::Int, ScalarType::Bool]
        );
        assert!(ScalarTypeSet::new([ScalarType::String]).is_none());
        assert!(ScalarTypeSet::new([ScalarType::String, ScalarType::String]).is_none());

        let union = SchemaNode::scalar_union("value", types);
        assert!(union.is_scalar());
        assert!(union.accepts_scalar_type(ScalarType::Bool));
        assert!(!union.accepts_scalar_type(ScalarType::Float));
        assert!(union.clone().with_fixed("ready").is_none());
        let encoded = serde_json::to_string(&union)?;
        assert!(
            encoded.contains(r#""kind":{"kind":"scalar_union","types":["string","int","bool"]}"#)
        );
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, union);

        let legacy: SchemaNode =
            serde_json::from_str(r#"{"name":"value","kind":{"kind":"scalar","ty":"string"}}"#)?;
        assert_eq!(
            legacy.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        );
        for invalid in [
            r#"{"name":"value","kind":{"kind":"scalar_union","types":[]}}"#,
            r#"{"name":"value","kind":{"kind":"scalar_union","types":["string"]}}"#,
            r#"{"name":"value","kind":{"kind":"scalar_union","types":["string","string"]}}"#,
            r#"{"name":"value","fixed":"ready","kind":{"kind":"scalar_union","types":["string","bool"]}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        let Some(fixed) = SchemaNode::scalar("value", ScalarType::String).with_fixed("ready")
        else {
            panic!("ordinary scalar fixed metadata should remain valid");
        };
        assert!(fixed.fixed_is_valid());
        assert_eq!(fixed.fixed.as_deref(), Some("ready"));
        assert!(
            SchemaNode::group("value", Vec::new())
                .with_fixed("ready")
                .is_none()
        );
        assert!(
            SchemaNode::scalar("value", ScalarType::String)
                .with_default("ready")
                .is_some_and(|node| node.with_fixed("ready").is_none())
        );

        let discriminator = SchemaNode::scalar_union("kind", types);
        let alternatives = vec![
            GroupAlternative {
                name: "text".into(),
                members: vec!["kind".into()],
                required: vec!["kind".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: GroupAlternativeConstraintValue::String("ready".into()),
                }],
            },
            GroupAlternative {
                name: "numeric".into(),
                members: vec!["kind".into()],
                required: vec!["kind".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: GroupAlternativeConstraintValue::Int(7),
                }],
            },
        ];
        assert!(
            SchemaNode::group("event", vec![discriminator.clone()])
                .with_alternatives(alternatives.clone())
                .is_some()
        );
        let mut invalid = alternatives;
        let Some(value) = FiniteF64::new(7.5) else {
            panic!("test float should be finite");
        };
        invalid[1].constraints[0].value = GroupAlternativeConstraintValue::Float(value);
        assert!(
            SchemaNode::group("event", vec![discriminator])
                .with_alternatives(invalid)
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn numeric_ranges_are_typed_nonempty_and_serde_validated()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(integer) = IntegerRange::new(Some(-4), Some(9)) else {
            panic!("ordered integer range is valid");
        };
        assert!(integer.contains(-4));
        assert!(integer.contains(9));
        assert!(!integer.contains(10));
        assert!(IntegerRange::new(None, None).is_none());
        assert!(IntegerRange::new(Some(2), Some(1)).is_none());

        let Some(zero) = FiniteF64::new(0.0) else {
            panic!("zero is finite");
        };
        let Some(ten) = FiniteF64::new(10.0) else {
            panic!("ten is finite");
        };
        let Some(number) = NumberRange::new(
            Some(NumberBound::exclusive(zero)),
            Some(NumberBound::inclusive(ten)),
        ) else {
            panic!("ordered number range is valid");
        };
        assert!(!number.contains(0.0));
        assert!(number.contains(0.5));
        assert!(number.contains(10.0));
        assert!(
            NumberRange::new(
                Some(NumberBound::exclusive(zero)),
                Some(NumberBound::inclusive(zero)),
            )
            .is_none()
        );

        let Some(schema) = SchemaNode::scalar_fixed("Count", ScalarType::Int, "7")
            .with_numeric_range(NumericRange::Integer(integer))
        else {
            panic!("fixed integer inside its range is valid");
        };
        let encoded = serde_json::to_string(&schema)?;
        assert!(
            encoded.contains(
                r#""numeric_range":{"kind":"integer","bounds":{"minimum":-4,"maximum":9}}"#
            )
        );
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
        assert!(
            SchemaNode::scalar_fixed("Count", ScalarType::Int, "10")
                .with_numeric_range(NumericRange::Integer(integer))
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Count", ScalarType::String)
                .with_numeric_range(NumericRange::Integer(integer))
                .is_none()
        );

        for invalid in [
            r#"{"name":"x","numeric_range":{"kind":"integer","bounds":{}},"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":1}},"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","numeric_range":{"kind":"integer","bounds":{"minimum":1}},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","fixed":"0","numeric_range":{"kind":"integer","bounds":{"minimum":1}},"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.0,"exclusive":true},"maximum":{"value":1.0}}},"kind":{"kind":"scalar","ty":"float"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn json_multiple_of_constraints_are_typed_exact_and_serde_validated()
    -> Result<(), Box<dyn std::error::Error>> {
        let divisor = JsonMultipleOf::from_decimal_lexical("2.5")
            .ok_or("test multipleOf divisor is representable")?;
        let constraints = JsonMultipleOfConstraints::new([[divisor]])?;

        let integer = SchemaNode::scalar("Count", ScalarType::Int)
            .with_json_multiple_of(constraints.clone())
            .ok_or("integer scalar accepts multipleOf")?;
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&integer)?)?,
            integer
        );
        assert!(
            SchemaNode::scalar_fixed("Count", ScalarType::Int, "5")
                .with_json_multiple_of(constraints.clone())
                .is_some()
        );
        assert!(
            SchemaNode::scalar_fixed("Count", ScalarType::Int, "6")
                .with_json_multiple_of(constraints.clone())
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Text", ScalarType::String)
                .with_json_multiple_of(constraints.clone())
                .is_none()
        );
        assert!(
            SchemaNode::group("Record", Vec::new())
                .with_json_multiple_of(constraints.clone())
                .is_none()
        );
        let arbitrary = SchemaNode::scalar("Any", ScalarType::String)
            .json_any()
            .ok_or("plain arbitrary JSON scalar is valid")?;
        assert!(
            arbitrary
                .with_json_multiple_of(constraints.clone())
                .is_none()
        );

        let types = ScalarTypeSet::new([ScalarType::String, ScalarType::Int])
            .ok_or("test scalar union is heterogeneous")?;
        assert!(
            SchemaNode::scalar_union("Value", types)
                .with_json_multiple_of(constraints)
                .is_some()
        );
        for invalid in [
            r#"{"name":"x","json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","fixed":"3","json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","json_any":true,"json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"string"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn json_allowed_values_are_canonical_typed_and_serde_validated()
    -> Result<(), Box<dyn std::error::Error>> {
        let values = JsonAllowedValues::new([
            JsonAllowedValue::String("ready".to_string()),
            JsonAllowedValue::JsonNull,
            JsonAllowedValue::String("pending".to_string()),
        ])?;
        let schema = SchemaNode::scalar("Status", ScalarType::String)
            .with_json_allowed_values(values.clone())
            .ok_or("string enum values match the scalar domain")?;
        assert!(schema.nullable);
        assert!(schema.json_allowed_values_are_valid());
        assert!(schema.json_allowed_values_tree_is_valid());
        let encoded = serde_json::to_string(&schema)?;
        assert!(encoded.contains(
            r#""json_allowed_values":[{"type":"json_null"},{"type":"string","value":"pending"},{"type":"string","value":"ready"}]"#
        ));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

        let numeric_values = JsonAllowedValues::new([
            JsonAllowedValue::Int(7),
            JsonAllowedValue::Float(FiniteF64::new(7.5).ok_or("test enum number must be finite")?),
        ])?;
        let numeric = SchemaNode::scalar("Amount", ScalarType::Float)
            .with_json_allowed_values(numeric_values)
            .ok_or("number enums admit exact integer values")?;
        assert!(!numeric.nullable);
        assert!(numeric.json_allowed_values_are_valid());
        assert!(numeric.clone().repeating().json_allowed_values_are_valid());

        let mixed_values = JsonAllowedValues::new([
            JsonAllowedValue::Int(1),
            JsonAllowedValue::String("one".to_string()),
        ])?;
        let mixed_types = ScalarTypeSet::new([ScalarType::String, ScalarType::Float])
            .ok_or("test scalar enum union has distinct types")?;
        assert!(
            SchemaNode::scalar_union("Mixed", mixed_types)
                .with_json_allowed_values(mixed_values.clone())
                .is_some()
        );
        assert!(
            SchemaNode::scalar("Text", ScalarType::String)
                .with_json_allowed_values(mixed_values)
                .is_none()
        );

        assert!(schema.clone().with_fixed("ready").is_none());
        assert!(
            SchemaNode::scalar_fixed("Status", ScalarType::String, "ready")
                .with_json_allowed_values(values.clone())
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Status", ScalarType::String)
                .json_any()
                .and_then(|schema| schema.with_json_allowed_values(values.clone()))
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Status", ScalarType::String)
                .with_json_allowed_values(values.clone())
                .and_then(SchemaNode::json_any)
                .is_none()
        );

        let mut nested = SchemaNode::group("Root", vec![schema.clone()]);
        assert!(nested.json_allowed_values_tree_is_valid());
        let SchemaKind::Group { children, .. } = &mut nested.kind else {
            return Err("test root must be a group".into());
        };
        let Some(child) = children.iter_mut().find(|child| child.name == "Status") else {
            return Err("test child must exist".into());
        };
        child.nullable = false;
        assert!(!nested.json_allowed_values_tree_is_valid());

        for invalid in [
            r#"{"name":"x","json_allowed_values":[{"type":"json_null"},{"type":"string","value":"a"}],"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","nullable":true,"json_allowed_values":[{"type":"string","value":"a"},{"type":"string","value":"b"}],"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","fixed":"a","json_allowed_values":[{"type":"string","value":"a"},{"type":"string","value":"b"}],"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","json_allowed_values":[{"type":"float","value":1.5},{"type":"float","value":2.5}],"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","json_allowed_values":[{"type":"bool","value":false},{"type":"bool","value":true}],"kind":{"kind":"group","children":[]}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn item_count_ranges_require_repeating_nodes_and_roundtrip()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(range) = ItemCountRange::new(2, Some(5)) else {
            panic!("ordered item-count range is valid");
        };
        assert!(range.contains_len(2));
        assert!(range.contains_len(5));
        assert!(!range.contains_len(1));
        assert!(ItemCountRange::new(0, None).is_none());
        assert!(ItemCountRange::new(3, Some(2)).is_none());

        let Some(schema) = SchemaNode::scalar("Item", ScalarType::String)
            .repeating()
            .with_item_count_range(range)
        else {
            panic!("item-count metadata is valid on a repeating node");
        };
        let encoded = serde_json::to_string(&schema)?;
        assert!(encoded.contains(r#""item_count_range":{"minimum":2,"maximum":5}"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
        assert!(
            SchemaNode::scalar("Item", ScalarType::String)
                .with_item_count_range(range)
                .is_none()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"x","item_count_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"x","repeating":true,"item_count_range":{},"kind":{"kind":"scalar","ty":"string"}}"#,
            )
            .is_err()
        );
        for invalid in [
            r#"{"name":"x","repeating":true,"item_count_range":{"minimum":-1},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","repeating":true,"item_count_range":{"maximum":1.5},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","repeating":true,"item_count_range":{"minimum":1,"maxmium":3},"kind":{"kind":"scalar","ty":"string"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        let permissive_null_maximum = r#"{"name":"x","repeating":true,"item_count_range":{"minimum":1,"maximum":null},"kind":{"kind":"scalar","ty":"string"}}"#;
        assert!(serde_json::from_str::<SchemaNode>(permissive_null_maximum).is_ok());
        Ok(())
    }

    #[test]
    fn property_count_ranges_require_feasible_groups_and_roundtrip()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(range) = PropertyCountRange::new(1, Some(2)) else {
            panic!("ordered property-count range is valid");
        };
        assert!(range.contains_len(1));
        assert!(range.contains_len(2));
        assert!(!range.contains_len(0));
        assert!(PropertyCountRange::new(0, None).is_none());
        assert!(PropertyCountRange::new(3, Some(2)).is_none());

        let Some(schema) = SchemaNode::group(
            "Object",
            vec![
                SchemaNode::scalar("first", ScalarType::String),
                SchemaNode::scalar("second", ScalarType::String),
            ],
        )
        .with_required_fields(vec!["first".into()])
        .and_then(|schema| schema.with_property_count_range(range)) else {
            panic!("property-count metadata is feasible on the test group");
        };
        let encoded = serde_json::to_string(&schema)?;
        assert!(encoded.contains(r#""property_count_range":{"minimum":1,"maximum":2}"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

        assert!(
            SchemaNode::scalar("value", ScalarType::String)
                .with_property_count_range(range)
                .is_none()
        );
        let Some(at_least_three) = PropertyCountRange::new(3, None) else {
            panic!("positive lower bound is constrained");
        };
        assert!(
            SchemaNode::group(
                "closed",
                vec![
                    SchemaNode::scalar("first", ScalarType::String),
                    SchemaNode::scalar("second", ScalarType::String),
                ],
            )
            .with_property_count_range(at_least_three)
            .is_none()
        );
        let Some(at_most_one) = PropertyCountRange::new(0, Some(1)) else {
            panic!("finite upper bound is constrained");
        };
        assert!(
            SchemaNode::group(
                "required",
                vec![
                    SchemaNode::scalar("first", ScalarType::String),
                    SchemaNode::scalar("second", ScalarType::String),
                ],
            )
            .with_required_fields(vec!["first".into(), "second".into()])
            .and_then(|schema| schema.with_property_count_range(at_most_one))
            .is_none()
        );

        for invalid in [
            r#"{"name":"x","property_count_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","property_count_range":{},"kind":{"kind":"group","children":[]}}"#,
            r#"{"name":"x","property_count_range":{"minimum":-1},"kind":{"kind":"group","children":[]}}"#,
            r#"{"name":"x","property_count_range":{"maximum":1.5},"kind":{"kind":"group","children":[]}}"#,
            r#"{"name":"x","property_count_range":{"minimum":1,"maxmium":3},"kind":{"kind":"group","children":[]}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn property_dependencies_are_group_scoped_feasible_and_transactional()
    -> Result<(), Box<dyn std::error::Error>> {
        let dependencies = JsonPropertyDependencies::new(std::collections::BTreeMap::from([
            ("a".into(), vec!["b".into()]),
            ("b".into(), vec!["c".into()]),
        ]))?;
        let schema = SchemaNode::group(
            "Object",
            vec![
                SchemaNode::scalar("a", ScalarType::String),
                SchemaNode::scalar("b", ScalarType::String),
                SchemaNode::scalar("c", ScalarType::String),
            ],
        )
        .with_json_property_dependencies(dependencies.clone())
        .ok_or("dependency rules fit the closed object")?;
        let encoded = serde_json::to_string(&schema)?;
        assert!(encoded.contains(r#""json_property_dependencies":{"a":["b"],"b":["c"]}"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);
        assert!(
            SchemaNode::scalar("value", ScalarType::String)
                .with_json_property_dependencies(dependencies.clone())
                .is_none()
        );

        let maximum_two =
            PropertyCountRange::new(0, Some(2)).ok_or("finite property maximum is valid")?;
        assert!(
            schema
                .clone()
                .with_required_fields(vec!["a".into()])
                .and_then(|schema| schema.with_property_count_range(maximum_two))
                .is_none()
        );
        let mut transactional = schema
            .clone()
            .with_property_count_range(maximum_two)
            .ok_or("optional dependency triggers fit the property maximum")?;
        assert!(!transactional.set_required_fields(vec!["a".into()]));
        assert!(transactional.required_fields().is_empty());

        let open_dependencies =
            JsonPropertyDependencies::new(std::collections::BTreeMap::from([(
                "a".into(),
                vec!["missing".into()],
            )]))?;
        let open = SchemaNode::group("Open", vec![SchemaNode::scalar("a", ScalarType::String)])
            .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
            .and_then(|schema| schema.with_required_fields(vec!["a".into()]))
            .and_then(|schema| schema.with_json_property_dependencies(open_dependencies))
            .ok_or("open objects can satisfy runtime-named dependency targets")?;
        let mut cannot_close = open;
        assert!(!cannot_close.set_dynamic_fields(None));
        assert!(cannot_close.dynamic_fields().is_some());

        let invalid = r#"{"name":"x","json_property_dependencies":{"a":["b"]},"kind":{"kind":"scalar","ty":"string"}}"#;
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        Ok(())
    }

    #[test]
    fn unique_items_require_repeating_nodes_and_roundtrip() -> Result<(), Box<dyn std::error::Error>>
    {
        assert!(
            SchemaNode::scalar("Item", ScalarType::String)
                .with_json_unique_items()
                .is_none()
        );
        let schema = SchemaNode::group("Item", vec![SchemaNode::scalar("value", ScalarType::Int)])
            .repeating()
            .with_json_unique_items()
            .ok_or_else(|| std::io::Error::other("repeating unique-items test schema is valid"))?;
        let encoded = serde_json::to_string(&schema)?;
        assert!(encoded.contains(r#""json_unique_items":true"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, schema);

        let legacy = r#"{"name":"x","repeating":true,"kind":{"kind":"scalar","ty":"string"}}"#;
        assert!(!serde_json::from_str::<SchemaNode>(legacy)?.json_unique_items);
        let invalid =
            r#"{"name":"x","json_unique_items":true,"kind":{"kind":"scalar","ty":"string"}}"#;
        assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        Ok(())
    }

    #[test]
    fn string_length_ranges_require_string_capable_domains_and_roundtrip()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(range) = StringLengthRange::new(1, Some(3)) else {
            panic!("test range is valid");
        };
        assert!(StringLengthRange::new(0, None).is_none());
        assert!(StringLengthRange::new(4, Some(3)).is_none());

        let Some(string) =
            SchemaNode::scalar("Value", ScalarType::String).with_string_length_range(range)
        else {
            panic!("string-length metadata matches a string scalar");
        };
        let encoded = serde_json::to_string(&string)?;
        assert!(encoded.contains(r#""string_length_range":{"minimum":1,"maximum":3}"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, string);

        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test union is valid");
        };
        assert!(
            SchemaNode::scalar_union("Value", types)
                .repeating()
                .with_string_length_range(range)
                .is_some()
        );
        assert!(
            SchemaNode::scalar("Value", ScalarType::Int)
                .with_string_length_range(range)
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Value", ScalarType::String)
                .with_string_length_range(range)
                .and_then(SchemaNode::json_any)
                .is_none()
        );
        assert!(
            SchemaNode::scalar_fixed("Value", ScalarType::String, "")
                .with_string_length_range(range)
                .is_none()
        );

        for invalid in [
            r#"{"name":"x","string_length_range":{"minimum":1},"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","json_any":true,"string_length_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","fixed":"","string_length_range":{"minimum":1},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","string_length_range":{},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","string_length_range":{"minimum":-1},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","string_length_range":{"maximum":1.0},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","string_length_range":{"minimum":1,"maximum":0},"kind":{"kind":"scalar","ty":"string"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn json_pattern_constraints_require_string_capable_domains_and_roundtrip()
    -> Result<(), Box<dyn std::error::Error>> {
        let patterns = JsonPatternConstraints::new([
            ["^A".to_string(), "Z$".to_string()],
            ["^B$".to_string(), "^B$".to_string()],
        ])?;
        let Some(string) =
            SchemaNode::scalar("Value", ScalarType::String).with_json_patterns(patterns.clone())
        else {
            panic!("pattern metadata matches a string scalar");
        };
        let encoded = serde_json::to_string(&string)?;
        assert!(encoded.contains(r#""json_patterns":{"any_of":[["^A","Z$"],["^B$"]]}"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, string);

        assert!(
            SchemaNode::scalar_fixed("Value", ScalarType::String, "ABZ")
                .with_json_patterns(patterns.clone())
                .is_some()
        );
        assert!(
            SchemaNode::scalar_fixed("Value", ScalarType::String, "B")
                .with_json_patterns(patterns.clone())
                .is_some()
        );
        assert!(
            SchemaNode::scalar_fixed("Value", ScalarType::String, "C")
                .with_json_patterns(patterns.clone())
                .is_none()
        );

        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test union is valid");
        };
        assert!(
            SchemaNode::scalar_union("Value", types)
                .repeating()
                .with_json_patterns(patterns.clone())
                .is_some()
        );
        assert!(
            SchemaNode::scalar("Value", ScalarType::Int)
                .with_json_patterns(patterns.clone())
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Value", ScalarType::String)
                .with_json_patterns(patterns)
                .and_then(SchemaNode::json_any)
                .is_none()
        );

        for invalid in [
            r#"{"name":"x","json_patterns":{"any_of":[["A"]]},"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","json_any":true,"json_patterns":{"any_of":[["A"]]},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","json_patterns":{"any_of":[]},"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","json_patterns":{"any_of":[["A","A"]]},"kind":{"kind":"scalar","ty":"string"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        let mismatched: SchemaNode = serde_json::from_str(
            r#"{"name":"x","fixed":"C","json_patterns":{"any_of":[["^A$"]]},"kind":{"kind":"scalar","ty":"string"}}"#,
        )?;
        assert!(!mismatched.json_pattern_budget_is_valid());
        Ok(())
    }

    #[test]
    fn json_pattern_plan_budgets_are_global_deduplicated_and_include_dynamic_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut distinct_children = Vec::new();
        for index in 0..MAX_DISTINCT_JSON_PATTERNS {
            let patterns = JsonPatternConstraints::new([[format!("^value-{index}$")]])?;
            let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
                .with_json_patterns(patterns)
            else {
                panic!("pattern metadata matches a string child");
            };
            distinct_children.push(child);
        }
        let within_budget = SchemaNode::group("Root", distinct_children.clone());
        assert!(within_budget.json_pattern_budget_is_valid());

        let overflow_patterns = JsonPatternConstraints::new([["^overflow$"]])?;
        let Some(overflow) =
            SchemaNode::scalar("*", ScalarType::String).with_json_patterns(overflow_patterns)
        else {
            panic!("overflow pattern metadata is locally valid");
        };
        let Some(with_dynamic_overflow) = within_budget.clone().with_dynamic_fields(overflow)
        else {
            panic!("dynamic field metadata is structurally valid");
        };
        let nested_overflow = SchemaNode::group("Envelope", vec![with_dynamic_overflow]);
        assert!(!nested_overflow.json_pattern_budget_is_valid());

        let mut source_heavy_children = Vec::new();
        for (index, marker) in ['b', 'c', 'd', 'e', 'f'].into_iter().enumerate() {
            let source = format!("[{}{marker}]", "a".repeat(60_000));
            let patterns = JsonPatternConstraints::new([[source]])?;
            let Some(child) = SchemaNode::scalar(format!("source-{index}"), ScalarType::String)
                .with_json_patterns(patterns)
            else {
                panic!("large character-class pattern is locally valid");
            };
            source_heavy_children.push(child);
        }
        assert!(
            !SchemaNode::group("SourceHeavy", source_heavy_children).json_pattern_budget_is_valid()
        );

        let mut instruction_heavy_children = Vec::new();
        for index in 0..14 {
            let source = format!("{}{index}", "a".repeat(5_000));
            let patterns = JsonPatternConstraints::new([[source]])?;
            let Some(child) =
                SchemaNode::scalar(format!("instruction-{index}"), ScalarType::String)
                    .with_json_patterns(patterns)
            else {
                panic!("long literal pattern is locally valid");
            };
            instruction_heavy_children.push(child);
        }
        assert!(
            !SchemaNode::group("InstructionHeavy", instruction_heavy_children)
                .json_pattern_budget_is_valid()
        );

        let repeated_patterns = JsonPatternConstraints::new([["^shared$"]])?;
        let mut repeated_children = Vec::new();
        for index in 0..=MAX_DISTINCT_JSON_PATTERNS {
            let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
                .with_json_patterns(repeated_patterns.clone())
            else {
                panic!("shared pattern metadata matches a string child");
            };
            repeated_children.push(child);
        }
        let Some(shared_dynamic) =
            SchemaNode::scalar("*", ScalarType::String).with_json_patterns(repeated_patterns)
        else {
            panic!("shared dynamic pattern metadata is valid");
        };
        let Some(shared_root) =
            SchemaNode::group("Root", repeated_children).with_dynamic_fields(shared_dynamic)
        else {
            panic!("shared dynamic root is structurally valid");
        };
        assert!(shared_root.json_pattern_budget_is_valid());

        let costly_source = format!("^{}$", "a".repeat(6_000));
        let costly_patterns = JsonPatternConstraints::new([[costly_source]])?;
        let costly_fixed = || {
            SchemaNode::scalar_fixed("fixed", ScalarType::String, "a".repeat(6_000))
                .with_json_patterns(costly_patterns.clone())
                .ok_or("costly fixed pattern remains locally valid")
        };
        let within_fixed_work = SchemaNode::group("Fixed", vec![costly_fixed()?, costly_fixed()?]);
        assert!(within_fixed_work.json_pattern_budget_is_valid());
        let over_fixed_work = SchemaNode::group(
            "Fixed",
            vec![costly_fixed()?, costly_fixed()?, costly_fixed()?],
        );
        assert!(!over_fixed_work.json_pattern_budget_is_valid());
        Ok(())
    }

    #[test]
    fn repeated_expansion_heavy_pattern_metadata_deserializes_before_one_root_compile()
    -> Result<(), Box<dyn std::error::Error>> {
        let patterns = JsonPatternConstraints::new([["a{16000}"]])?;
        let mut children = Vec::new();
        for index in 0..1_024 {
            let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
                .with_json_patterns(patterns.clone())
            else {
                panic!("expansion-heavy pattern metadata matches a string child");
            };
            children.push(child);
        }
        let schema = SchemaNode::group("Root", children);
        let encoded = serde_json::to_string(&schema)?;
        let decoded: SchemaNode = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, schema);
        assert!(decoded.json_pattern_budget_is_valid());
        Ok(())
    }

    #[test]
    fn json_format_annotations_require_string_capable_non_arbitrary_domains()
    -> Result<(), Box<dyn std::error::Error>> {
        let formats =
            JsonFormatAnnotations::new([String::new(), "email".to_string(), "custom".to_string()])?;
        let Some(string) =
            SchemaNode::scalar("Value", ScalarType::String).with_json_formats(formats.clone())
        else {
            panic!("string format metadata is valid");
        };
        assert_eq!(string.json_formats.as_slice(), ["", "email", "custom"]);
        let encoded = serde_json::to_string(&string)?;
        assert!(encoded.contains(r#""json_formats":["","email","custom"]"#));
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, string);
        assert!(
            SchemaNode::scalar("Value", ScalarType::Int)
                .with_json_formats(formats.clone())
                .is_none()
        );

        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test union contains distinct types");
        };
        assert!(
            SchemaNode::scalar_union("Value", types)
                .repeating()
                .with_json_formats(formats.clone())
                .is_some()
        );
        let mut arbitrary = SchemaNode::scalar("Value", ScalarType::String);
        arbitrary.json_any = true;
        arbitrary.json_formats = formats;
        assert!(!arbitrary.metadata_is_valid());

        for invalid in [
            r#"{"name":"x","json_formats":["email"],"kind":{"kind":"scalar","ty":"int"}}"#,
            r#"{"name":"x","json_any":true,"json_formats":["email"],"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","json_formats":["email","email"],"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"x","json_formats":"email","kind":{"kind":"scalar","ty":"string"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
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

        let mut optional = discriminated.alternatives().to_vec();
        optional[0].required.retain(|field| field != "kind");
        let optional = SchemaNode::group(
            "Event",
            vec![
                SchemaNode::scalar("kind", ScalarType::String),
                SchemaNode::scalar("value", ScalarType::String),
            ],
        )
        .with_alternatives(optional)
        .unwrap();
        assert!(
            !optional.alternatives()[0]
                .required
                .iter()
                .any(|field| field == "kind")
        );
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&optional).unwrap()).unwrap(),
            optional
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

        let nullable_discriminator = SchemaNode::group(
            "Nullable",
            vec![
                SchemaNode::scalar("kind", ScalarType::String)
                    .nullable()
                    .unwrap(),
            ],
        )
        .with_alternatives(vec![GroupAlternative {
            name: "missing".into(),
            members: vec!["kind".into()],
            required: Vec::new(),
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::JsonNull,
            }],
        }])
        .unwrap();
        assert_eq!(
            serde_json::from_str::<SchemaNode>(
                &serde_json::to_string(&nullable_discriminator).unwrap()
            )
            .unwrap(),
            nullable_discriminator
        );
        assert!(
            SchemaNode::group(
                "NonNullable",
                vec![SchemaNode::scalar("kind", ScalarType::String)],
            )
            .with_alternatives(nullable_discriminator.alternatives().to_vec())
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
    fn xml_restricted_alternatives_are_explicit_and_validated() {
        let mut restricted = SchemaNode::group(
            "Record",
            vec![
                SchemaNode::scalar("id", ScalarType::String),
                SchemaNode::scalar("note", ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            GroupAlternative {
                name: "Base".into(),
                members: vec!["id".into(), "note".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
            GroupAlternative {
                name: "Compact".into(),
                members: vec!["id".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
        ])
        .unwrap();
        assert!(restricted.set_xml_restricted_alternatives(vec!["Compact".into()]));
        assert_eq!(restricted.xml_restricted_alternatives(), ["Compact"]);
        let encoded = serde_json::to_string(&restricted).unwrap();
        assert!(encoded.contains(r#""xml_restricted_alternatives":["Compact"]"#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            restricted
        );

        assert!(
            !restricted.set_xml_restricted_alternatives(vec!["Compact".into(), "Compact".into()])
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"Record","kind":{"kind":"group","children":[],"xml_restricted_alternatives":["Missing"]}}"#
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
    fn required_fields_are_validated_and_roundtrip() {
        let schema = SchemaNode::group(
            "Order",
            vec![
                SchemaNode::scalar("id", ScalarType::Int),
                SchemaNode::scalar("note", ScalarType::String),
            ],
        )
        .with_required_fields(vec!["id".into(), "note".into()])
        .unwrap();
        assert_eq!(schema.required_fields(), ["id", "note"]);
        let encoded = serde_json::to_string(&schema).unwrap();
        assert!(encoded.contains(r#""required":["id","note"]"#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            schema
        );

        assert!(
            SchemaNode::group("Closed", Vec::new())
                .with_required_fields(vec!["missing".into()])
                .is_none()
        );
        let open = SchemaNode::group("Open", Vec::new())
            .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
            .unwrap()
            .with_required_fields(vec!["runtime-name".into()])
            .unwrap();
        assert_eq!(open.required_fields(), ["runtime-name"]);
        let mut cannot_close = open.clone();
        assert!(!cannot_close.set_dynamic_fields(None));
        assert!(cannot_close.dynamic_fields().is_some());
        assert!(
            open.clone()
                .with_required_fields(vec!["same".into(), "same".into()])
                .is_none()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"Broken","kind":{"kind":"group","children":[],"required":["missing"]}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"Legacy","kind":{"kind":"group","children":[]}}"#
            )
            .unwrap()
            .required_fields()
            .is_empty()
        );
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
    fn xml_defaults_are_scalar_only_exclusive_and_serde_defaulted() {
        let defaulted = SchemaNode::scalar("Count", ScalarType::Int)
            .with_default("7")
            .unwrap();
        let encoded = serde_json::to_string(&defaulted).unwrap();
        assert!(encoded.contains(r#""default":"7""#));
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            defaulted
        );

        let legacy: SchemaNode =
            serde_json::from_str(r#"{"name":"Count","kind":{"kind":"scalar","ty":"int"}}"#)
                .unwrap();
        assert!(legacy.default.is_none());
        assert!(
            SchemaNode::group("Count", Vec::new())
                .with_default("7")
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Count", ScalarType::Int)
                .repeating()
                .with_default("7")
                .is_none()
        );
        assert!(
            SchemaNode::scalar_fixed("Count", ScalarType::Int, "7")
                .with_default("7")
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Count", ScalarType::Int)
                .with_default("7")
                .unwrap()
                .with_value_generation(ValueGeneration::MaxNumber)
                .is_none()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"Count","fixed":"7","default":"7","kind":{"kind":"scalar","ty":"int"}}"#
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
    fn json_container_nullability_and_arbitrary_values_are_validated() {
        let object = SchemaNode::group("object", Vec::new())
            .nullable_container()
            .unwrap();
        let array = SchemaNode::scalar("items", ScalarType::Int)
            .repeating()
            .nullable_container()
            .unwrap();
        let any = SchemaNode::scalar("*", ScalarType::String)
            .json_any()
            .unwrap();
        for schema in [&object, &array, &any] {
            let encoded = serde_json::to_string(schema).unwrap();
            assert_eq!(
                serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
                *schema
            );
        }

        let old: SchemaNode =
            serde_json::from_str(r#"{"name":"object","kind":{"kind":"group","children":[]}}"#)
                .unwrap();
        assert!(!old.container_nullable);
        assert!(!old.json_any);
        assert!(
            SchemaNode::scalar("value", ScalarType::String)
                .nullable_container()
                .is_none()
        );
        assert!(SchemaNode::group("value", Vec::new()).json_any().is_none());
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"object","json_any":true,"kind":{"kind":"group","children":[]}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn arbitrary_json_rejects_bypassed_scalar_metadata_in_either_builder_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(any) = SchemaNode::scalar("*", ScalarType::String).json_any() else {
            panic!("plain arbitrary JSON schema should be valid");
        };
        assert!(any.clone().nullable().is_none());
        assert!(any.clone().with_fixed("value").is_none());
        assert!(any.clone().with_default("value").is_none());
        assert!(
            any.clone()
                .with_value_generation(ValueGeneration::MaxNumber)
                .is_none()
        );

        assert!(
            SchemaNode::scalar("*", ScalarType::String)
                .nullable()
                .and_then(SchemaNode::json_any)
                .is_none()
        );
        assert!(
            SchemaNode::scalar_fixed("*", ScalarType::String, "value")
                .json_any()
                .is_none()
        );
        assert!(
            SchemaNode::scalar("*", ScalarType::String)
                .with_default("value")
                .and_then(SchemaNode::json_any)
                .is_none()
        );
        assert!(
            SchemaNode::scalar("*", ScalarType::String)
                .with_value_generation(ValueGeneration::MaxNumber)
                .and_then(SchemaNode::json_any)
                .is_none()
        );

        for invalid in [
            r#"{"name":"*","json_any":true,"nullable":true,"kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"*","json_any":true,"fixed":"value","kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"*","json_any":true,"default":"value","kind":{"kind":"scalar","ty":"string"}}"#,
            r#"{"name":"*","json_any":true,"value_generation":"max_number","kind":{"kind":"scalar","ty":"string"}}"#,
        ] {
            assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());
        }
        Ok(())
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
    fn xml_name_alternatives_require_unique_exact_element_names() {
        let primary =
            XmlNamespace::qualified("urn:ferrule:name:first").unwrap_or(XmlNamespace::Unqualified);
        let alternate =
            XmlNamespace::qualified("urn:ferrule:name:second").unwrap_or(XmlNamespace::Unqualified);
        let schema = SchemaNode::scalar("Note", ScalarType::String)
            .xml_qualified("urn:ferrule:name:first")
            .and_then(|schema| schema.with_xml_name_alternatives(vec![alternate.clone()]))
            .unwrap_or_else(|| SchemaNode::scalar("invalid", ScalarType::String));
        assert!(schema.xml_namespace_matches(primary.uri()));
        assert!(schema.xml_namespace_matches(alternate.uri()));
        let encoded = serde_json::to_string(&schema).unwrap();
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            schema
        );

        assert!(
            SchemaNode::scalar("Note", ScalarType::String)
                .with_xml_name_alternatives(vec![alternate.clone()])
                .is_none()
        );
        assert!(
            SchemaNode::scalar("Note", ScalarType::String)
                .xml_qualified("urn:ferrule:name:first")
                .and_then(
                    |schema| schema.with_xml_name_alternatives(vec![alternate.clone(), alternate,])
                )
                .is_none()
        );
        let legacy: SchemaNode =
            serde_json::from_str(r#"{"name":"Note","kind":{"kind":"scalar","ty":"string"}}"#)
                .unwrap();
        assert!(legacy.xml_name_alternatives.is_empty());
    }

    #[test]
    fn xml_wildcard_namespaces_are_typed_validated_and_serde_defaulted() {
        let constraint = XmlWildcardNamespaceConstraint::list([
            XmlNamespace::Unqualified,
            XmlNamespace::qualified("urn:ferrule:external").unwrap_or(XmlNamespace::Unqualified),
        ])
        .unwrap_or(XmlWildcardNamespaceConstraint::Any);
        assert!(constraint.allows(None));
        assert!(constraint.allows(Some("urn:ferrule:external")));
        assert!(!constraint.allows(Some("urn:ferrule:blocked")));

        let mut wildcard = SchemaNode::group(
            XML_ELEMENTS_FIELD,
            vec![
                SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
                SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
            ],
        )
        .repeating()
        .with_xml_wildcard_namespace(constraint)
        .unwrap_or_else(|| SchemaNode::group("invalid", Vec::new()));
        wildcard.xml_wildcard_process_contents = XmlWildcardProcessContents::Lax;
        assert!(wildcard.xml_wildcard_namespace_is_valid());
        assert!(wildcard.xml_wildcard_process_contents_is_valid());
        let encoded = serde_json::to_string(&wildcard).unwrap();
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
            wildcard
        );

        let other = XmlWildcardNamespaceConstraint::Other {
            target_namespace: XmlNamespaceUri::new("urn:ferrule:owner"),
        };
        assert!(!other.allows(None));
        assert!(!other.allows(Some("urn:ferrule:owner")));
        assert!(other.allows(Some("urn:ferrule:external")));
        assert!(
            SchemaNode::group("ordinary", Vec::new())
                .repeating()
                .with_xml_wildcard_namespace(XmlWildcardNamespaceConstraint::Any)
                .is_none()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"element()","repeating":true,"xml_wildcard_namespace":{"kind":"list","namespaces":[]},"kind":{"kind":"group","children":[]}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SchemaNode>(
                r#"{"name":"ordinary","xml_wildcard_process_contents":"strict","kind":{"kind":"group","children":[]}}"#
            )
            .is_err()
        );

        let legacy: SchemaNode =
            serde_json::from_str(r#"{"name":"Root","kind":{"kind":"group","children":[]}}"#)
                .unwrap();
        assert!(legacy.xml_wildcard_namespace.is_none());
        assert_eq!(
            legacy.xml_wildcard_process_contents,
            XmlWildcardProcessContents::Skip
        );
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
