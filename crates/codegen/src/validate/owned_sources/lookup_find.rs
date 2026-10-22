//! Explicitly owned Lookup and CollectionFind. Each walks exactly one declared
//! static endpoint collection like owned iteration; lookup keys and values are
//! nonrepeating scalars inside the selected item. Only the `search.lookup_find`
//! capability admits them; without it they keep their AggregateLookup refusal.

use super::*;

/// Key and value read the selected item through its own frame, so each must
/// name a scalar reached without crossing another repetition.
pub(super) fn lookup_structure(
    program: &Program,
    location: Location,
    lookup: &mapping::OwnedLookup,
) -> Result<(), ProgramValidationError> {
    for relative in [&lookup.key, &lookup.value] {
        item_scalar_structure(program, location.clone(), &lookup.collection, relative)?;
    }
    Ok(())
}

/// An item-relative read names a scalar inside each selected item of
/// `collection` without crossing another repetition below that item.
pub(super) fn item_scalar_structure(
    program: &Program,
    location: Location,
    collection: &mapping::OwnedSourceRef,
    relative: &[String],
) -> Result<(), ProgramValidationError> {
    let mut path = collection.path.clone();
    path.extend(relative.iter().cloned());
    let query = Request::Field {
        owner: (&collection.owner).into(),
        path,
        frame: Some(collection.path.clone()),
    };
    structure(program, location.clone(), query.clone())?;
    if let Some(found) = repeated_below(program, collection, relative) {
        return Err(ProgramValidationError::OwnedSource {
            diagnostic: Diagnostic::Structure {
                location,
                request: query,
                reason: Structure::InvalidOwnedField { found },
            }
            .into(),
        });
    }
    Ok(())
}

/// The first repeating occurrence strictly below the selected item, if any.
fn repeated_below(
    program: &Program,
    collection: &mapping::OwnedSourceRef,
    relative: &[String],
) -> Option<Shape> {
    let root = endpoint(program, &(&collection.owner).into()).ok()?;
    let mut occurrence = follow(root, &collection.path).ok()?;
    for name in relative {
        occurrence = resolved(root, occurrence).ok()?.child(name)?;
        if occurrence.repeating {
            return Some(shape(occurrence));
        }
    }
    None
}
