//! Presence and null-admission facts consulted by `.mfd` import and export
//! proofs.
//!
//! Each fact is derived from the effective schema views rather than from the
//! saved role flags, so absence, scalar JSON null, object or array-container
//! JSON null, object-item JSON null and XML nil keep their separate meanings.
//! Role metadata without an effective interpretation never proves a fact.

use ir::{
    EffectiveCardinality, EffectivePresence, JsonNullability, RepeatedItems, SchemaKind,
    SchemaNode, XmlOccurrence,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod wrapper_corpus_tests;

/// No layer admits XML nil or an explicit JSON null, and the node is not a
/// nested-array frame.
pub(super) fn plain_non_null(node: &SchemaNode) -> bool {
    node.effective_view().is_ok_and(|view| {
        !view.nullability().admits_null()
            && !matches!(
                view.cardinality(),
                EffectiveCardinality::Repeated {
                    items: RepeatedItems::NestedArrays,
                    ..
                }
            )
    })
}

/// The JSON layer admits no explicit null at any level: occurrence, array
/// container, scalar item or object item.
pub(super) fn json_non_null(node: &SchemaNode) -> bool {
    node.effective_json_nullability()
        .is_ok_and(|json| !json.admits_null())
}

/// [`json_non_null`], and the node is not a nested-array frame.
pub(super) fn json_plain_non_null(node: &SchemaNode) -> bool {
    node.effective_json_nullability().is_ok_and(|json| {
        !json.admits_null() && !matches!(json, JsonNullability::NestedArrays { .. })
    })
}

/// Neither the JSON occurrence, its array container, nor any scalar value is
/// an explicit null. Whether object items of an array admit null is the
/// separate [`json_object_items_non_null`] fact.
pub(super) fn json_values_non_null(node: &SchemaNode) -> bool {
    match node.effective_json_nullability() {
        Ok(JsonNullability::Occurrence { null }) => !null,
        Ok(JsonNullability::Array {
            container_null,
            item_null,
        }) => !container_null && (!item_null || matches!(node.kind, SchemaKind::Group { .. })),
        Ok(JsonNullability::NestedArrays { container_null }) => !container_null,
        Err(_) => false,
    }
}

/// No object item of this node is an explicit JSON null: the node is not an
/// array of objects, or its object items exclude null.
pub(super) fn json_object_items_non_null(node: &SchemaNode) -> bool {
    match node.effective_json_nullability() {
        Ok(JsonNullability::Array { item_null, .. }) => {
            !item_null || !matches!(node.kind, SchemaKind::Group { .. })
        }
        Ok(_) => true,
        Err(_) => false,
    }
}

/// The object `parent` declares its property `name` as required.
pub(super) fn json_property_required(parent: &SchemaNode, name: &str) -> bool {
    parent.json_property_presence(name) == Some(EffectivePresence::Required)
}

/// The XML occurrence cannot carry `xsi:nil`.
pub(crate) fn xml_non_nil(node: &SchemaNode) -> bool {
    node.effective_xml_view().is_ok_and(|xml| !xml.nil())
}

/// No scalar value, or scalar array item, is an explicit JSON null. Object
/// and array-container null are the separate [`json_container_non_null`].
pub(crate) fn json_value_non_null(node: &SchemaNode) -> bool {
    let group = matches!(node.kind, SchemaKind::Group { .. });
    match node.effective_json_nullability() {
        Ok(JsonNullability::Occurrence { null }) => group || !null,
        Ok(JsonNullability::Array { item_null, .. }) => group || !item_null,
        Ok(JsonNullability::NestedArrays { .. }) => true,
        Err(_) => false,
    }
}

/// Neither one object occurrence nor a whole array or nested array is an
/// explicit JSON null.
pub(crate) fn json_container_non_null(node: &SchemaNode) -> bool {
    match node.effective_json_nullability() {
        Ok(JsonNullability::Occurrence { null }) => {
            !null || !matches!(node.kind, SchemaKind::Group { .. })
        }
        Ok(
            JsonNullability::Array { container_null, .. }
            | JsonNullability::NestedArrays { container_null },
        ) => !container_null,
        Err(_) => false,
    }
}

/// Whether `node` is one singular attribute whose declared use is required;
/// `None` when its XML roles have no effective view.
pub(crate) fn attribute_required(node: &SchemaNode) -> Option<bool> {
    node.effective_xml_view().ok().map(|xml| {
        node.attribute
            && xml.occurrence()
                == XmlOccurrence::Single {
                    presence: EffectivePresence::Required,
                }
    })
}
