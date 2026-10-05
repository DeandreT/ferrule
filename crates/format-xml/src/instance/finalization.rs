use std::fmt;

use ir::{Instance, SchemaNode};

use super::{XmlFormatError, XmlWriteOptions, render_with_options, validate_output_characters};

/// Separates native rendering failures from a caller's ordered output policy.
/// Adapters can project either original error by move without adding a cause.
#[derive(Debug)]
pub enum XmlWriteFinalizationError<E> {
    Format(XmlFormatError),
    Policy(E),
}

impl<E: fmt::Display> fmt::Display for XmlWriteFinalizationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => fmt::Display::fmt(error, formatter),
            Self::Policy(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for XmlWriteFinalizationError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::Policy(error) => Some(error),
        }
    }
}

/// A scoped, single-use capability for a header-only preview without hints.
/// Its constructor is private and the token is neither clonable nor reusable.
/// Preview bytes are transient rendering and have not passed character checks.
pub struct XmlWritePreflight<'a> {
    schema: &'a SchemaNode,
    instance: &'a Instance,
    options: &'a XmlWriteOptions,
}

impl XmlWritePreflight<'_> {
    /// Runs one header predicate against the same rendering with hints removed.
    /// The predicate returns only a Boolean verdict; its fixed error type cannot
    /// borrow the temporary preview. Body-character validation stays deferred
    /// until the main finalizer and the unconditional final native scan.
    pub fn check_unhinted_root_header<E, P>(
        self,
        predicate: P,
    ) -> Result<bool, XmlWriteFinalizationError<E>>
    where
        P: for<'preview> FnOnce(&'preview str) -> Result<bool, E>,
    {
        let mut options = self.options.clone();
        options.schema_hints = None;
        let preview = render_with_options(self.schema, self.instance, &options)
            .map_err(XmlWriteFinalizationError::Format)?;
        predicate(&preview).map_err(XmlWriteFinalizationError::Policy)
    }
}

/// Renders once, applies a trusted finalizer, and validates the final characters.
/// The finalizer receives unverified transient text and a scoped single-use
/// header preview. It may copy text or perform effects; those effects are the
/// caller's responsibility. Every successful returned String still passes the
/// native XML 1.0 character scan, even if the callback omits its own check.
/// This does not assert complete XML document or QName validity for arbitrary
/// changes made by a callback. Native rendering errors precede the callback.
/// Policy errors retain their original value and may use any error type E.
pub fn to_string_with_options_and_finalizer<E, F>(
    schema: &SchemaNode,
    instance: &Instance,
    options: &XmlWriteOptions,
    finalizer: F,
) -> Result<String, XmlWriteFinalizationError<E>>
where
    F: for<'call> FnOnce(
        String,
        XmlWritePreflight<'call>,
    ) -> Result<String, XmlWriteFinalizationError<E>>,
{
    let rendered = render_with_options(schema, instance, options)
        .map_err(XmlWriteFinalizationError::Format)?;
    let preview = XmlWritePreflight {
        schema,
        instance,
        options,
    };
    let finalized = finalizer(rendered, preview)?;
    validate_output_characters(&finalized).map_err(XmlWriteFinalizationError::Format)?;
    Ok(finalized)
}
