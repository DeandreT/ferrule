use std::collections::BTreeSet;

use crate::{Instance, SchemaKind, SchemaNode};

/// The closed, nonempty scalar-record schema shared by flat row documents.
/// Repetition belongs to the document value, not the row schema.
pub fn flat_row_schema_is_supported(schema: &SchemaNode) -> bool {
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    else {
        return false;
    };
    let mut names = BTreeSet::new();
    !schema.repeating
        && schema.recursive_ref.is_none()
        && alternatives.is_empty()
        && dynamic.is_none()
        && !children.is_empty()
        && children.iter().all(|child| {
            !child.name.is_empty()
                && names.insert(&child.name)
                && !child.repeating
                && child.recursive_ref.is_none()
                && matches!(child.kind, SchemaKind::Scalar { .. })
        })
}

/// Whether a value is one flat row, without coercing or dropping any field.
pub fn is_flat_row(value: &Instance) -> bool {
    matches!(value, Instance::Group(fields) if fields.iter().all(|(_, value)| matches!(value, Instance::Scalar(_))))
}
