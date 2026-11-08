//! Schema shape queries and named traversal.

use crate::{ScalarType, SchemaKind, SchemaNode};

impl SchemaNode {
    pub fn is_scalar(&self) -> bool {
        matches!(
            self.kind,
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }
        )
    }

    /// Whether this scalar domain contains exactly String and Int.
    /// Nullability and repetition remain independent occurrence metadata.
    pub fn is_string_int_union(&self) -> bool {
        matches!(
            self.kind,
            SchemaKind::ScalarUnion { types }
                if types.contains(ScalarType::String)
                    && types.contains(ScalarType::Int)
                    && types.iter().count() == 2
        )
    }

    /// Whether this node admits exactly String and finite Float values.
    pub fn is_string_float_union(&self) -> bool {
        matches!(
            self.kind,
            SchemaKind::ScalarUnion { types }
                if types.contains(ScalarType::String)
                    && types.contains(ScalarType::Float)
                    && types.iter().count() == 2
        )
    }

    pub fn accepts_scalar_type(&self, ty: ScalarType) -> bool {
        match self.kind {
            SchemaKind::Scalar { ty: expected } => expected == ty,
            SchemaKind::ScalarUnion { types } => types.contains(ty),
            SchemaKind::Group { .. } => false,
        }
    }

    pub fn child(&self, name: &str) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children, .. } => children.iter().find(|c| c.name == name),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
        }
    }

    /// Follows `path` one named child at a time, taking the first child with
    /// each name. Recursive references are not resolved.
    pub fn descendant(&self, path: &[String]) -> Option<&SchemaNode> {
        path.iter()
            .try_fold(self, |node, segment| node.child(segment))
    }

    /// Finds the first concrete group named `anchor` in depth-first,
    /// declaration order, starting with this node. Recursive-reference nodes
    /// never match, although their children are still searched.
    pub fn find_concrete_group(&self, anchor: &str) -> Option<&SchemaNode> {
        if self.recursive_ref.is_none()
            && self.name == anchor
            && matches!(self.kind, SchemaKind::Group { .. })
        {
            return Some(self);
        }
        let SchemaKind::Group { children, .. } = &self.kind else {
            return None;
        };
        children
            .iter()
            .find_map(|child| child.find_concrete_group(anchor))
    }

    pub fn text_child(&self) -> Option<&SchemaNode> {
        match &self.kind {
            SchemaKind::Group { children, .. } => children.iter().find(|child| child.text),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
        }
    }
}
