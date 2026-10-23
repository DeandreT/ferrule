//! Occurrence profiles every xsi:type view of one flattened member must share.

use ir::{SchemaKind, SchemaNode};

// A flattened member has one occurrence profile for every xsi:type view.
// Reject differing profiles before falling back from an otherwise supported
// derivation; dropping the alternatives would also lose their declared default.
pub(super) fn incompatible_attribute_use(left: &SchemaNode, right: &SchemaNode) -> Option<String> {
    if left.xml_attribute_required != right.xml_attribute_required {
        return Some(left.name.clone());
    }
    if let (
        SchemaKind::Group {
            children: left_children,
            ..
        },
        SchemaKind::Group {
            children: right_children,
            ..
        },
    ) = (&left.kind, &right.kind)
    {
        for left_child in left_children {
            if let Some(right_child) = right_children
                .iter()
                .find(|child| child.name == left_child.name)
                && let Some(path) = incompatible_attribute_use(left_child, right_child)
            {
                return Some(format!("{}/{}", left.name, path));
            }
        }
    }
    None
}

pub(super) fn incompatible_optional_occurrence(
    left: &SchemaNode,
    right: &SchemaNode,
) -> Option<String> {
    if left.xml_optional != right.xml_optional {
        return Some(left.name.clone());
    }
    if let (
        SchemaKind::Group {
            children: left_children,
            ..
        },
        SchemaKind::Group {
            children: right_children,
            ..
        },
    ) = (&left.kind, &right.kind)
    {
        for left_child in left_children {
            if let Some(right_child) = right_children
                .iter()
                .find(|child| child.name == left_child.name)
                && let Some(path) = incompatible_optional_occurrence(left_child, right_child)
            {
                return Some(format!("{}/{}", left.name, path));
            }
        }
    }
    None
}
