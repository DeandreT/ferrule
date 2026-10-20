//! Structural scope identities shared by closed ownership boundaries.
use crate::{Project, Scope, ScopeConstruction, SourceSelectorRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceScopeTarget {
    Primary,
    Named { index: usize, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceScopeStep {
    Child(usize),
    DynamicChild(usize),
    Segment(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScopeLocation {
    pub target: SourceScopeTarget,
    pub path: Vec<SourceScopeStep>,
}

impl SourceScopeLocation {
    pub fn root(target: SourceScopeTarget) -> Self {
        Self {
            target,
            path: Vec::new(),
        }
    }

    pub fn descendant(&self, step: SourceScopeStep) -> Self {
        let mut location = self.clone();
        location.path.push(step);
        location
    }
}

/// Finds the first owned selector in stable structural order, without schema
/// resolution, expression traversal, acquisition, or execution. An owned
/// path-hierarchy input collection is a scope-owned selector too.
pub fn first_owned_source_scope(
    root: &Scope,
    location: SourceScopeLocation,
) -> Option<SourceScopeLocation> {
    let mut pending = vec![(root, location)];
    while let Some((scope, location)) = pending.pop() {
        if matches!(scope.source_selector(), Some(SourceSelectorRef::Owned(_)))
            || matches!(
                scope.construction,
                ScopeConstruction::OwnedPathHierarchy { .. }
            )
        {
            return Some(location);
        }
        for (index, child) in scope.dynamic_children.iter().enumerate().rev() {
            pending.push((
                &child.scope,
                location.descendant(SourceScopeStep::DynamicChild(index)),
            ));
        }
        for (index, child) in scope.children.iter().enumerate().rev() {
            pending.push((child, location.descendant(SourceScopeStep::Child(index))));
        }
        if let Some(segments) = scope.concatenated() {
            let segments: Vec<_> = segments.iter().collect();
            for (index, segment) in segments.into_iter().enumerate().rev() {
                pending.push((
                    segment,
                    location.descendant(SourceScopeStep::Segment(index)),
                ));
            }
        }
    }
    None
}

/// Export and generation represent every declared target scope.
pub fn first_project_owned_source_scope(project: &Project) -> Option<SourceScopeLocation> {
    first_owned_source_scope(
        &project.root,
        SourceScopeLocation::root(SourceScopeTarget::Primary),
    )
    .or_else(|| {
        project
            .extra_targets
            .iter()
            .enumerate()
            .find_map(|(index, target)| {
                first_owned_source_scope(
                    &target.root,
                    SourceScopeLocation::root(SourceScopeTarget::Named {
                        index,
                        name: target.name.clone(),
                    }),
                )
            })
    })
}
