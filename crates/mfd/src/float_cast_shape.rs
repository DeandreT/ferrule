//! Shared model-shape boundary for the qualified native float-text cast.

use ir::{SchemaKind, SchemaNode};
use mapping::Scope;

pub(crate) fn plain_scope(scope: &Scope) -> bool {
    let mut shell = scope.clone();
    shell.bindings.clear();
    shell == Scope::default()
}

pub(crate) fn flat_schema(schema: &SchemaNode, target: bool) -> bool {
    fn plain(node: &SchemaNode) -> bool {
        !node.attribute
            && !node.text
            && node.fixed.is_none()
            && node.default.is_none()
            && node.value_generation.is_none()
            && node.recursive_ref.is_none()
            && node.xml_name_alternatives.is_empty()
            && node
                .xml_namespace
                .as_ref()
                .is_none_or(|namespace| matches!(namespace, ir::XmlNamespace::Unqualified))
            && node.xml_wildcard_namespace.is_none()
            && !node.xml_type_alternatives
            && node.xml_repeating_sequences.is_empty()
            && node.xml_repeating_choices.is_empty()
    }
    let SchemaKind::Group {
        children,
        alternatives,
        xml_restricted_alternatives,
        dynamic,
        ..
    } = &schema.kind
    else {
        return false;
    };
    !schema.repeating
        && plain(schema)
        && alternatives.is_empty()
        && xml_restricted_alternatives.is_empty()
        && dynamic.is_none()
        && children.iter().all(|child| {
            plain(child)
                && !child.nillable
                && (target || !child.repeating)
                && matches!(child.kind, SchemaKind::Scalar { .. })
        })
}
