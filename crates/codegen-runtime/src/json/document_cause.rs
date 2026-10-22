//! Cause-retaining JSON document parsing, additive beside [`super::parse_json`].

use std::fmt;

use ir::Instance;

use super::{
    ConstraintBoundary, JsonBoundaryError, check_input_size, unique_items,
    without_boundary_constraints,
};

/// The original `format_json` failure behind [`JsonBoundaryError::InvalidDocument`].
#[derive(Debug)]
pub struct JsonDocumentError(format_json::JsonFormatError);

impl JsonDocumentError {
    pub fn format_error(&self) -> &format_json::JsonFormatError {
        &self.0
    }
}

impl fmt::Display for JsonDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for JsonDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Format errors are not comparable values; equal text is equal.
impl PartialEq for JsonDocumentError {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_string() == other.0.to_string()
    }
}

fn invalid_document(error: format_json::JsonFormatError) -> JsonBoundaryError {
    JsonBoundaryError::InvalidDocument(JsonDocumentError(error))
}

/// Parses one bounded JSON document exactly as [`super::parse_json`] does, but
/// retains a `format_json` failure as a typed cause instead of its message.
/// Display text is unchanged; other boundary variants are identical.
pub fn parse_json_with_format_cause(
    schema: &str,
    document: &str,
) -> Result<Instance, JsonBoundaryError> {
    check_input_size(document.len())?;
    let boundary = ConstraintBoundary::new();
    let schema = boundary.parse_schema(schema)?;
    if unique_items::schema_contains_assertion(&schema) {
        format_json::json_schema::unique_items::validate_raw_json_unique_items(&schema, document)
            .map_err(invalid_document)?;
    }
    let formatter_schema = without_boundary_constraints(schema.clone());
    let instance = format_json::from_str(document, &formatter_schema).map_err(invalid_document)?;
    boundary.validate_input(&schema, &instance)?;
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;
    use crate::parse_json;

    const SCHEMA: &str = r#"{"name":"Root","kind":{"kind":"group","children":[{"name":"Value","kind":{"kind":"scalar","ty":"int"}}]}}"#;
    const UNIQUE: &str = r#"{"name":"Root","kind":{"kind":"group","children":[{"name":"Items","repeating":true,"json_unique_items":true,"kind":{"kind":"scalar","ty":"int"}}]}}"#;

    #[test]
    fn matches_legacy_parsing_while_retaining_format_causes() {
        for (schema, document) in [
            (SCHEMA, r#"{"Value":7}"#),
            (SCHEMA, r#"{"Value":"#),
            (SCHEMA, r#"{"Value":"text"}"#),
            (SCHEMA, ""),
            (UNIQUE, r#"{"Items":[1,1]}"#),
            (UNIQUE, r#"{"Items":[1,2]}"#),
            ("{", "{}"),
        ] {
            let legacy = parse_json(schema, document);
            let retained = parse_json_with_format_cause(schema, document);
            match (&legacy, &retained) {
                (Ok(legacy), Ok(retained)) => assert_eq!(legacy, retained),
                (Err(JsonBoundaryError::InvalidInput { message }), Err(retained)) => {
                    let JsonBoundaryError::InvalidDocument(cause) = retained else {
                        panic!("expected a retained format cause, got {retained:?}");
                    };
                    assert_eq!(&cause.to_string(), message);
                    assert!(retained.source().unwrap().is::<JsonDocumentError>());
                    assert!(cause.source().unwrap().is::<format_json::JsonFormatError>());
                    assert_eq!(
                        legacy.as_ref().unwrap_err().to_string(),
                        retained.to_string()
                    );
                }
                (Err(legacy), Err(retained)) => assert_eq!(legacy, retained),
                _ => panic!("parse outcomes differ: {legacy:?} / {retained:?}"),
            }
        }
    }
}
