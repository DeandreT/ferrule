/// Internal scalar functions that have no lossless native MapForce component.
///
/// Exported designs put these calls in the explicit `ferrule` library. Keeping
/// this list narrow prevents an unrelated vendor component with the same name
/// from being accepted as though it had ferrule's semantics.
pub(crate) fn is_internal(name: &str) -> bool {
    matches!(
        name,
        "isbn10_to_isbn13"
            | "sql_like"
            | "to_number"
            | "json_serialize_object"
            | "json_parse_field"
            | "flextext_parse_field"
    )
}

#[cfg(test)]
mod tests {
    use super::is_internal;

    #[test]
    fn list_excludes_similarly_named_vendor_functions() {
        assert!(is_internal("to_number"));
        assert!(!is_internal("to-number"));
        assert!(!is_internal("convertToISBN13"));
    }
}
