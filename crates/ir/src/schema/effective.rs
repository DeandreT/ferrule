//! Effective occurrence and null-admission views over saved schema nodes.
//!
//! [`SchemaNode`] stores boundary metadata as independent saved flags whose
//! meaning depends on the node's role: `nullable` is a scalar value (or each
//! scalar array item), `container_nullable` is a JSON object occurrence or an
//! array wrapper, `json_object_item_nullable` overrides the object items of an
//! array, `nillable` is an XML `xsi:nil` occurrence, and `xml_optional` or
//! `xml_attribute_required` declare singular XML presence. These views derive
//! that combined interpretation once from the unchanged saved DTO. They add no
//! saved fields and do not replace [`SchemaNode`] as the schema model.
//!
//! The views keep absence, scalar or object JSON null, array-container JSON
//! null and XML nil as separate facts. Absence is described by
//! [`EffectivePresence`] and [`EffectiveCardinality`]; a missing occurrence is
//! never a JSON null or XML nil value.

use crate::{SchemaKind, SchemaNode};

use super::ItemCountRange;

/// The combined effective occurrence and null-admission view of one node.
///
/// Construct it with [`SchemaNode::effective_view`]; the cardinality and
/// nullability it reports always describe the same occurrence shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveSchemaView {
    cardinality: EffectiveCardinality,
    nullability: EffectiveNullability,
}

impl EffectiveSchemaView {
    pub fn cardinality(self) -> EffectiveCardinality {
        self.cardinality
    }

    pub fn nullability(self) -> EffectiveNullability {
        self.nullability
    }

    /// Exactly one declared occurrence that admits no null-like value at any
    /// XML or JSON layer.
    pub fn is_required_non_null_singleton(self) -> bool {
        self.cardinality
            == EffectiveCardinality::Single {
                presence: EffectivePresence::Required,
            }
            && !self.nullability.admits_null()
    }
}

/// How many occurrences of a node one parent occurrence carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveCardinality {
    /// At most one occurrence, with its declared presence.
    Single { presence: EffectivePresence },
    /// An ordered collection: XML element repetition or one JSON array.
    ///
    /// A missing collection is an empty collection, not an absent value.
    Repeated {
        /// Exact JSON array length bounds. Absent bounds are unconstrained.
        item_count: Option<ItemCountRange>,
        items: RepeatedItems,
    },
}

/// The shape of each item of a repeated node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatedItems {
    /// Each item is one scalar value or one object/group occurrence.
    Values,
    /// Each item is itself a JSON array whose complete schema is owned by the
    /// wrapper's sole `item` child; the wrapper frame is JSON-transparent.
    NestedArrays,
}

/// Declared XML presence of one singular occurrence.
///
/// Elements are required unless `xml_optional`; attributes are optional unless
/// `xml_attribute_required`; simple text content is required. JSON property
/// presence is owned by the parent's required-property list instead. XML and
/// JSON instance readers keep their lenient handling of missing occurrences;
/// this is the declared presence consulted by schema export and mapping proofs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectivePresence {
    Required,
    Optional,
}

/// Which explicit null-like values each format boundary admits at this node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveNullability {
    xml_nil: bool,
    json: JsonNullability,
}

impl EffectiveNullability {
    /// The XML occurrence may carry `xsi:nil="true"` ([`crate::Value::XmlNil`]).
    pub fn xml_nil(self) -> bool {
        self.xml_nil
    }

    pub fn json(self) -> JsonNullability {
        self.json
    }

    /// Any layer admits XML nil or an explicit JSON null.
    pub fn admits_null(self) -> bool {
        self.xml_nil || self.json.admits_null()
    }
}

/// Explicit JSON null admission, represented as [`crate::Value::JsonNull`].
///
/// An arbitrary-JSON (`json_any`) value carries `null` as canonical JSON text
/// in its string domain rather than as a JSON null value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonNullability {
    /// One scalar value or object occurrence.
    Occurrence { null: bool },
    /// One array of scalar or object items; the array and each item admit
    /// null independently.
    Array {
        container_null: bool,
        item_null: bool,
    },
    /// One array of nested arrays; each nested array's nullability belongs to
    /// the wrapper's `item` child.
    NestedArrays { container_null: bool },
}

impl JsonNullability {
    pub fn admits_null(self) -> bool {
        match self {
            Self::Occurrence { null } => null,
            Self::Array {
                container_null,
                item_null,
            } => container_null || item_null,
            Self::NestedArrays { container_null } => container_null,
        }
    }
}

/// Saved role metadata that has no single effective interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveViewError {
    /// `nullable` on a group; object and array null use `container_nullable`.
    ScalarNullOutsideScalar,
    /// `container_nullable` on a singular scalar.
    ContainerNullOutsideContainer,
    /// `json_object_item_nullable` outside a repeated object group.
    ObjectItemNullOutsideObjectArray,
    /// `json_array_wrapper` on a node without the exact wrapper frame.
    InvalidJsonArrayWrapper,
    /// `xml_optional` outside a named singular XML element.
    XmlOptionalOutsideElement,
    /// `xml_attribute_required` outside a named singular scalar attribute.
    XmlAttributeRequiredOutsideAttribute,
}

impl core::fmt::Display for EffectiveViewError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::ScalarNullOutsideScalar => "scalar nullability is declared on a group",
            Self::ContainerNullOutsideContainer => {
                "container nullability is declared on a singular scalar"
            }
            Self::ObjectItemNullOutsideObjectArray => {
                "object-item nullability is declared outside a repeated object group"
            }
            Self::InvalidJsonArrayWrapper => "JSON array wrapper metadata has an invalid frame",
            Self::XmlOptionalOutsideElement => {
                "optional XML occurrence is declared outside a named singular element"
            }
            Self::XmlAttributeRequiredOutsideAttribute => {
                "required XML attribute use is declared outside a named singular attribute"
            }
        })
    }
}

impl std::error::Error for EffectiveViewError {}

impl SchemaNode {
    /// Derives this node's effective occurrence and null-admission view.
    ///
    /// Role metadata that contradicts the node's shape is rejected rather than
    /// interpreted. Recursive references report their own local occurrence
    /// metadata; their child shape is resolved separately by the formats.
    pub fn effective_view(&self) -> Result<EffectiveSchemaView, EffectiveViewError> {
        if self.nullable && !self.is_scalar() {
            return Err(EffectiveViewError::ScalarNullOutsideScalar);
        }
        if !self.container_nullable_is_valid() {
            return Err(EffectiveViewError::ContainerNullOutsideContainer);
        }
        if !self.json_object_item_nullable_is_valid() {
            return Err(EffectiveViewError::ObjectItemNullOutsideObjectArray);
        }
        if !self.json_array_wrapper_is_valid() {
            return Err(EffectiveViewError::InvalidJsonArrayWrapper);
        }
        if !self.xml_optional_is_valid() {
            return Err(EffectiveViewError::XmlOptionalOutsideElement);
        }
        if !self.xml_attribute_required_is_valid() {
            return Err(EffectiveViewError::XmlAttributeRequiredOutsideAttribute);
        }
        let group = matches!(self.kind, SchemaKind::Group { .. });
        let (cardinality, json) = if !self.repeating {
            let presence = if self.attribute {
                if self.xml_attribute_required {
                    EffectivePresence::Required
                } else {
                    EffectivePresence::Optional
                }
            } else if self.xml_optional {
                EffectivePresence::Optional
            } else {
                EffectivePresence::Required
            };
            let null = if group {
                self.container_nullable
            } else {
                self.nullable
            };
            (
                EffectiveCardinality::Single { presence },
                JsonNullability::Occurrence { null },
            )
        } else if self.json_array_wrapper {
            (
                EffectiveCardinality::Repeated {
                    item_count: self.item_count_range,
                    items: RepeatedItems::NestedArrays,
                },
                JsonNullability::NestedArrays {
                    container_null: self.container_nullable,
                },
            )
        } else {
            let item_null = if group {
                self.json_single_container_nullable()
            } else {
                self.nullable
            };
            (
                EffectiveCardinality::Repeated {
                    item_count: self.item_count_range,
                    items: RepeatedItems::Values,
                },
                JsonNullability::Array {
                    container_null: self.container_nullable,
                    item_null,
                },
            )
        };
        Ok(EffectiveSchemaView {
            cardinality,
            nullability: EffectiveNullability {
                xml_nil: self.nillable,
                json,
            },
        })
    }
}
