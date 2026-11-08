//! Schema node representation, deserialization and responsibility-specific methods.

use serde::{Deserialize, Serialize};

use crate::{
    DatabaseRelation, GroupAlternativeMode, ItemCountRange, JsonAllowedValues,
    JsonContainsConstraints, JsonDependentSchemaConstraints, JsonFormatAnnotations,
    JsonMultipleOfConstraints, JsonPatternConstraints, JsonPatternPropertyNames,
    JsonPropertyDependencies, JsonPropertyNameConstraints, JsonTupleLayout, NumericRange,
    PropertyCountRange, SchemaKind, StringLengthRange, ValueGeneration, XmlAlternativeKind,
    XmlNamespace, XmlRepeatingChoice, XmlRepeatingSequence, XmlWildcardExcludedNames,
    XmlWildcardNamespaceConstraint, XmlWildcardProcessContents,
};

mod access;
mod alternatives;
mod builders;
mod json_objects;
mod json_patterns;
mod json_validity;
mod metadata;
mod xml_validity;

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
    /// XSD 1.1 expanded names this generic wildcard group never matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml_wildcard_excluded_names: Option<XmlWildcardExcludedNames>,
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
    /// Non-XML formats ignore it; in [`crate::Instance`] trees an attribute is an
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
    /// This named singular XML element occurrence permits absence (0..1).
    /// XML and JSON instance boundaries retain their existing presence rules.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub xml_optional: bool,
    /// This singular XML attribute use requires an explicitly present attribute.
    /// Instance boundaries retain their existing lenient presence semantics.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub xml_attribute_required: bool,
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
    /// Explicit nullability of each object item in a repeating JSON group.
    /// Absence retains the legacy `container_nullable` item behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_object_item_nullable: Option<bool>,
    /// This repeating group preserves an outer JSON array whose items are
    /// themselves arrays. Its sole repeating `item` child owns the complete
    /// inner array schema; JSON boundaries make this group frame transparent.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub json_array_wrapper: bool,
    /// This group is a JSON Schema tuple frame: an array whose positions
    /// have their own schemas. Its children are the positional members
    /// `item1`..`itemN` and, for an open tail, a repeating `rest` item.
    /// See [`SchemaNode::check_json_tuple_frame`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_tuple: Option<JsonTupleLayout>,
    /// This string scalar stores one arbitrary JSON value as canonical JSON
    /// text. JSON boundaries use it for unconstrained dynamic object fields;
    /// other formats and the mapping graph continue to see an ordinary string.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub json_any: bool,
    /// This object property is an authored member that its parent object's
    /// JSON Schema does not declare in `properties` but admits through its
    /// dynamic value schema (`additionalProperties`, or a matching
    /// `patternProperties` selector).
    ///
    /// The member keeps its exact authored name and carries exactly the
    /// parent's dynamic value schema, so instance boundaries govern it as that
    /// runtime-named property. JSON Schema export omits it from `properties`.
    /// See [`SchemaNode::add_json_undeclared_property`].
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub json_undeclared_property: bool,
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
    /// Integer bounds are normalized to an inclusive `i64` interval. On an
    /// exact String-or-Int union they constrain only its Int member. Number
    /// bounds retain finite values and endpoint exclusivity on a Float scalar.
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
    /// Exact, independently counted JSON Schema `contains` assertions for
    /// this array wrapper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_contains: Option<JsonContainsConstraints>,
    /// Exact conditional whole-object JSON Schema assertions.
    ///
    /// Each predicate applies to this complete object when its trigger
    /// property is present. Repeated triggers form a conjunction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_dependent_schemas: Option<JsonDependentSchemaConstraints>,
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
    /// Exact portable patterns selecting runtime-named properties governed by
    /// this object's dynamic value schema.
    ///
    /// Sources are ORed in declaration order. A statically declared child
    /// whose name also matches a selector must have the same schema as the
    /// dynamic value, ignoring only its own property name. Nonmatching
    /// statically declared children remain independent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_pattern_property_names: Option<JsonPatternPropertyNames>,
    /// Exact portable JSON Schema assertions applied to every property name
    /// in this object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_property_names: Option<JsonPropertyNameConstraints>,
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
    /// These alternatives came from XSD `xsi:type` derivation. A JSON host
    /// may resolve overlapping projections only when one matching type has
    /// strictly fewer members than every other matching derived type.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub xml_type_alternatives: bool,
    /// Exact expanded identity of a retained concrete declared XML type.
    /// Absent metadata remains conservative for abstract or legacy declarations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml_default_type: Option<String>,
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

std::thread_local! {
    static SCHEMA_DESERIALIZE_DEPTH: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

struct SchemaDeserializeGuard {
    is_root: bool,
}

impl SchemaDeserializeGuard {
    fn enter() -> Self {
        let is_root = SCHEMA_DESERIALIZE_DEPTH.with(|depth| {
            let current = depth.get();
            depth.set(current.saturating_add(1));
            current == 0
        });
        Self { is_root }
    }
}

impl Drop for SchemaDeserializeGuard {
    fn drop(&mut self) {
        SCHEMA_DESERIALIZE_DEPTH.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

pub(crate) fn schema_deserialization_is_active() -> bool {
    SCHEMA_DESERIALIZE_DEPTH.with(|depth| depth.get() != 0)
}

fn deserialize_explicit_object_item_nullable<'de, D>(
    deserializer: D,
) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    bool::deserialize(deserializer).map(Some)
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
            xml_wildcard_excluded_names: Option<XmlWildcardExcludedNames>,
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
            xml_optional: bool,
            #[serde(default)]
            xml_attribute_required: bool,
            #[serde(default)]
            nullable: bool,
            #[serde(default)]
            container_nullable: bool,
            #[serde(
                default,
                deserialize_with = "deserialize_explicit_object_item_nullable"
            )]
            json_object_item_nullable: Option<bool>,
            #[serde(default)]
            json_array_wrapper: bool,
            #[serde(default)]
            json_tuple: Option<JsonTupleLayout>,
            #[serde(default)]
            json_any: bool,
            #[serde(default)]
            json_undeclared_property: bool,
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
            json_contains: Option<JsonContainsConstraints>,
            #[serde(default)]
            json_dependent_schemas: Option<JsonDependentSchemaConstraints>,
            #[serde(default)]
            property_count_range: Option<PropertyCountRange>,
            #[serde(default)]
            json_property_dependencies: Option<JsonPropertyDependencies>,
            #[serde(default)]
            json_pattern_property_names: Option<JsonPatternPropertyNames>,
            #[serde(default)]
            json_property_names: Option<JsonPropertyNameConstraints>,
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
            xml_type_alternatives: bool,
            #[serde(default)]
            xml_default_type: Option<String>,
            #[serde(default)]
            xml_repeating_sequences: Vec<XmlRepeatingSequence>,
            #[serde(default)]
            xml_repeating_choices: Vec<XmlRepeatingChoice>,
            #[serde(default)]
            database_relation: Option<DatabaseRelation>,
            kind: SchemaKind,
        }

        let guard = SchemaDeserializeGuard::enter();
        let repr = Repr::deserialize(deserializer)?;
        let node = Self {
            name: repr.name,
            xml_namespace: repr.xml_namespace,
            xml_name_alternatives: repr.xml_name_alternatives,
            xml_wildcard_namespace: repr.xml_wildcard_namespace,
            xml_wildcard_process_contents: repr.xml_wildcard_process_contents,
            xml_wildcard_excluded_names: repr.xml_wildcard_excluded_names,
            repeating: repr.repeating,
            recursive_ref: repr.recursive_ref,
            attribute: repr.attribute,
            text: repr.text,
            nillable: repr.nillable,
            xml_optional: repr.xml_optional,
            xml_attribute_required: repr.xml_attribute_required,
            nullable: repr.nullable,
            container_nullable: repr.container_nullable,
            json_object_item_nullable: repr.json_object_item_nullable,
            json_array_wrapper: repr.json_array_wrapper,
            json_tuple: repr.json_tuple,
            json_any: repr.json_any,
            json_undeclared_property: repr.json_undeclared_property,
            fixed: repr.fixed,
            json_allowed_values: repr.json_allowed_values,
            numeric_range: repr.numeric_range,
            json_multiple_of: repr.json_multiple_of,
            item_count_range: repr.item_count_range,
            json_contains: repr.json_contains,
            json_dependent_schemas: repr.json_dependent_schemas,
            property_count_range: repr.property_count_range,
            json_property_dependencies: repr.json_property_dependencies,
            json_pattern_property_names: repr.json_pattern_property_names,
            json_property_names: repr.json_property_names,
            json_unique_items: repr.json_unique_items,
            string_length_range: repr.string_length_range,
            json_patterns: repr.json_patterns,
            json_formats: repr.json_formats,
            default: repr.default,
            value_generation: repr.value_generation,
            alternative_mode: repr.alternative_mode,
            xml_alternative_kind: repr.xml_alternative_kind,
            xml_type_alternatives: repr.xml_type_alternatives,
            xml_default_type: repr.xml_default_type,
            xml_repeating_sequences: repr.xml_repeating_sequences,
            xml_repeating_choices: repr.xml_repeating_choices,
            database_relation: repr.database_relation,
            kind: repr.kind,
        };
        if !node.json_object_item_nullable_is_valid() {
            return Err(serde::de::Error::custom(
                "invalid JSON object-item nullability metadata",
            ));
        }
        if guard.is_root && !node.json_array_wrappers_tree_is_valid() {
            return Err(serde::de::Error::custom(
                "invalid JSON array-wrapper metadata",
            ));
        }
        if guard.is_root && !node.json_tuple_frames_tree_is_valid() {
            return Err(serde::de::Error::custom(
                "invalid JSON tuple frame metadata",
            ));
        }
        if guard.is_root && node.json_undeclared_property {
            return Err(serde::de::Error::custom(
                "a schema root cannot be an undeclared JSON object property",
            ));
        }
        if guard.is_root
            && (!node.metadata_tree_without_pattern_evaluation_is_valid()
                || !node.json_pattern_budget_is_valid())
        {
            return Err(serde::de::Error::custom(
                "schema metadata contains invalid nested alternatives, required fields, recursion, fixed or JSON allowed values, numeric range, JSON multipleOf constraints, item-count, contains, property-count, property-dependency, pattern-property selector, property-name, or unique-items constraints, string-length range, JSON pattern constraints or format annotations, value generation, default value, alternative mode, XML alternative kind, XML name alternatives, XML repeating sequences or choices, XML wildcard namespace, process policy or excluded names, database relation, or JSON nullability, or exceeds its schema-wide JSON pattern program or match-work budget",
            ));
        }
        Ok(node)
    }
}
