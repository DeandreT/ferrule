//! Generated scalar items for predicates evaluated in a paired Basic view.
//!
//! An item is a lexical empty-path legacy collection frame. It never becomes
//! an owned occurrence or origin, so exact endpoint reads keep resolving in the
//! receiving context while ordinary empty-path reads see the item first.

use super::super::{CollectionIdentity, GeneratedItems, SourceOwnerRef, collection_frame};
use super::BasicScopeContext;
use crate::{Instance, RuntimeError};

impl<'ctx> BasicScopeContext<'ctx> {
    /// Lazily copies this view once per generated scalar and appends the item
    /// with its raw one-based position. Each copy is admitted against the
    /// caller's policy only when reached, so an existential consumer can stop
    /// before later items allocate any view metadata.
    pub fn generated_item_contexts<'b>(
        &'b self,
        items: &'b GeneratedItems,
    ) -> impl Iterator<Item = Result<BasicScopeContext<'b>, RuntimeError>> + 'b
    where
        'ctx: 'b,
    {
        items
            .items
            .iter()
            .enumerate()
            .map(move |(index, item)| self.generated_item_context(item, index + 1, items.owner))
    }

    fn generated_item_context<'b>(
        &'b self,
        item: &'b Instance,
        position: usize,
        owner: Option<u32>,
    ) -> Result<BasicScopeContext<'b>, RuntimeError>
    where
        'ctx: 'b,
    {
        let (mut budget, _) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
        let mut view: BasicScopeContext<'b> = self.clone_under(&mut budget)?;
        budget.slots(1)?; // the appended generated frame
        view.legacy.frames.push(collection_frame(
            item,
            owner.map_or_else(
                || CollectionIdentity::Repeated {
                    path: Vec::new(),
                    index: position,
                },
                |item| CollectionIdentity::Generated {
                    item,
                    original_index: position,
                    index: position,
                },
            ),
        ));
        Ok(view)
    }
}

#[cfg(test)]
#[path = "basic_generated_item_tests.rs"]
mod tests;
