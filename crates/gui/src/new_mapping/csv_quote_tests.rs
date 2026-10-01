use super::*;

fn valid_target() -> CsvBoundaryDraft {
    let mut target = CsvBoundaryDraft::target();
    target.path = "output.csv".to_owned();
    target.columns[0].name = "Name".to_owned();
    target
}

#[test]
fn csv_quote_defaults_and_inactive_custom_text_keep_the_runtime_dialect() {
    let mut target = valid_target();
    assert_eq!(target.quote_mode, CsvQuoteMode::Standard);
    assert_eq!(target.options().unwrap().csv_quote, None);
    assert!(!target.options().unwrap().csv_quote_disabled);

    target.delimiter = '"';
    assert!(
        target
            .validate()
            .unwrap_err()
            .to_string()
            .contains("different")
    );
    target.quote_mode = CsvQuoteMode::Custom;
    target.custom_quote = "'".to_owned();
    target.validate().unwrap();
    assert_eq!(target.options().unwrap().csv_quote, Some('\''));

    target.quote_mode = CsvQuoteMode::Disabled;
    target.custom_quote.clear();
    target.validate().unwrap();
    let options = target.options().unwrap();
    assert_eq!(options.delimiter, Some('"'));
    assert_eq!(options.csv_quote, None);
    assert!(options.csv_quote_disabled);
}

#[test]
fn csv_quote_rejects_invalid_characters_and_active_delimiter_collisions() {
    for quote in ["", "''", " ", "é", "\n", "\0", ","] {
        let mut target = valid_target();
        target.quote_mode = CsvQuoteMode::Custom;
        target.custom_quote = quote.to_owned();
        assert!(
            target
                .validate()
                .unwrap_err()
                .to_string()
                .contains("CSV quote")
        );
        assert!(target.options().is_err());
        let setup = NewMappingSetup {
            source: Some(MappingBoundary::Csv(valid_target())),
            target: Some(MappingBoundary::Csv(target)),
        };
        assert!(!setup.can_create());
        assert!(setup.build_project().is_err());
    }
    for delimiter in ['\0', '\r', '\n', 'é'] {
        let mut target = valid_target();
        target.delimiter = delimiter;
        target.quote_mode = CsvQuoteMode::Disabled;
        assert!(
            target
                .validate()
                .unwrap_err()
                .to_string()
                .contains("CSV delimiter")
        );
    }
}

#[test]
fn csv_quote_sample_failure_retains_cache_and_recovers_selected_types() {
    let path = std::env::temp_dir().join(format!(
        "ferrule-gui-csv-quote-cache-{}.csv",
        std::process::id()
    ));
    std::fs::write(&path, "Name,Age\n'O''Neil, Jr.',42\n").unwrap();
    let mut source = CsvBoundaryDraft::source(path.clone()).unwrap();
    source.quote_mode = CsvQuoteMode::Custom;
    source.custom_quote = "'".to_owned();
    source.refresh_sample().unwrap();
    source.columns[1].ty = ScalarType::Int;
    assert_eq!(source.preview_rows, vec![vec!["O'Neil, Jr.", "42"]]);
    let cached_rows = source.preview_rows.clone();
    let cached_columns = source
        .columns
        .iter()
        .map(|column| (column.name.clone(), column.ty))
        .collect::<Vec<_>>();

    source.custom_quote = ",".to_owned();
    assert!(
        source
            .refresh_sample()
            .unwrap_err()
            .to_string()
            .contains("different")
    );
    assert!(source.sample_error.is_some());
    assert!(source.validate().is_err());
    assert_eq!(source.preview_rows, cached_rows);
    assert_eq!(
        source
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.ty))
            .collect::<Vec<_>>(),
        cached_columns
    );
    source.custom_quote = "'".to_owned();
    source.refresh_sample().unwrap();
    source.validate().unwrap();
    assert!(source.sample_error.is_none());
    assert_eq!(source.columns[1].ty, ScalarType::Int);
    assert_eq!(source.preview_rows, cached_rows);
    std::fs::remove_file(path).unwrap();
}
