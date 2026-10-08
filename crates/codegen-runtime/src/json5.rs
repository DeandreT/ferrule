//! Additive singular JSON5 mapping boundaries. Ordinary JSON routes are unchanged.

use ir::Instance;

use crate::RuntimeError;

mod codec;
mod error;
#[cfg(test)]
mod tests;

pub use error::{Json5BoundaryError, Json5BoundaryResource};
pub use format_json::JsonFormatError;
pub use format_json::json5_boundary::{Json5SyntaxError, Json5SyntaxKind, Json5SyntaxResource};

/// Validate both closed-object descriptors, normalize the complete document,
/// project one object, execute once, and return complete strict JSON with LF.
pub fn map_json5_with<F>(
    source_schema: &str,
    target_schema: &str,
    document: &str,
    mapping: F,
) -> Result<String, Json5BoundaryError>
where
    F: FnOnce(&Instance) -> Result<Instance, RuntimeError>,
{
    codec::check_original_size(document.len())?;
    let (source, target) = codec::schemas(source_schema, target_schema)?;
    let input = codec::project(&source, document)?;
    let output = mapping(&input).map_err(Json5BoundaryError::Mapping)?;
    codec::serialize(&target, &output)
}

/// Apply the same boundary to original UTF-8 bytes. Length precedes decoding,
/// and the exact Utf8Error is retained before any descriptor or mapping work.
pub fn map_json5_bytes_with<F>(
    source_schema: &str,
    target_schema: &str,
    document: &[u8],
    mapping: F,
) -> Result<Vec<u8>, Json5BoundaryError>
where
    F: FnOnce(&Instance) -> Result<Instance, RuntimeError>,
{
    codec::check_original_size(document.len())?;
    let document = std::str::from_utf8(document).map_err(Json5BoundaryError::Encoding)?;
    map_json5_with(source_schema, target_schema, document, mapping).map(String::into_bytes)
}
