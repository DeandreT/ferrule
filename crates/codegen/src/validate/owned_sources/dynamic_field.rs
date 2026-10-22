//! Explicitly owned dynamic source-field reads. The open object is an exact
//! owned read in the receiving Basic context and its key is a parent-context
//! scalar. Only the `dynamic_fields` capability admits the read; without it, in
//! a closed context and in the default emitter it is refused.

use super::*;

impl OwnedCapabilities {
    /// Read permission is orthogonal to source controls, consumers and failures.
    pub(super) fn permits_dynamic_field(self, expression: &Expression) -> bool {
        self.dynamic_fields && matches!(expression, Expression::OwnedDynamicSourceField(_))
    }
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
pub(super) fn closed_read(
    expression: &Expression,
    capabilities: OwnedCapabilities,
) -> Option<Request> {
    match expression {
        Expression::OwnedDynamicSourceField(field)
            if !capabilities.permits_dynamic_field(expression) =>
        {
            Some(request(field))
        }
        _ => None,
    }
}
