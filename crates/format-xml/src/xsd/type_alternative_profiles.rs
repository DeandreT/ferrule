//! Occurrence profiles every xsi:type view of one flattened member must share.

use ir::{SchemaKind, SchemaNode};

use crate::XmlFormatError;

// A flattened member has one occurrence profile for every xsi:type view.
// Reject differing profiles before falling back from an otherwise supported
// derivation; dropping the alternatives would also lose their declared default.
pub(super) fn incompatible_attribute_use(left: &SchemaNode, right: &SchemaNode) -> Option<String> {
    if !same_profile(left, right, crate::declaration::required_attribute) {
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
    if !same_profile(left, right, crate::declaration::optional_element) {
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

/// Both declarations derive the same fact; a declaration without an effective
/// XML view never shares a profile.
fn same_profile(
    left: &SchemaNode,
    right: &SchemaNode,
    fact: fn(&SchemaNode) -> Result<bool, XmlFormatError>,
) -> bool {
    matches!((fact(left), fact(right)), (Ok(left), Ok(right)) if left == right)
}
