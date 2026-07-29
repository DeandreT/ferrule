use super::{files, unsupported_union};
use crate::JsonFormatError;

pub(super) fn reject_active_contains(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    let dialect = files::validation_dialect(schema);
    if dialect.supports_contains() && object.contains_key("contains") {
        let counts = dialect.supports_contains_counts()
            && (object.contains_key("minContains") || object.contains_key("maxContains"));
        let detail = if counts {
            "`contains` with `minContains` or `maxContains`"
        } else {
            "`contains`"
        };
        return Err(unsupported_union(
            name,
            &format!(
                "{detail} array-item validation is not supported and cannot be ignored exactly"
            ),
        ));
    }
    Ok(())
}
