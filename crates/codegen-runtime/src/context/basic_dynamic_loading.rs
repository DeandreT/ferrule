//! Owned dynamic driver loading in the paired Basic context.
//!
//! A loading scope walks the declared driver collection of exactly its
//! endpoint, evaluates the logical path once per driver row in driver order
//! and requests every document before its own walk. Each loaded document is
//! paired with only the driver row that requested it and roots its owner,
//! so nested loading and owned reads never borrow another driver, document or
//! endpoint. Generated code never caches: every reached path is one callback.

use std::sync::{Arc, OnceLock};

use super::{
    BasicScopeContext, BasicSourceCandidate, OwnedOwnerKey, OwnedSourceError, OwnedSourceFrame,
    OwnedSourceLimitKind, PhaseCursor, SourceOwnerRef, SourceRootRegistry, WalkCohort,
    owned_charge,
};
use crate::dynamic_source::{MAX_DYNAMIC_SOURCE_LOADS, MAX_DYNAMIC_SOURCE_PATH_BYTES};
use crate::{Instance, RuntimeError, Value};

/// Every document one loading scope requested, each with its driver row.
pub struct OwnedDynamicDocuments<'ctx> {
    key: OwnedOwnerKey,
    drivers: Vec<BasicSourceCandidate<'ctx>>,
    /// Each document with its driver row and the logical path it was loaded by.
    documents: Vec<(usize, String, Instance)>,
}

/// The loaded set attached to a loading scope's receiving view.
pub(super) struct LoadedDocuments<'ctx> {
    key: OwnedOwnerKey,
    entries: Vec<LoadedEntry<'ctx>>,
}

struct LoadedEntry<'ctx> {
    /// The driver row's view with its document as the owner's root.
    context: BasicScopeContext<'ctx>,
    /// The driver walk's newest repeated cursor, if the driver repeats.
    cursor: Option<PhaseCursor>,
}

impl LoadedDocuments<'_> {
    pub(super) fn owns(
        &self,
        registry: &SourceRootRegistry<'_>,
        owner: SourceOwnerRef<'_>,
    ) -> bool {
        match (self.key, owner) {
            (OwnedOwnerKey::Named(index), SourceOwnerRef::Named(name)) => registry
                .named
                .get(index)
                .is_some_and(|declaration| declaration.name == name),
            _ => false,
        }
    }
}

impl<'ctx> BasicScopeContext<'ctx> {
    /// Requests one owned dynamic endpoint's documents for every row of its
    /// exact driver collection, in driver order, before any is walked.
    ///
    /// A missing loader fails before the driver walk, even for an empty
    /// collection. Absent and null paths skip their driver; other non-string
    /// paths, oversized paths and excessive drivers retain their typed
    /// dynamic-source failures, and each host failure keeps its source and path.
    pub fn load_owned_dynamic_documents(
        &self,
        source: &'static str,
        driver: SourceOwnerRef<'_>,
        driver_path: &[&str],
        node: u32,
        mut path: impl FnMut(&BasicScopeContext<'ctx>) -> Result<Value, RuntimeError>,
    ) -> Result<OwnedDynamicDocuments<'ctx>, RuntimeError> {
        let loader = self
            .legacy
            .dynamic_source_loader()
            .ok_or(RuntimeError::MissingDynamicSourceLoader { source })?;
        let index = self
            .owned
            .registry
            .named
            .iter()
            .position(|declaration| declaration.name == source)
            .ok_or_else(|| OwnedSourceError::UnknownOwner {
                name: source.to_string(),
            })?;
        let drivers = self.walk_owned_source(driver, driver_path)?;
        if drivers.len() > MAX_DYNAMIC_SOURCE_LOADS {
            return Err(RuntimeError::DynamicSourceTooMany {
                source,
                maximum: MAX_DYNAMIC_SOURCE_LOADS,
            });
        }
        let mut documents = Vec::with_capacity(drivers.len());
        for (index, candidate) in drivers.iter().enumerate() {
            let logical_path = match path(candidate.context())? {
                Value::Null | Value::JsonNull(_) => continue,
                Value::String(path) => path,
                value => {
                    return Err(RuntimeError::DynamicSourcePath {
                        source,
                        node,
                        found: value.type_name(),
                    });
                }
            };
            if logical_path.len() > MAX_DYNAMIC_SOURCE_PATH_BYTES {
                return Err(RuntimeError::DynamicSourcePathTooLong {
                    source,
                    maximum: MAX_DYNAMIC_SOURCE_PATH_BYTES,
                });
            }
            let document = loader
                .load_tagged(source, &logical_path)
                .map_err(|fault| fault.into_runtime_error(source, logical_path.clone()))?;
            documents.push((index, logical_path, document));
        }
        Ok(OwnedDynamicDocuments {
            key: OwnedOwnerKey::Named(index),
            drivers,
            documents,
        })
    }

    /// One walk of every loaded document of `loading`'s owner, in driver order,
    /// as a single cohort of this receiving view.
    pub(super) fn walk_loaded_documents(
        &self,
        loading: &LoadedDocuments<'ctx>,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
    ) -> Result<(Vec<BasicSourceCandidate<'ctx>>, Arc<WalkCohort<'ctx>>), RuntimeError> {
        let cohort = Arc::new(WalkCohort {
            parent: Arc::clone(&self.identity),
            registry: Arc::clone(&self.owned.registry),
            original_count: OnceLock::new(),
        });
        let mut rows = Vec::new();
        for entry in &loading.entries {
            for candidate in entry.context.walk_owned_source(owner, path)? {
                owned_charge(
                    OwnedSourceLimitKind::WalkContexts,
                    rows.len(),
                    1,
                    self.owned.registry.limits.max_walk_contexts,
                )?;
                // The candidate's newest repetition is its document walk's
                // own, or else its driver row's.
                rows.push(BasicSourceCandidate {
                    cohort: Arc::clone(&cohort),
                    ordinal: rows.len(),
                    cursor: candidate.cursor.or(entry.cursor),
                    ..candidate
                });
            }
        }
        cohort
            .original_count
            .set(rows.len())
            .expect("one completed loaded walk");
        Ok((rows, cohort))
    }
}

impl<'ctx> OwnedDynamicDocuments<'ctx> {
    /// The receiving view of the loading scope. Its walk of the loaded owner
    /// reads every document under its own driver row; parent expressions keep
    /// the unchanged receiving context.
    pub fn loading_context<'b>(
        &'b self,
        parent: &BasicScopeContext<'b>,
    ) -> Result<BasicScopeContext<'b>, RuntimeError>
    where
        'ctx: 'b,
    {
        let mut entries = Vec::with_capacity(self.documents.len());
        for (index, path, document) in &self.documents {
            let driver = &self.drivers[*index];
            let mut context: BasicScopeContext<'b> = driver.context().try_clone_view()?;
            owned_charge(
                OwnedSourceLimitKind::Frames,
                context.owned.frames.len(),
                1,
                context.owned.registry.limits.max_active_frames,
            )?;
            context.owned.frames.push(OwnedSourceFrame {
                owner: self.key,
                collection: Vec::new(),
                instance: document,
                index: 0,
                kind: super::super::OwnedOccurrenceKind::Document { location: path },
            });
            entries.push(LoadedEntry {
                context,
                cursor: driver.cursor,
            });
        }
        let mut loading = parent.try_clone_view()?;
        loading.loading = Some(std::rc::Rc::new(LoadedDocuments {
            key: self.key,
            entries,
        }));
        Ok(loading)
    }
}

#[cfg(test)]
#[path = "basic_dynamic_loading_tests.rs"]
mod tests;
