use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};

use crate::NamedSourceProgram;

use super::ProgramValidationError;

pub(super) fn validate_names(sources: &[NamedSourceProgram]) -> Result<(), ProgramValidationError> {
    let mut names = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        let name = source.name.trim();
        if name.is_empty() {
            return Err(ProgramValidationError::EmptyExtraSourceName { index });
        }
        if let Some(&first) = names.get(name) {
            return Err(ProgramValidationError::DuplicateExtraSourceName {
                name: name.to_string(),
                first,
                duplicate: index,
            });
        }
        names.insert(name, index);
    }
    Ok(())
}

/// One schema node paired with the document root that owns its recursive refs.
#[derive(Clone, Copy)]
pub(super) struct SchemaCursor<'a> {
    root: &'a SchemaNode,
    node: &'a SchemaNode,
}

impl<'a> SchemaCursor<'a> {
    fn new(root: &'a SchemaNode, node: &'a SchemaNode) -> Self {
        Self { root, node }
    }

    pub(super) fn node(self) -> &'a SchemaNode {
        self.node
    }

    pub(super) fn follow(self, path: &[String]) -> Option<Self> {
        follow_from(self.root, self.node, path).map(|node| Self::new(self.root, node))
    }

    pub(super) fn follow_direct(self, path: &[String]) -> Option<Self> {
        follow_direct(self.root, self.node, path).map(|node| Self::new(self.root, node))
    }

    pub(super) fn resolved(self) -> Option<Self> {
        let Some(anchor) = self.node.recursive_ref.as_deref() else {
            return Some(self);
        };
        find_concrete_group(self.root, anchor).map(|node| Self::new(self.root, node))
    }
}

/// All source schemas visible to one neutral program, in engine fallback order.
#[derive(Clone, Copy)]
pub(super) struct SourceCatalog<'a> {
    primary: &'a SchemaNode,
    extras: &'a [NamedSourceProgram],
}

impl<'a> SourceCatalog<'a> {
    pub(super) fn new(primary: &'a SchemaNode, extras: &'a [NamedSourceProgram]) -> Self {
        Self { primary, extras }
    }

    pub(super) fn primary(self) -> SchemaCursor<'a> {
        SchemaCursor::new(self.primary, self.primary)
    }

    /// Resolves an ordinary collection from an explicit document root without
    /// recursive descendant fallback.
    pub(super) fn root_schema_at(self, path: &[String]) -> Option<SchemaCursor<'a>> {
        self.explicit_target(path, false)
            .or_else(|| self.primary().follow(path))
    }

    /// Resolves a scope iteration like the engine: current frame, explicit
    /// named source, primary fallback, then named sources in declaration order.
    pub(super) fn schema_at(
        self,
        parent: Option<SchemaCursor<'a>>,
        path: &[String],
    ) -> Option<SchemaCursor<'a>> {
        parent
            .and_then(|current| current.follow(path))
            .or_else(|| self.explicit_target(path, false))
            .or_else(|| first_path_target(self.primary, path, false))
            .or_else(|| {
                self.extras
                    .iter()
                    .find_map(|source| first_path_target(&source.source, path, false))
            })
    }

    pub(super) fn path_matches(
        self,
        path: &[String],
        predicate: impl Fn(&SchemaNode) -> bool,
    ) -> bool {
        self.path_targets(path)
            .into_iter()
            .any(|candidate| predicate(candidate.node))
    }

    pub(super) fn path_targets(self, path: &[String]) -> Vec<SchemaCursor<'a>> {
        let mut targets = Vec::new();
        if let Some(target) = self.explicit_target(path, false) {
            targets.push(target);
        }
        collect_path_targets(self.primary, self.primary, path, false, &mut targets);
        for source in self.extras {
            collect_path_targets(&source.source, &source.source, path, false, &mut targets);
        }
        targets
    }

    /// Lookup collections cannot cross another repeated boundary before their
    /// terminal collection.
    pub(super) fn direct_path_targets(self, path: &[String]) -> Vec<SchemaCursor<'a>> {
        let mut targets = Vec::new();
        if let Some(target) = self.explicit_target(path, true) {
            targets.push(target);
        }
        collect_path_targets(self.primary, self.primary, path, true, &mut targets);
        for source in self.extras {
            collect_path_targets(&source.source, &source.source, path, true, &mut targets);
        }
        targets
    }

    fn explicit_target(self, path: &[String], direct: bool) -> Option<SchemaCursor<'a>> {
        let (name, rest) = path.split_first()?;
        let source = self.extras.iter().find(|source| source.name == *name)?;
        let node = if direct {
            follow_direct(&source.source, &source.source, rest)
        } else {
            follow_from(&source.source, &source.source, rest)
        }?;
        Some(SchemaCursor::new(&source.source, node))
    }
}

fn first_path_target<'a>(
    root: &'a SchemaNode,
    path: &[String],
    direct: bool,
) -> Option<SchemaCursor<'a>> {
    let node = if direct {
        follow_direct(root, root, path)
    } else {
        follow_from(root, root, path)
    };
    node.map(|node| SchemaCursor::new(root, node))
        .or_else(|| match &root.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .find_map(|child| first_path_target_from(root, child, path, direct)),
            SchemaKind::Scalar { .. } => None,
        })
}

fn first_path_target_from<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
    direct: bool,
) -> Option<SchemaCursor<'a>> {
    let node = if direct {
        follow_direct(root, current, path)
    } else {
        follow_from(root, current, path)
    };
    node.map(|node| SchemaCursor::new(root, node))
        .or_else(|| match &current.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .find_map(|child| first_path_target_from(root, child, path, direct)),
            SchemaKind::Scalar { .. } => None,
        })
}

fn collect_path_targets<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
    direct: bool,
    targets: &mut Vec<SchemaCursor<'a>>,
) {
    let node = if direct {
        follow_direct(root, current, path)
    } else {
        follow_from(root, current, path)
    };
    if let Some(node) = node {
        targets.push(SchemaCursor::new(root, node));
    }
    if let SchemaKind::Group { children, .. } = &current.kind {
        for child in children {
            collect_path_targets(root, child, path, direct, targets);
        }
    }
}

fn follow_from<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = current;
    for segment in path {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_group(root, anchor)?;
        }
        current = current.child(segment)?;
    }
    Some(current)
}

fn follow_direct<'a>(
    root: &'a SchemaNode,
    current: &'a SchemaNode,
    path: &[String],
) -> Option<&'a SchemaNode> {
    let mut current = current;
    for (index, segment) in path.iter().enumerate() {
        if let Some(anchor) = &current.recursive_ref {
            current = find_concrete_group(root, anchor)?;
        }
        current = current.child(segment)?;
        if current.repeating && index + 1 != path.len() {
            return None;
        }
    }
    Some(current)
}

fn find_concrete_group<'a>(current: &'a SchemaNode, anchor: &str) -> Option<&'a SchemaNode> {
    if current.recursive_ref.is_none()
        && current.name == anchor
        && matches!(current.kind, SchemaKind::Group { .. })
    {
        return Some(current);
    }
    let SchemaKind::Group { children, .. } = &current.kind else {
        return None;
    };
    children
        .iter()
        .find_map(|child| find_concrete_group(child, anchor))
}

#[cfg(test)]
mod tests {
    use ir::ScalarType;

    use super::*;

    #[test]
    fn schema_at_uses_engine_fallback_order() {
        let primary = SchemaNode::group(
            "Primary",
            vec![
                SchemaNode::group(
                    "Current",
                    vec![SchemaNode::scalar("Value", ScalarType::Bool)],
                ),
                SchemaNode::scalar("Shared", ScalarType::String),
            ],
        );
        let extras = vec![
            NamedSourceProgram {
                name: "Catalog".into(),
                source: SchemaNode::group(
                    "CatalogDocument",
                    vec![
                        SchemaNode::scalar("OnlyExtra", ScalarType::Int),
                        SchemaNode::scalar("Shared", ScalarType::Float),
                    ],
                ),
            },
            NamedSourceProgram {
                name: "Other".into(),
                source: SchemaNode::group(
                    "OtherDocument",
                    vec![SchemaNode::scalar("OnlyExtra", ScalarType::Bool)],
                ),
            },
        ];
        let sources = SourceCatalog::new(&primary, &extras);
        let current = sources.schema_at(None, &["Current".into()]);
        let Some(current) = current else {
            panic!("primary current scope exists");
        };

        let relative = sources.schema_at(Some(current), &["Value".into()]);
        assert!(matches!(
            relative.map(|cursor| &cursor.node().kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Bool
            })
        ));
        let explicit = sources.schema_at(None, &["Catalog".into(), "Shared".into()]);
        assert!(matches!(
            explicit.map(|cursor| &cursor.node().kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Float
            })
        ));
        let primary_fallback = sources.schema_at(None, &["Shared".into()]);
        assert!(matches!(
            primary_fallback.map(|cursor| &cursor.node().kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::String
            })
        ));
        let extra_fallback = sources.schema_at(None, &["OnlyExtra".into()]);
        assert!(matches!(
            extra_fallback.map(|cursor| &cursor.node().kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Int
            })
        ));
    }
}
