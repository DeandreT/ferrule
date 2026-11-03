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

/// Optional Count work has its own sticky quota. It never enables declared
/// libraries and its exhaustion must not alter ordinary registration.
#[derive(Clone, Debug)]
#[cfg_attr(not(test), derive(Default))]
pub(in crate::import) struct CountConstruction(ConstructionBudget);
#[cfg(test)]
impl Default for CountConstruction {
    fn default() -> Self {
        #[cfg(test)]
        if let Some((work, text)) =
            crate::import::aggregate::filtered_count::tests::construction_limits()
        {
            return Self::for_test(work, text);
        }
        Self(ConstructionBudget::default())
    }
}

impl CountConstruction {
    pub(in crate::import) fn claim(
        &self,
        work: usize,
        text: usize,
        depth: usize,
    ) -> Result<(), String> {
        self.0.claim(work, text, depth).map_err(|_| {
            crate::import::aggregate::filtered_count::count_visit(
                crate::import::aggregate::filtered_count::Counter::QuotaFailure,
            );
            "physical filtered count construction quota exhausted".into()
        })
    }
    pub(super) fn budget(&self) -> &ConstructionBudget {
        &self.0
    }
    #[cfg(test)]
    pub(in crate::import) fn for_test(work: usize, text: usize) -> Self {
        Self(ConstructionBudget::for_test(work, text))
    }
    #[cfg(test)]
    pub(in crate::import) fn remaining_for_test(&self) -> (usize, usize) {
        self.0.remaining_for_test()
    }
}

#[derive(Clone, Default)]
pub(in crate::import) enum OriginCapture {
    #[default]
    Existing,
    Count(CountConstruction),
}

/// Values keep their reader-assigned origin in an aligned, append-only slot.
/// Mutable access is slice-only: registration never exposes the backing Vec.
/// Import refinements may edit values but must not reorder registration slots.
pub(in crate::import) struct RegisteredVec<T> {
    values: Vec<T>,
    origins: Vec<Origin>,
    current: Origin,
    budget: Option<ConstructionBudget>,
    count_capture: OriginCapture,
    count_origins: Vec<Origin>,
    count_complete: bool,
}

impl<T> RegisteredVec<T> {
    pub(in crate::import) fn new(registry: &Registry) -> Self {
        Self::with_origin_capture(registry, OriginCapture::Existing)
    }
    pub(in crate::import) fn with_origin_capture(
        registry: &Registry,
        capture: OriginCapture,
    ) -> Self {
        Self {
            values: Vec::new(),
            origins: Vec::new(),
            current: Origin::SyntheticOrUnproved,
            budget: registry.construction.clone(),
            count_capture: capture,
            count_origins: Vec::new(),
            count_complete: true,
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
        if let OriginCapture::Count(budget) = &self.count_capture {
            if self.count_complete && budget.claim(2, 0, 0).is_ok() {
                self.count_origins.push(origin);
                crate::import::aggregate::filtered_count::count_visit(
                    crate::import::aggregate::filtered_count::Counter::Origin,
                );
            } else {
                self.count_complete = false;
            }
        }
        self.values.push(value);
    }
    pub(in crate::import) fn count_origin_at(&self, index: usize) -> Origin {
        if !self.count_complete {
            return Origin::SyntheticOrUnproved;
        }
        self.count_origins
            .get(index)
            .copied()
            .unwrap_or(Origin::SyntheticOrUnproved)
    }
    pub(super) fn count_origin_of(
        &self,
        value: &T,
        budget: &ConstructionBudget,
    ) -> Result<Origin, String> {
        budget.claim(self.values.len(), 0, 0)?;
        for _ in &self.values {
            crate::import::aggregate::filtered_count::count_visit(
                crate::import::aggregate::filtered_count::Counter::OriginLookup,
            );
        }
        Ok(self
            .values
            .iter()
            .position(|candidate| std::ptr::eq(candidate, value))
            .map(|index| self.count_origin_at(index))
            .unwrap_or(Origin::SyntheticOrUnproved))
    }
    pub(in crate::import) fn count_origin_of_for_count(
        &self,
        value: &T,
        budget: &CountConstruction,
    ) -> Result<Origin, String> {
        self.count_origin_of(value, budget.budget())
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_count_exhaustion_never_drops_values_or_misaligns_either_origin_ledger() {
        let doc = roxmltree::Document::parse("<component/>").unwrap();
        let owner = Origin::ActiveMain(RawOwner::of(doc.root_element()));
        for udf in [false, true] {
            let registry = Registry {
                construction: udf.then(ConstructionBudget::default),
                ..Registry::default()
            };
            let count = CountConstruction::for_test(2, 0);
            let mut values =
                RegisteredVec::with_origin_capture(&registry, OriginCapture::Count(count.clone()));
            values.push_origin(1, owner);
            assert_eq!(values.count_origin_at(0), owner);
            values.push_origin(2, Origin::SyntheticOrUnproved);
            values.push_origin(3, owner);
            assert_eq!(&*values, &[1, 2, 3]);
            assert_eq!(values.count_origin_at(0), Origin::SyntheticOrUnproved);
            assert_eq!(values.count_origin_at(2), Origin::SyntheticOrUnproved);
            assert!(count.claim(0, 0, 0).is_err());
            if udf {
                assert_eq!(values.origin_at(0), owner);
                assert_eq!(values.origin_at(2), owner);
                assert!(
                    registry
                        .construction
                        .as_ref()
                        .unwrap()
                        .claim(0, 0, 0)
                        .is_ok()
                );
            } else {
                assert!(registry.construction.is_none());
            }
        }
    }
}
