use mapping::{FormatOptions, TabularBoundaryKind};

use super::CsvQuoteMode;

/// A pending CSV edit. Saved options are kept separately so an untouched
/// default remains `None` instead of being written as an explicit value.
#[derive(Debug, Clone)]
pub(crate) struct CsvDialectDraft {
    pub(crate) delimiter: String,
    pub(crate) quote_mode: CsvQuoteMode,
    pub(crate) custom_quote: String,
    pub(crate) headers: bool,
    pub(crate) preserve_empty_strings: bool,
    pub(crate) utf8_bom: bool,
    original_options: FormatOptions,
    initial: InitialValues,
}

#[derive(Debug, Clone)]
struct InitialValues {
    delimiter: String,
    quote_mode: CsvQuoteMode,
    custom_quote: String,
    headers: bool,
    preserve_empty_strings: bool,
    utf8_bom: bool,
}

impl CsvDialectDraft {
    pub(crate) fn from_options(options: &FormatOptions, path: &str) -> Self {
        let initial = InitialValues {
            delimiter: options.delimiter.unwrap_or(',').to_string(),
            quote_mode: if options.csv_quote_disabled {
                CsvQuoteMode::Disabled
            } else if options.csv_quote.is_some() {
                CsvQuoteMode::Custom
            } else {
                CsvQuoteMode::Standard
            },
            custom_quote: options.csv_quote.unwrap_or('\'').to_string(),
            headers: options.has_header_row.unwrap_or(true),
            preserve_empty_strings: options.csv_preserve_empty_strings,
            utf8_bom: options.csv_utf8_bom,
        };
        // Like the primary CSV wizard, an explicit TSV selection starts with
        // a tab. The stored default remains comma until this editor is saved.
        let tsv = std::path::Path::new(path.trim())
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tsv"));
        Self {
            delimiter: if tsv && options.delimiter.is_none() && options.tabular_kind.is_none() {
                "\t".to_owned()
            } else {
                initial.delimiter.clone()
            },
            quote_mode: initial.quote_mode,
            custom_quote: initial.custom_quote.clone(),
            headers: initial.headers,
            preserve_empty_strings: initial.preserve_empty_strings,
            utf8_bom: initial.utf8_bom,
            original_options: options.clone(),
            initial,
        }
    }

    pub(crate) fn validated_options(
        &self,
        path: &str,
        source: bool,
    ) -> Result<FormatOptions, String> {
        if !super::uses_path_format(&self.original_options, path)
            && self.original_options.csv_text_repair_dependency.is_none()
        {
            return Err("choose a path-based format before configuring CSV".into());
        }
        if !super::csv_path_compatible(path) {
            return Err(format!(
                "CSV format cannot be used with the recognized non-CSV path `{path}`"
            ));
        }
        let (delimiter, quote, quote_disabled) =
            validate_fields(&self.delimiter, self.quote_mode, &self.custom_quote)?;
        if source && self.utf8_bom != self.initial.utf8_bom {
            return Err("CSV UTF-8 BOM is not editable for an input source".into());
        }
        if !source && self.preserve_empty_strings != self.initial.preserve_empty_strings {
            return Err("CSV empty-text preservation is not editable for an output target".into());
        }

        let mut options = self.original_options.clone();
        if !recognized_csv_path(path) {
            options.tabular_kind = Some(TabularBoundaryKind::Csv);
        }
        if self.delimiter != self.initial.delimiter {
            options.delimiter = Some(delimiter);
        }
        if self.quote_mode != self.initial.quote_mode
            || (self.quote_mode == CsvQuoteMode::Custom
                && self.custom_quote != self.initial.custom_quote)
        {
            options.csv_quote = quote;
            options.csv_quote_disabled = quote_disabled;
        }
        if self.headers != self.initial.headers {
            options.has_header_row = Some(self.headers);
        }
        if source && self.preserve_empty_strings != self.initial.preserve_empty_strings {
            options.csv_preserve_empty_strings = self.preserve_empty_strings;
        }
        if !source && self.utf8_bom != self.initial.utf8_bom {
            options.csv_utf8_bom = self.utf8_bom;
        }
        validate_existing_csv_options(&options, path)?;
        Ok(options)
    }
}

/// Validate a saved dialect without normalizing it. In particular, a repair
/// dependency remains available to the ordinary import and validation paths.
pub(crate) fn validate_existing_csv_options(
    options: &FormatOptions,
    path: &str,
) -> Result<(), String> {
    if !super::csv_path_compatible(path) {
        return Err(format!(
            "CSV format cannot be used with the recognized non-CSV path `{path}`"
        ));
    }
    if options.csv_quote_disabled && options.csv_quote.is_some() {
        return Err("CSV quote cannot be set while quoting is disabled".into());
    }
    let mode = if options.csv_quote_disabled {
        CsvQuoteMode::Disabled
    } else if options.csv_quote.is_some() {
        CsvQuoteMode::Custom
    } else {
        CsvQuoteMode::Standard
    };
    validate_fields(
        &options.delimiter.unwrap_or(',').to_string(),
        mode,
        &options.csv_quote.unwrap_or('\'').to_string(),
    )?;
    Ok(())
}

/// The same lexical check is used by the primary CSV wizard and named drafts.
pub(super) fn validate_fields(
    delimiter: &str,
    quote_mode: CsvQuoteMode,
    custom_quote: &str,
) -> Result<(char, Option<char>, bool), String> {
    let mut delimiter_chars = delimiter.chars();
    let Some(delimiter) = delimiter_chars.next() else {
        return Err(
            "CSV delimiter must be a single ASCII character other than NUL or a line break".into(),
        );
    };
    if delimiter_chars.next().is_some()
        || !delimiter.is_ascii()
        || matches!(delimiter, '\0' | '\r' | '\n')
    {
        return Err(
            "CSV delimiter must be a single ASCII character other than NUL or a line break".into(),
        );
    }
    let quote = match quote_mode {
        CsvQuoteMode::Standard => '"',
        CsvQuoteMode::Custom => {
            let mut quote_chars = custom_quote.chars();
            let Some(quote) = quote_chars.next() else {
                return Err("CSV quote must be exactly one printable ASCII character".into());
            };
            if quote_chars.next().is_some() || !quote.is_ascii_graphic() {
                return Err("CSV quote must be exactly one printable ASCII character".into());
            }
            quote
        }
        CsvQuoteMode::Disabled => return Ok((delimiter, None, true)),
    };
    if delimiter == quote {
        return Err("CSV quote and delimiter must be different characters".into());
    }
    Ok((
        delimiter,
        (quote_mode == CsvQuoteMode::Custom).then_some(quote),
        false,
    ))
}

fn recognized_csv_path(path: &str) -> bool {
    std::path::Path::new(path.trim())
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "csv" | "txt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapping::{CsvTextRepairCause, CsvTextRepairDependency};

    #[test]
    fn named_edit_keeps_repair_and_untouched_optional_csv_flags() {
        let original = FormatOptions {
            csv_text_repair_dependency: Some(CsvTextRepairDependency::new(
                CsvTextRepairCause::ByteOrderMark,
            )),
            csv_utf8_bom: true,
            csv_preserve_empty_strings: true,
            ..FormatOptions::default()
        };
        let mut source = CsvDialectDraft::from_options(&original, "input.csv");
        source.delimiter = ";".into();
        let edited = source.validated_options("input.csv", true).unwrap();
        assert_eq!(
            edited.csv_text_repair_dependency,
            original.csv_text_repair_dependency
        );
        assert_eq!(edited.has_header_row, None);
        assert_eq!(edited.csv_quote, None);
        assert_eq!(edited.tabular_kind, None);
        assert!(edited.csv_utf8_bom);
        assert!(edited.csv_preserve_empty_strings);
        assert_eq!(edited.delimiter, Some(';'));

        let mut target = CsvDialectDraft::from_options(&original, "output.txt");
        target.quote_mode = CsvQuoteMode::Disabled;
        let edited = target.validated_options("output.txt", false).unwrap();
        assert_eq!(
            edited.csv_text_repair_dependency,
            original.csv_text_repair_dependency
        );
        assert_eq!(edited.has_header_row, None);
        assert_eq!(edited.delimiter, None);
        assert!(edited.csv_quote_disabled);
        assert!(edited.csv_utf8_bom);
        assert!(edited.csv_preserve_empty_strings);
    }

    #[test]
    fn named_dialect_rejects_raw_invalid_and_recognized_non_csv_paths() {
        let mut draft = CsvDialectDraft::from_options(&FormatOptions::default(), "in.csv");
        draft.delimiter = "é".into();
        assert!(draft.validated_options("in.csv", true).is_err());
        draft.delimiter = ";".into();
        draft.quote_mode = CsvQuoteMode::Custom;
        draft.custom_quote = ";".into();
        assert!(draft.validated_options("in.csv", true).is_err());
        draft.custom_quote = "'".into();
        assert!(draft.validated_options("in.db", true).is_err());
        assert_eq!(
            draft
                .validated_options("in.dat", true)
                .unwrap()
                .tabular_kind,
            Some(TabularBoundaryKind::Csv)
        );
    }
}
