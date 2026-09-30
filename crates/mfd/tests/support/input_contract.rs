//! Unknown imported host-input contracts cannot establish behavioral parity.

pub fn unsupported_input_contract_warning(warnings: &[String]) -> Option<&str> {
    warnings
        .iter()
        .find(|warning| {
            warning.contains("omitted-input semantics are unsupported")
                || warning.contains("with design-time preview")
                || warning.contains("has only a design-time preview value")
                || (warning.contains("design-time preview value")
                    && warning.contains("dependent value skipped"))
        })
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_omission_is_unknown_with_or_without_preview_metadata() {
        for note in ["", "; design-time preview is not a runtime default"] {
            let warning = format!(
                "optional input parameter `choice` has no connected runtime default{note}; omitted-input semantics are unsupported; dependent value skipped"
            );
            assert_eq!(
                unsupported_input_contract_warning(std::slice::from_ref(&warning)),
                Some(warning.as_str())
            );
        }
    }

    #[test]
    fn preview_contract_warnings_remain_distinct_from_unrelated_import_warnings() {
        let unrelated = vec!["unresolved optional annotation omitted".to_owned()];
        assert_eq!(unsupported_input_contract_warning(&unrelated), None);
        for warning in [
            "input component `input` has a design-time preview value but no named runtime parameter or connected default; dependent value skipped",
            "query parameter `choice` with design-time preview uses an unsupported scalar type",
            "optional input parameter `choice` has only a design-time preview value and no connected runtime default; dependent value skipped",
        ] {
            let mut warnings = unrelated.clone();
            warnings.push(warning.to_owned());
            assert_eq!(unsupported_input_contract_warning(&warnings), Some(warning));
        }
    }
}
