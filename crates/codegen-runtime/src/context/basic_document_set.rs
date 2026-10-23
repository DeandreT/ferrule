//! Owned scopes over one document-set endpoint in the paired Basic context.
//!
//! An ungrouped owned scope selecting a document-set endpoint with no active
//! document walks each member in set order. The member's document roots its
//! owner, with the member's location, and the selector path is walked from it
//! as from a supplied root. Documents are not positions; nested scopes and
//! reads of the same owner see the current document and never re-enter the
//! set, while other owners keep their own roots and documents.

use std::sync::{Arc, OnceLock};

use super::{
    BasicScopeContext, BasicSourceCandidate, OwnedSourceFrame, OwnedSourceLimitKind,
    RetainedSourceCohort, SourceOwnerRef, WalkCohort, owned_charge,
};
use crate::{Instance, RuntimeError, Value};

impl<'ctx> BasicScopeContext<'ctx> {
    /// The location of the selected owner's latest active document-set
    /// member; another owner's document never supplies it.
    pub fn owned_document_path(&self, owner: SourceOwnerRef<'_>) -> Result<Value, RuntimeError> {
        self.budget(owner, &[], None, false)?;
        self.owned.owned_document_path(owner).map_err(Into::into)
    }

    /// Walks an owned scope selector like [`Self::walk_owned_source_cohort`],
    /// except that a document-set endpoint with no active document walks each
    /// member as its owner's root, in set order, as one cohort of this view.
    pub fn walk_owned_document_source_cohort(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
    ) -> Result<RetainedSourceCohort<'ctx>, RuntimeError> {
        let loading = self
            .loading
            .as_ref()
            .is_some_and(|loading| loading.owns(&self.owned.registry, owner));
        if loading {
            return self.walk_owned_source_cohort(owner, path);
        }
        self.budget(owner, path, None, false)?;
        let (key, root) = self.owned.resolve_owned_owner(owner)?;
        let Instance::DocumentSet(members) = root else {
            return self.walk_owned_source_cohort(owner, path);
        };
        if self.owned.document_frame(key).is_some() || self.owned.owned_base(key, path).is_some() {
            return self.walk_owned_source_cohort(owner, path);
        }
        let limits = self.owned.registry.limits;
        let cohort = Arc::new(WalkCohort {
            parent: Arc::clone(&self.identity),
            registry: Arc::clone(&self.owned.registry),
            original_count: OnceLock::new(),
        });
        let mut candidates = Vec::new();
        for member in members {
            let mut context = self.try_clone_view()?;
            owned_charge(
                OwnedSourceLimitKind::Frames,
                context.owned.frames.len(),
                1,
                limits.max_active_frames,
            )?;
            context.owned.frames.push(OwnedSourceFrame {
                owner: key,
                collection: Vec::new(),
                instance: member.value(),
                index: 0,
                kind: super::super::OwnedOccurrenceKind::Document {
                    location: Some(member.source_path()),
                },
            });
            for candidate in context.walk_owned_source(owner, path)? {
                owned_charge(
                    OwnedSourceLimitKind::WalkContexts,
                    candidates.len(),
                    1,
                    limits.max_walk_contexts,
                )?;
                candidates.push(BasicSourceCandidate {
                    cohort: Arc::clone(&cohort),
                    ordinal: candidates.len(),
                    ..candidate
                });
            }
        }
        cohort
            .original_count
            .set(candidates.len())
            .expect("one completed document-set walk");
        Ok(RetainedSourceCohort {
            candidates,
            witness: cohort,
        })
    }
}
