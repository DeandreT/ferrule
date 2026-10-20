//! Paired fixed-block grouping over the shared prepaid grouping storage.

use super::*;
use std::num::NonZeroUsize;

/// Generated fixed-block controls for paired IntoBlocks. Bounds and the block
/// size receive the parent; all other controls receive structural or sorted
/// candidates. The size is evaluated once, after the window bounds and before
/// any candidate control, even for an empty cohort.
pub trait BasicGroupIntoBlocksControls<E> {
    fn metadata(&self) -> BasicGroupByMetadata<'_>;
    fn filter(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
    fn sort_key(&mut self, key: usize, context: &BasicScopeContext<'_>) -> Result<Value, E>;
    fn window(&mut self, bound: usize, parent: &BasicScopeContext<'_>)
    -> Result<SequenceWindow, E>;
    fn block_size(&mut self, parent: &BasicScopeContext<'_>) -> Result<NonZeroUsize, E>;
    fn retain_member(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
}

fn prepare_blocks<'p, 'src, E, C>(
    parent: &BasicScopeContext<'src>,
    original: &'p RetainedSourceCohort<'src>,
    controls: &C,
) -> Result<Prepared<'p, 'src>, RuntimeError>
where
    C: BasicGroupIntoBlocksControls<E>,
{
    prepare_with_metadata(parent, original, || controls.metadata())
}

impl<'ctx> BasicScopeContext<'ctx> {
    /// Map fixed-size blocks under the shared bounded paired operation.
    /// Selected candidates fill each block in order and the last block may be
    /// partial. Post runs for every selected candidate, before retained-block
    /// windows or targets; dropping a block never refills its neighbors. No
    /// grouping-key text is returned or charged; reserved key storage remains
    /// Null and unused.
    pub fn map_group_into_blocks_source_cohort<E, C, T>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        controls: &mut C,
        target: &mut T,
    ) -> Result<Vec<Instance>, E>
    where
        E: From<RuntimeError>,
        C: BasicGroupIntoBlocksControls<E>,
        T: for<'g> FnMut(&BasicScopeContext<'g>) -> Result<Instance, E>,
    {
        let mut p = prepare_blocks::<E, C>(self, original, controls).map_err(E::from)?;
        self.execute_grouped_source_cohort(
            original,
            &mut GroupControls::Blocks(controls),
            target,
            &mut p,
        )?;
        Ok(p.output)
    }
}

#[cfg(test)]
pub(super) fn blocks_run_snapshot_for_test<'src, E, C, T>(
    parent: &BasicScopeContext<'src>,
    original: &RetainedSourceCohort<'src>,
    controls: &mut C,
    target: &mut T,
) -> Result<(Vec<Instance>, [usize; 12], bool, usize), E>
where
    E: From<RuntimeError>,
    C: BasicGroupIntoBlocksControls<E>,
    T: for<'g> FnMut(&BasicScopeContext<'g>) -> Result<Instance, E>,
{
    let mut p = prepare_blocks::<E, C>(parent, original, controls).map_err(E::from)?;
    let prepaid = p.budget.bytes;
    parent.execute_grouped_source_cohort(
        original,
        &mut GroupControls::Blocks(controls),
        target,
        &mut p,
    )?;
    let (capacities, null_keys) = prepared_snapshot_for_test(&p);
    // Text charged after preparation: sort keys only, never a block size.
    Ok((p.output, capacities, null_keys, p.budget.bytes - prepaid))
}
