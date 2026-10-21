//! Explicitly owned dynamic source-field reads. The open object is an exact
//! owned read in the receiving Basic context and its key is a parent-context
//! scalar. Only the explicit dynamic-field companion admits the read; every
//! earlier companion, closed context and default emitter refuses it.

use super::*;

/// Read permission is orthogonal to source controls, consumers and failures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::validate) enum DynamicFieldPolicy {
    Closed,
    Owned,
}

impl DynamicFieldPolicy {
    pub(super) fn permits(self, expression: &Expression) -> bool {
        self == Self::Owned && matches!(expression, Expression::OwnedDynamicSourceField(_))
    }
}

/// Validate the owned lookup/find composition (owned Lookup and CollectionFind,
/// recursive filters, adjacency trees, path hierarchies, failure sources,
/// IntoBlocks controls and ItemAt over scalar and owned recursive producers)
/// plus owned dynamic reads.
pub fn validate_basic_owned_dynamic_field_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_dynamic_fields(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndIntoBlocksAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::OwnedWithRecursiveFilter,
        SearchPolicy::Basic,
        DynamicFieldPolicy::Owned,
    )
}

pub(super) fn request(field: &mapping::OwnedDynamicSourceField) -> Request {
    Request::DynamicField {
        owner: (&field.object.owner).into(),
        object: field.object.path.clone(),
        frame: field.frame.clone(),
    }
}

/// The object must be an open group whose runtime-named properties are
/// non-repeating scalars; an explicit frame prefixes it and is a collection.
pub(super) fn check(
    program: &Program,
    owner: &Owner,
    object: &[String],
    frame: Option<&[String]>,
) -> Result<(), Structure> {
    let root = endpoint(program, owner)?;
    if let Some(frame) = frame {
        if !object.starts_with(frame) {
            return Err(Structure::IncompatibleFramePath);
        }
        collection(root, frame)?;
    }
    let occurrence = follow(root, object)?;
    let open = resolved(root, occurrence)?
        .dynamic_fields()
        .is_some_and(|dynamic| !dynamic.repeating && dynamic.is_scalar());
    if open {
        Ok(())
    } else {
        Err(Structure::InvalidOwnedDynamicObject {
            found: shape(occurrence),
        })
    }
}

/// The request of an owned read this policy does not admit.
pub(super) fn closed_read(expression: &Expression, policy: DynamicFieldPolicy) -> Option<Request> {
    match expression {
        Expression::OwnedDynamicSourceField(field) if !policy.permits(expression) => {
            Some(request(field))
        }
        _ => None,
    }
}
