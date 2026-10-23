//! Explicit source acquisition view, distinct from strict X12 validation.
use super::*;

/// Retain source lexicals so unused invalid scalars are not eagerly converted.
/// The descriptor still binds the physical schema. This does not establish the
/// reference parser's complete validation/reporting policy or publish outputs.
pub fn parse_lexical_x12_source(
    descriptor_text: &str,
    bytes: &[u8],
) -> Result<Instance, X12BoundaryError> {
    input_size(bytes.len())?;
    let profile = Profile::parse(descriptor_text)?;
    let text = format_csv::decode_bom_aware_non_flushing(bytes);
    input_size(text.len())?;
    format_edi::x12::from_str_lexical(&text, &profile.schema).map_err(X12BoundaryError::Native)
}
