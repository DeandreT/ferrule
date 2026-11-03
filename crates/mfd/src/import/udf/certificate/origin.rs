use std::ops::{Deref, DerefMut};

use super::super::{Registry, limits::ConstructionBudget};
use crate::import::schema::SchemaComponent;

/// Session-local immutable XML identity. Never serialized, dereferenced as a
/// pointer, or compared across imports. Main XML remains alive through lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::import) struct RawOwner {
    document: usize,
    structure: (usize, usize),
    component: (usize, usize),
}

impl RawOwner {
    pub(super) fn contains_pin(self, range: (usize, usize)) -> bool {
        range.0 < range.1 && self.component.0 <= range.0 && range.1 <= self.component.1
    }
    pub(in crate::import) fn of(node: roxmltree::Node<'_, '_>) -> Self {
        let scope = node
            .ancestors()
            .find(|n| n.is_element() && n.tag_name().name() == "structure");
        let range = node.range();
        let scope = scope.map(|n| n.range()).unwrap_or(0..0);
        Self {
            document: std::ptr::from_ref(node.document()) as usize,
            structure: (scope.start, scope.end),
            component: (range.start, range.end),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::import) enum Origin {
    ActiveMain(RawOwner),
    PromotedLocalCatalog(RawOwner),
    SyntheticOrUnproved,
}

/// Values keep their reader-assigned origin in an aligned, append-only slot.
/// Mutable access is slice-only: registration never exposes the backing Vec.
/// Import refinements may edit values but must not reorder registration slots.
pub(in crate::import) struct RegisteredVec<T> {
    values: Vec<T>,
    origins: Vec<Origin>,
    current: Origin,
    budget: Option<ConstructionBudget>,
}

impl<T> RegisteredVec<T> {
    pub(in crate::import) fn new(registry: &Registry) -> Self {
        Self {
            values: Vec::new(),
            origins: Vec::new(),
            current: Origin::SyntheticOrUnproved,
            budget: registry.construction.clone(),
        }
    }
    pub(in crate::import) fn set_active(&mut self, component: roxmltree::Node<'_, '_>) {
        self.current = Origin::ActiveMain(RawOwner::of(component));
    }
    pub(in crate::import) fn push(&mut self, value: T) {
        self.push_origin(value, self.current);
    }
    pub(in crate::import) fn push_origin(&mut self, value: T, origin: Origin) {
        if let Some(budget) = &self.budget {
            if budget.claim(2, 0, 0).is_err() {
                return;
            }
            self.origins.push(origin);
        }
        self.values.push(value);
    }
    pub(in crate::import) fn origin_at(&self, index: usize) -> Origin {
        self.origins
            .get(index)
            .copied()
            .unwrap_or(Origin::SyntheticOrUnproved)
    }
    pub(super) fn origin_of(
        &self,
        value: &T,
        budget: &ConstructionBudget,
    ) -> Result<Origin, String> {
        budget.claim(self.values.len(), 0, 0)?;
        Ok(self
            .values
            .iter()
            .position(|candidate| std::ptr::eq(candidate, value))
            .map(|index| self.origin_at(index))
            .unwrap_or(Origin::SyntheticOrUnproved))
    }
}
impl<T> Deref for RegisteredVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> DerefMut for RegisteredVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}
impl<'a, T> IntoIterator for &'a RegisteredVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

pub(in crate::import) trait SchemaSink:
    Deref<Target = [SchemaComponent]>
{
    fn push_synthetic(&mut self, component: SchemaComponent);
}
impl SchemaSink for Vec<SchemaComponent> {
    fn push_synthetic(&mut self, component: SchemaComponent) {
        self.push(component);
    }
}
impl SchemaSink for RegisteredVec<SchemaComponent> {
    fn push_synthetic(&mut self, component: SchemaComponent) {
        self.push_origin(component, Origin::SyntheticOrUnproved);
    }
}

pub(in crate::import) struct CatalogSource {
    pub(in crate::import) component: SchemaComponent,
    pub(in crate::import) origin: Origin,
}
impl CatalogSource {
    pub(in crate::import) fn new(component: SchemaComponent, raw: roxmltree::Node<'_, '_>) -> Self {
        Self {
            component,
            origin: Origin::PromotedLocalCatalog(RawOwner::of(raw)),
        }
    }
}
