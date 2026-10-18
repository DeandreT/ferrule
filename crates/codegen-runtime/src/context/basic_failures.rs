//! Authenticated primary-row global failures. This route never returns its
//! private cohort or publishes a specialized context to an expression callback.

use super::*;

fn unsupported() -> RuntimeError {
    BasicSourceBridgeError::UnsupportedPrimaryFailureContext.into()
}

impl<'ctx> BasicScopeContext<'ctx> {
    /// Acquire one complete bounded legacy row walk from the actual Primary
    /// Group root, then visit rows in raw structural order. A selected callback
    /// normally returns a mapping failure; later rows/messages stay unevaluated.
    /// Count and metadata admission finishes before the first callback. This
    /// operation has no sorting, filtering, windows or position compaction.
    pub fn visit_primary_failure_rows<E, F>(&self, path: &[&str], mut row: F) -> Result<(), E>
    where
        E: From<RuntimeError>,
        F: FnMut(&BasicScopeContext<'ctx>) -> Result<(), E>,
    {
        let candidates = self.primary_failure_rows(path).map_err(E::from)?;
        for candidate in &candidates {
            let terminal = candidate
                .context
                .legacy
                .frames
                .last()
                .expect("observed row frame")
                .instance;
            supported(terminal).map_err(E::from)?;
            row(candidate.context())?;
        }
        Ok(())
    }

    fn primary_failure_rows(
        &self,
        path: &[&str],
    ) -> Result<Vec<BasicSourceCandidate<'ctx>>, RuntimeError> {
        let [collection] = path else {
            return Err(unsupported());
        };
        let [frame] = self.legacy.frames.as_slice() else {
            return Err(unsupported());
        };
        let [origin] = self.origins.as_slice() else {
            return Err(unsupported());
        };
        let primary = self.owned.registry.primary;
        if collection.is_empty()
            || !matches!(primary, Instance::Group(_))
            || !std::ptr::eq(frame.instance, primary)
            || !std::ptr::eq(self.legacy.primary_source, primary)
            || frame.collection.is_some()
            || frame.document_path.is_some()
            || frame.join.is_some()
            || frame.join_position.is_some()
            || self.legacy.dynamic_source_loader.is_some()
            || self.legacy.filter_map.is_some()
            || !self.owned.frames.is_empty()
            || origin.owner != OwnedOwnerKey::Primary
            || !origin.path.is_empty()
            || origin.owned_frame.is_some()
            || origin.raw_index.is_some()
        {
            return Err(unsupported());
        }
        // For this authenticated one-frame root, legacy selection can only be
        // the actual Primary frame or an actually supplied named binding. Keep
        // native identity refusal before work admission; the observed selected
        // callback below independently verifies the frame that was really used.
        if primary.field(collection).is_none()
            && self
                .legacy
                .named_inputs
                .iter()
                .any(|input| input.name == *collection)
        {
            return Err(unsupported());
        }
        let (mut budget, bounds) = self.budget(SourceOwnerRef::Primary, path, None, false)?;
        budget.slots(1)?;
        let cohort = Arc::new(WalkCohort {
            parent: Arc::clone(&self.identity),
            registry: Arc::clone(&self.owned.registry),
            original_count: OnceLock::new(),
        });
        let mut observed = PrimaryFailureObserver(BasicLegacyObserver {
            parent: self,
            budget,
            bounds,
            cohort: Arc::clone(&cohort),
            key: OwnedOwnerKey::Primary,
            base: &[],
            prefix_len: 0,
            produced: 0,
            raw_head: None,
            wrapped: true,
        });
        let candidates = walk_source_observed(&self.legacy, path, &mut observed)?;
        cohort
            .original_count
            .set(candidates.len())
            .expect("one completed failure walk");
        Ok(candidates)
    }
}

struct PrimaryFailureObserver<'p, 'ctx>(BasicLegacyObserver<'p, 'ctx>);

impl<'ctx> LegacyWalkObserver<'ctx> for PrimaryFailureObserver<'_, 'ctx> {
    type Error = RuntimeError;
    type Output = BasicSourceCandidate<'ctx>;

    fn selected(
        &mut self,
        selected: LegacySelected,
        prefix_len: usize,
    ) -> Result<(), RuntimeError> {
        let LegacySelected::Frame(index) = selected else {
            return Err(unsupported());
        };
        let actual = self
            .0
            .parent
            .legacy
            .frames
            .get(index)
            .ok_or_else(unsupported)?;
        // Reference identity and the actual selected frame establish provenance.
        // A same-name or same-payload named binding is never a Primary witness.
        if !std::ptr::eq(actual.instance, self.0.parent.owned.registry.primary)
            || !matches!(actual.instance, Instance::Group(_))
            || index != 0
            || prefix_len != 0
        {
            return Err(unsupported());
        }
        self.0.selected(LegacySelected::Frame(index), prefix_len)
    }
    fn enter(&mut self, instance: &'ctx Instance, depth: usize) -> Result<(), RuntimeError> {
        self.0.enter(instance, depth)
    }
    fn field_consumed(
        &mut self,
        parent: &'ctx Instance,
        child: &'ctx Instance,
        segment: &str,
    ) -> Result<(), RuntimeError> {
        self.0.field_consumed(parent, child, segment)
    }
    fn reserve(&mut self) -> Result<(), RuntimeError> {
        self.0.reserve()
    }
    fn stack_capacity(&self) -> usize {
        self.0.stack_capacity()
    }
    fn output_capacity(&self) -> usize {
        self.0.output_capacity()
    }
    fn path_copy(&mut self, path: &[String], segment: Option<&str>) -> Result<(), RuntimeError> {
        self.0.path_copy(path, segment)
    }
    fn path_drop(&mut self, segments: usize) {
        self.0.path_drop(segments);
    }
    fn frame_push(
        &mut self,
        frames: &[ScopeFrame<'ctx>],
        path: Option<&[String]>,
    ) -> Result<(), RuntimeError> {
        self.0.frame_push(frames, path)
    }
    fn frame_drop(&mut self, path: Option<&[String]>) {
        self.0.frame_drop(path);
    }
    fn publish(
        &mut self,
        frames: &[ScopeFrame<'ctx>],
        _terminal: &'ctx Instance,
        path: &[String],
    ) -> Result<Self::Output, RuntimeError> {
        #[cfg(test)]
        TERMINALS.with(|n| n.set(n.get() + 1));
        self.0.publish_terminal(frames, path)
    }
}
