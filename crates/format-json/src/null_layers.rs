//! Where the JSON boundary admits explicit null, derived from the effective
//! JSON view.
//!
//! JSON Schema export renders these layers, the instance reader and both
//! writers accept or emit `null` from them, and JSON Schema import reads them
//! when it composes, compares and intersects imported shapes, so object null,
//! array-container null, object-item null and scalar value null keep their
//! separate meanings.
//! Every reader and writer preflight ([`crate::RecursiveSchemas::new`]) refuses
//! a schema with any node whose JSON null metadata has no effective
//! interpretation before a value is read or written.

use ir::{EffectiveViewError, JsonNullability, SchemaKind, SchemaNode};

use crate::JsonFormatError;

#[cfg(test)]
mod tests;

/// Where one node renders arrays and admits explicit JSON null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JsonLayers {
    /// The whole object, array or nested array may be `null`; JSON Schema
    /// renders it as `anyOf [..., null]`.
    pub(crate) container_null: bool,
    /// The node is an array of its shape.
    pub(crate) array: bool,
    /// Each object item of the array may be `null`.
    pub(crate) object_item_null: bool,
    /// The scalar value, or each scalar item, may be `null`.
    pub(crate) value_null: bool,
}

/// Derives the JSON layers from the effective JSON view, so scalar null,
/// object null and array-container null stay distinct.
pub(crate) fn layers(node: &SchemaNode) -> Result<JsonLayers, JsonFormatError> {
    let group = matches!(node.kind, SchemaKind::Group { .. });
    let nullability = node
        .effective_json_nullability()
        .map_err(|error| match error {
            EffectiveViewError::InvalidJsonArrayWrapper => {
                JsonFormatError::InvalidArrayWrapperMetadata {
                    name: node.name.clone(),
                }
            }
            EffectiveViewError::ObjectItemNullOutsideObjectArray => {
                JsonFormatError::InvalidObjectItemNullabilityMetadata {
                    name: node.name.clone(),
                }
            }
            reason => JsonFormatError::InvalidNullabilityMetadata {
                name: node.name.clone(),
                reason,
            },
        })?;
    Ok(match nullability {
        JsonNullability::Occurrence { null } => JsonLayers {
            container_null: group && null,
            array: false,
            object_item_null: false,
            value_null: !group && null,
        },
        JsonNullability::Array {
            container_null,
            item_null,
        } => JsonLayers {
            container_null,
            array: true,
            // Arbitrary JSON carries null as canonical text, not as a variant.
            object_item_null: group && !node.json_any && item_null,
            value_null: !group && item_null,
        },
        // Each nested array's nullability belongs to the wrapper's item child.
        JsonNullability::NestedArrays { container_null } => JsonLayers {
            container_null,
            array: true,
            object_item_null: false,
            value_null: false,
        },
    })
}

/// Where the instance reader and writers accept or emit `null` for one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstanceNull {
    /// The complete occurrence is `null`: one object, or one whole array of
    /// values or nested arrays. A missing property of such a node reads as
    /// absent rather than as an empty array.
    pub(crate) whole: bool,
    /// One singular object or object-array item is `null` before its own
    /// shape is read or written. Nested-array frames and arbitrary JSON never
    /// consult it: their items are owned by the frame's `item` child or are
    /// JSON text. Scalar array items admit null only through `value`.
    pub(crate) single: bool,
    /// The scalar value, or each scalar item, is `null`.
    pub(crate) value: bool,
}

/// Derives the instance boundary's null admission from [`layers`].
pub(crate) fn instance_null(node: &SchemaNode) -> Result<InstanceNull, JsonFormatError> {
    let layers = layers(node)?;
    let single = match (layers.array, &node.kind) {
        (false, _) => layers.container_null,
        (true, SchemaKind::Group { .. }) => layers.object_item_null,
        // A scalar item's null belongs to its declared value domain, never to
        // the array container.
        (true, SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }) => false,
    };
    Ok(InstanceNull {
        whole: layers.container_null,
        single,
        value: layers.value_null,
    })
}

/// The scalar value, or each scalar item, admits `null`.
pub(crate) fn value_null(node: &SchemaNode) -> Result<bool, JsonFormatError> {
    Ok(layers(node)?.value_null)
}

/// One object occurrence, or one whole array or nested array, admits `null`.
pub(crate) fn container_null(node: &SchemaNode) -> Result<bool, JsonFormatError> {
    Ok(layers(node)?.container_null)
}

/// One singular object, or each object item of an array, admits `null`.
pub(crate) fn single_null(node: &SchemaNode) -> Result<bool, JsonFormatError> {
    Ok(instance_null(node)?.single)
}
