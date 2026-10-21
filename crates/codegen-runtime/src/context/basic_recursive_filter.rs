//! Paired frames for recursive filtering of an explicitly owned selector.
//!
//! The legacy view stacks one frame per visited level exactly like the legacy
//! filter. The owned view keeps the scope's own frames and adds one same-shape
//! occurrence of the selector's owner: a newer occurrence with the same owner
//! and collection shadows the older one, so only the innermost is retained.

use super::*;

impl<'ctx> BasicScopeContext<'ctx> {
    /// The current group of a scope iterating `collection` of `owner`: that
    /// owner's latest exact occurrence, or its registry root for an endpoint
    /// root, which is an identity selection without an occurrence.
    pub(crate) fn owned_recursive_filter_root(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: &[&str],
    ) -> Result<(OwnedOwnerKey, &'ctx Instance), RuntimeError> {
        self.budget(owner, collection, None, false)?;
        let (key, root) = self.owned.resolve_owned_owner(owner)?;
        match self.owned.owned_frame_index(key, Some(collection)) {
            Some(index) => Ok((key, self.owned.frames[index].instance)),
            None if collection.is_empty() => Ok((key, root)),
            None => Err(RuntimeError::RecursiveFilterRequiresGroup {
                found: "missing context",
            }),
        }
    }

    pub(crate) fn owned_frame_count(&self) -> usize {
        self.owned.frames.len()
    }

    /// One recursive level: stack a legacy `legacy_collection` frame and keep
    /// the first `retain` owned frames before adding `owner`'s occurrence of
    /// `item` at its one-based `index` in `collection`.
    pub(crate) fn with_owned_recursive_frame(
        &self,
        retain: usize,
        owner: OwnedOwnerKey,
        collection: &[String],
        item: &'ctx Instance,
        legacy_collection: &str,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        let (mut budget, bounds) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
        length_limit("legacy frames", self.legacy.frames.len(), 1, bounds.legacy)?;
        let retained = self
            .owned
            .frames
            .get(..retain)
            .ok_or(BasicSourceBridgeError::RegistryMismatch)?;
        owned_charge(
            OwnedSourceLimitKind::Frames,
            0,
            add(retained.len(), 1)?,
            self.owned.registry.limits.max_active_frames,
        )?;
        budget.slots(2)?;
        let mut legacy = self.legacy_copy(&mut budget)?;
        let origins = self.origins_copy(&mut budget)?;
        grow(&mut legacy.frames, 1, &mut budget)?;
        let legacy_path = budget.path(&[legacy_collection.to_owned()])?;
        legacy.frames.push(super::super::collection_frame(
            item,
            CollectionIdentity::Repeated {
                path: legacy_path,
                index,
            },
        ));
        budget.slots(add(retained.len(), 1)?)?;
        owned_frames_bytes(retained, &mut budget)?;
        let owned_path = budget.path(collection)?;
        let mut frames = Vec::with_capacity(add(retained.len(), 1)?);
        frames.extend_from_slice(retained);
        frames.push(OwnedSourceFrame {
            owner,
            collection: owned_path,
            instance: item,
            index,
            kind: super::super::OwnedOccurrenceKind::Item,
        });
        Ok(Self {
            legacy,
            owned: OwnedScopeContext {
                registry: Arc::clone(&self.owned.registry),
                frames,
            },
            origins,
            bindings: self.bindings,
            projection_bytes: self.projection_bytes,
            identity: Arc::new(()),
        })
    }
}
