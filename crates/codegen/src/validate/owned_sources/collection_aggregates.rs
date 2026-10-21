//! Explicitly owned collection aggregates. Each walks exactly one declared
//! static endpoint collection like owned iteration; a value path is a
//! nonrepeating scalar inside the selected item, and a computed value runs in
//! that item's context. Only the `search.collection_aggregates` capability
//! admits them; without it they keep their AggregateLookup refusal.

use super::*;

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
