//! The scope-root slots each engine analysis reads.
//!
//! Analyses visit a scope's roots through [`Scope::visit_roots`] and admit
//! exactly the slots named here, so a slot added to the model stays outside
//! every analysis until one admits it. Roots keep the visitor's slot order,
//! which fixes the order of join validation issues and of owned admission
//! errors.

use mapping::{NodeId, Scope, ScopeRootSlot as Slot};

/// This scope's admitted roots, in visit order.
pub(crate) fn roots(scope: &Scope, admit: impl Fn(Slot) -> bool) -> Vec<NodeId> {
    let mut roots = Vec::new();
    scope.visit_roots(&mut |root| {
        if admit(root.slot) {
            roots.push(root.node);
        }
    });
    roots
}

/// Required-source discovery reads every root except the reserved item a
/// composed sequence owns besides its own item. Generated items are
/// unframed empty-path fields, so neither names a source.
pub(crate) fn required_source(slot: Slot) -> bool {
    slot != Slot::SequenceReservedItem
}

/// Whether a scope needs owned evaluation is decided by its controls,
/// bindings and scalar value. Output paths, dynamic bindings and keys, tree
/// and recursive-filter roots, and sequence roots are not read: owned
/// admission keeps owned reads out of those contexts, and owned selectors and
/// constructions already route the scope themselves.
pub(crate) fn owned_probe(slot: Slot) -> bool {
    matches!(
        slot,
        Slot::Filter
            | Slot::PostGroupFilter
            | Slot::GroupBy
            | Slot::GroupAdjacentBy
            | Slot::GroupStartingWith
            | Slot::GroupEndingWith
            | Slot::GroupIntoBlocks
            | Slot::SortBy
            | Slot::Presence
            | Slot::Window(_)
            | Slot::SortThenBy(_)
            | Slot::Binding(_)
            | Slot::ScalarValue
    )
}

/// The roots whose join references are validated against a scope's own
/// active joins. Presence, windows and sequence inputs are validated
/// separately in the parent context; construction roots and generated items
/// are not join-validated. A block size is validated here even though it
/// evaluates in the parent context.
pub(crate) fn join_validation(slot: Slot) -> bool {
    matches!(
        slot,
        Slot::Filter
            | Slot::PostGroupFilter
            | Slot::GroupBy
            | Slot::GroupAdjacentBy
            | Slot::GroupStartingWith
            | Slot::GroupEndingWith
            | Slot::GroupIntoBlocks
            | Slot::SortBy
            | Slot::OutputPath
            | Slot::SortThenBy(_)
            | Slot::Binding(_)
            | Slot::DynamicBindingKey(_)
            | Slot::DynamicBindingValue(_)
            | Slot::DynamicChildKey(_)
    )
}

/// The tuple expressions of an owned join scope and its static descendants.
/// Its windows are receiving-context expressions admitted separately.
pub(crate) fn join_tuple(slot: Slot) -> bool {
    matches!(
        slot,
        Slot::Filter
            | Slot::PostGroupFilter
            | Slot::GroupBy
            | Slot::GroupAdjacentBy
            | Slot::GroupStartingWith
            | Slot::GroupEndingWith
            | Slot::GroupIntoBlocks
            | Slot::SortBy
            | Slot::SortThenBy(_)
            | Slot::Binding(_)
            | Slot::ScalarValue
    )
}

/// When and in which context owned capability admission reads a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmittedRoot {
    /// Before the construction, in the scope's own context.
    Scope,
    /// Before the construction, in the dynamic target context.
    DynamicBinding,
    /// After the construction, in the dynamic target context.
    DynamicChildKey,
}

/// Construction and sequence roots are admitted with their construction or
/// sequence, whose contexts depend on more than the slot.
pub(crate) fn admitted(slot: Slot) -> Option<AdmittedRoot> {
    match slot {
        Slot::Filter
        | Slot::PostGroupFilter
        | Slot::GroupBy
        | Slot::GroupAdjacentBy
        | Slot::GroupStartingWith
        | Slot::GroupEndingWith
        | Slot::GroupIntoBlocks
        | Slot::SortBy
        | Slot::OutputPath
        | Slot::Presence
        | Slot::Window(_)
        | Slot::SortThenBy(_)
        | Slot::Binding(_) => Some(AdmittedRoot::Scope),
        Slot::DynamicBindingKey(_) | Slot::DynamicBindingValue(_) => {
            Some(AdmittedRoot::DynamicBinding)
        }
        Slot::DynamicChildKey(_) => Some(AdmittedRoot::DynamicChildKey),
        Slot::ScalarValue
        | Slot::RecursiveFilterPredicate
        | Slot::AdjacencyTreeRoot
        | Slot::SequenceInput
        | Slot::SequenceItem
        | Slot::SequenceReservedItem
        | Slot::FailurePredicate
        | Slot::FailureMessage => None,
    }
}

#[cfg(test)]
#[path = "tests/scope_slots.rs"]
mod tests;
