//! Explicitly owned collection aggregates. Each walks exactly one declared
//! static endpoint collection like owned iteration; a value path is a
//! nonrepeating scalar inside the selected item, and a computed value runs in
//! that item's context. Only the explicit aggregate companion admits them;
//! every older profile keeps its AggregateLookup refusal.

use super::*;

/// Validate owned collection aggregates together with the owned search
/// profile: owned Lookup and CollectionFind, the owned recursive-filter chain
/// and owned IntoBlocks grouping. Earlier companions keep refusing them.
pub fn validate_basic_owned_collection_aggregates_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_search(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndIntoBlocksAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::OwnedWithRecursiveFilter,
        SearchPolicy::BasicWithAggregates,
    )
}

/// Count never reads its value path, so its empty path also counts group
/// items; every other path must be an item scalar.
pub(super) fn value_structure(
    program: &Program,
    location: Location,
    aggregate: &mapping::OwnedAggregate,
) -> Result<(), ProgramValidationError> {
    match &aggregate.value {
        mapping::OwnedAggregateValue::Path(value)
            if !(value.is_empty() && aggregate.function == mapping::AggregateOp::Count) =>
        {
            super::lookup_find::item_scalar_structure(
                program,
                location,
                &aggregate.collection,
                value,
            )
        }
        _ => Ok(()),
    }
}
