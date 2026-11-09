//! Constructors and scalar/occurrence metadata builders.

use crate::{
    DatabaseRelation, GroupAlternativeMode, ItemCountRange, JsonAllowedValues,
    JsonContainsConstraints, JsonDependentSchemaConstraints, JsonFormatAnnotations,
    JsonMultipleOfConstraints, JsonPatternConstraints, NumericRange, ScalarType, ScalarTypeSet,
    SchemaKind, SchemaNode, StringLengthRange, ValueGeneration, XmlAlternativeKind,
    XmlWildcardProcessContents,
};

impl SchemaNode {
    pub fn scalar(name: impl Into<String>, ty: ScalarType) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            xml_name_alternatives: Vec::new(),
            xml_wildcard_namespace: None,
            xml_wildcard_process_contents: XmlWildcardProcessContents::Skip,
            xml_wildcard_excluded_names: None,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            xml_optional: false,
            xml_attribute_required: false,
            nullable: false,
            container_nullable: false,
            json_object_item_nullable: None,
            json_array_wrapper: false,
            json_tuple: None,
            json_any: false,
            json_undeclared_property: false,
            fixed: None,
            json_allowed_values: None,
            numeric_range: None,
            json_multiple_of: None,
            item_count_range: None,
            json_contains: None,
            json_dependent_schemas: None,
            property_count_range: None,
            json_property_dependencies: None,
            json_pattern_property_names: None,
            json_property_names: None,
            json_unique_items: false,
            string_length_range: None,
            json_patterns: None,
            json_formats: JsonFormatAnnotations::default(),
            default: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_type_alternatives: false,
            xml_default_type: None,
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
            xml_wildcard_excluded_names: None,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            xml_optional: false,
            xml_attribute_required: false,
            nullable: false,
            container_nullable: false,
            json_object_item_nullable: None,
            json_array_wrapper: false,
            json_tuple: None,
            json_any: false,
            json_undeclared_property: false,
            fixed: None,
            json_allowed_values: None,
            numeric_range: None,
            json_multiple_of: None,
            item_count_range: None,
            json_contains: None,
            json_dependent_schemas: None,
            property_count_range: None,
            json_property_dependencies: None,
            json_pattern_property_names: None,
            json_property_names: None,
            json_unique_items: false,
            string_length_range: None,
            json_patterns: None,
            json_formats: JsonFormatAnnotations::default(),
            default: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_type_alternatives: false,
            xml_default_type: None,
            xml_repeating_sequences: Vec::new(),
            xml_repeating_choices: Vec::new(),
            database_relation: None,
            kind: SchemaKind::ScalarUnion { types },
        }
    }

    pub fn group(name: impl Into<String>, children: Vec<SchemaNode>) -> Self {
        Self {
            name: name.into(),
            xml_namespace: None,
            xml_name_alternatives: Vec::new(),
            xml_wildcard_namespace: None,
            xml_wildcard_process_contents: XmlWildcardProcessContents::Skip,
            xml_wildcard_excluded_names: None,
            repeating: false,
            recursive_ref: None,
            attribute: false,
            text: false,
            nillable: false,
            xml_optional: false,
            xml_attribute_required: false,
            nullable: false,
            container_nullable: false,
            json_object_item_nullable: None,
            json_array_wrapper: false,
            json_tuple: None,
            json_any: false,
            json_undeclared_property: false,
            fixed: None,
            json_allowed_values: None,
            numeric_range: None,
            json_multiple_of: None,
            item_count_range: None,
            json_contains: None,
            json_dependent_schemas: None,
            property_count_range: None,
            json_property_dependencies: None,
            json_pattern_property_names: None,
            json_property_names: None,
            json_unique_items: false,
            string_length_range: None,
            json_patterns: None,
            json_formats: JsonFormatAnnotations::default(),
            default: None,
            value_generation: None,
            alternative_mode: GroupAlternativeMode::Exclusive,
            xml_alternative_kind: XmlAlternativeKind::XsiType,
            xml_type_alternatives: false,
            xml_default_type: None,
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

    pub fn with_json_array_wrapper(mut self) -> Option<Self> {
        self.json_array_wrapper = true;
        (self.json_array_wrapper_is_valid() && self.json_tuple_frame_is_valid()).then_some(self)
    }

    /// Attaches exact relation endpoints to a nested repeating database group.
    pub fn with_database_relation(mut self, relation: DatabaseRelation) -> Option<Self> {
        self.database_relation = Some(relation);
        (self.database_relation_is_valid() && self.json_tuple_frame_is_valid()).then_some(self)
    }

    /// Marks a non-repeating scalar as format-generated.
    pub fn with_value_generation(mut self, generation: ValueGeneration) -> Option<Self> {
        self.value_generation = Some(generation);
        (self.value_generation_is_valid() && self.json_any_is_valid()).then_some(self)
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

    pub fn with_json_contains(mut self, constraints: JsonContainsConstraints) -> Option<Self> {
        self.json_contains = Some(constraints);
        (self.json_contains_are_valid()
            && self.json_tuple_frame_is_valid()
            && self.json_pattern_budget_is_valid())
        .then_some(self)
    }

    pub fn with_json_dependent_schemas(
        mut self,
        constraints: JsonDependentSchemaConstraints,
    ) -> Option<Self> {
        self.json_dependent_schemas = Some(constraints);
        (self.json_dependent_schemas_are_valid()
            && self.json_tuple_frame_is_valid()
            && self.json_pattern_budget_is_valid())
        .then_some(self)
    }

    /// Requires pairwise-distinct JSON array items under JSON Schema equality.
    pub fn with_json_unique_items(mut self) -> Option<Self> {
        self.json_unique_items = true;
        (self.json_unique_items_is_valid() && self.json_tuple_frame_is_valid()).then_some(self)
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
}
