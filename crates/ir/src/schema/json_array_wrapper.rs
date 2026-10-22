//! The exact frame invariant of a nested JSON array wrapper.
//!
//! A `json_array_wrapper` group is a JSON-transparent frame: it repeats once
//! per outer array item, and its sole repeating [`JSON_ARRAY_ITEM_FIELD`]
//! child owns the complete inner array schema. The frame itself may carry
//! only its name and outer-array metadata (container null, item-count bounds,
//! `contains` and `uniqueItems`). Saved-project deserialization, the JSON
//! readers and writers, JSON Schema export and engine validation all consult
//! this one check.

use crate::{
    GroupAlternativeMode, JSON_ARRAY_ITEM_FIELD, SchemaKind, SchemaNode, XmlAlternativeKind,
    XmlWildcardProcessContents,
};

/// The first way a `json_array_wrapper` node departs from the exact frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonArrayWrapperFrameError {
    /// The frame is a scalar rather than a group.
    NotGroup,
    /// The frame does not repeat, so it cannot be an outer array.
    NotRepeating,
    /// The frame does not have exactly one child.
    ChildCount { count: usize },
    /// The sole child is not named [`JSON_ARRAY_ITEM_FIELD`].
    ItemName,
    /// The sole `item` child does not repeat, so it cannot be an inner array.
    ItemNotRepeating,
    /// The frame carries a recursive schema reference.
    RecursiveReference,
    /// The frame carries object metadata: required, alternative or dynamic
    /// properties, object-item null, or property constraints.
    ObjectMetadata,
    /// The frame carries scalar value metadata: null, arbitrary JSON, fixed,
    /// allowed, default or generated values, or value constraints.
    ScalarMetadata,
    /// The frame carries XML naming, occurrence, nil, type or compositor
    /// metadata.
    XmlMetadata,
    /// The frame carries a database relation.
    DatabaseRelation,
}

impl core::fmt::Display for JsonArrayWrapperFrameError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotGroup => formatter.write_str("the frame is not a group"),
            Self::NotRepeating => formatter.write_str("the frame does not repeat"),
            Self::ChildCount { count } => write!(
                formatter,
                "the frame has {count} children instead of one `{JSON_ARRAY_ITEM_FIELD}` child"
            ),
            Self::ItemName => write!(
                formatter,
                "the frame's sole child is not named `{JSON_ARRAY_ITEM_FIELD}`"
            ),
            Self::ItemNotRepeating => write!(
                formatter,
                "the frame's `{JSON_ARRAY_ITEM_FIELD}` child does not repeat"
            ),
            Self::RecursiveReference => {
                formatter.write_str("the frame declares a recursive reference")
            }
            Self::ObjectMetadata => formatter.write_str("the frame declares object metadata"),
            Self::ScalarMetadata => formatter.write_str("the frame declares scalar value metadata"),
            Self::XmlMetadata => formatter.write_str("the frame declares XML metadata"),
            Self::DatabaseRelation => formatter.write_str("the frame declares a database relation"),
        }
    }
}

impl std::error::Error for JsonArrayWrapperFrameError {}

impl SchemaNode {
    /// Checks the distinct array frame without copying its child tree.
    ///
    /// A node without `json_array_wrapper` has no frame and always passes.
    pub fn check_json_array_wrapper_frame(&self) -> Result<(), JsonArrayWrapperFrameError> {
        if !self.json_array_wrapper {
            return Ok(());
        }
        let SchemaKind::Group {
            children,
            alternatives,
            required,
            xml_restricted_alternatives,
            dynamic,
        } = &self.kind
        else {
            return Err(JsonArrayWrapperFrameError::NotGroup);
        };
        if !self.repeating {
            return Err(JsonArrayWrapperFrameError::NotRepeating);
        }
        let [item] = children.as_slice() else {
            return Err(JsonArrayWrapperFrameError::ChildCount {
                count: children.len(),
            });
        };
        if item.name != JSON_ARRAY_ITEM_FIELD {
            return Err(JsonArrayWrapperFrameError::ItemName);
        }
        if !item.repeating {
            return Err(JsonArrayWrapperFrameError::ItemNotRepeating);
        }
        if self.recursive_ref.is_some() {
            return Err(JsonArrayWrapperFrameError::RecursiveReference);
        }
        if !alternatives.is_empty()
            || !required.is_empty()
            || dynamic.is_some()
            || self.alternative_mode != GroupAlternativeMode::Exclusive
            || self.json_object_item_nullable.is_some()
            || self.json_dependent_schemas.is_some()
            || self.property_count_range.is_some()
            || self.json_property_dependencies.is_some()
            || self.json_pattern_property_names.is_some()
            || self.json_property_names.is_some()
        {
            return Err(JsonArrayWrapperFrameError::ObjectMetadata);
        }
        if self.nullable
            || self.json_any
            || self.fixed.is_some()
            || self.json_allowed_values.is_some()
            || self.numeric_range.is_some()
            || self.json_multiple_of.is_some()
            || self.string_length_range.is_some()
            || self.json_patterns.is_some()
            || !self.json_formats.is_empty()
            || self.default.is_some()
            || self.value_generation.is_some()
        {
            return Err(JsonArrayWrapperFrameError::ScalarMetadata);
        }
        if !xml_restricted_alternatives.is_empty()
            || self.xml_namespace.is_some()
            || !self.xml_name_alternatives.is_empty()
            || self.xml_wildcard_namespace.is_some()
            || self.xml_wildcard_process_contents != XmlWildcardProcessContents::Skip
            || self.attribute
            || self.text
            || self.nillable
            || self.xml_optional
            || self.xml_attribute_required
            || self.xml_alternative_kind != XmlAlternativeKind::XsiType
            || self.xml_type_alternatives
            || self.xml_default_type.is_some()
            || !self.xml_repeating_sequences.is_empty()
            || !self.xml_repeating_choices.is_empty()
        {
            return Err(JsonArrayWrapperFrameError::XmlMetadata);
        }
        if self.database_relation.is_some() {
            return Err(JsonArrayWrapperFrameError::DatabaseRelation);
        }
        Ok(())
    }

    /// Validates the distinct array frame without copying its child tree.
    pub fn json_array_wrapper_is_valid(&self) -> bool {
        self.check_json_array_wrapper_frame().is_ok()
    }
}
