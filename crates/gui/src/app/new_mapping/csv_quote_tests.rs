use super::*;

struct CsvQuoteFiles(PathBuf);

impl CsvQuoteFiles {
    fn new(tag: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-csv-quote-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for CsvQuoteFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bind_csv_columns(project: &mut mapping::Project) {
    project.root.iteration = mapping::ScopeIteration::Source(vec![]);
    for (index, name) in ["Name", "Age"].iter().enumerate() {
        let id = index as mapping::NodeId + 1;
        project.graph.nodes.insert(
            id,
            mapping::Node::SourceField {
                path: vec![(*name).to_owned()],
                frame: None,
            },
        );
        project.root.bindings.push(mapping::Binding {
            target_field: (*name).to_owned(),
            node: id,
        });
    }
}

fn csv_quote_saved_wizard_run(
    tag: &str,
    mode: CsvQuoteMode,
    text: &str,
    expected_name: &str,
) -> anyhow::Result<()> {
    let files = CsvQuoteFiles::new(tag)?;
    let source_path = files.0.join("source.csv");
    let output_path = files.0.join("output.csv");
    let project_path = files.0.join("mapping.json");
    std::fs::write(&source_path, text)?;

    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_csv_source(source_path.clone());
    let setup = app.new_mapping_setup.as_mut().unwrap();
    let Some(MappingBoundary::Csv(source)) = setup.source.as_mut() else {
        panic!("CSV source was not staged");
    };
    source.quote_mode = mode;
    source.custom_quote = "'".to_owned();
    source.refresh_sample()?;
    assert_eq!(source.preview_rows, vec![vec![expected_name, "42"]]);
    assert_eq!(
        source
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["Name", "Age"]
    );
    source.columns[1].ty = ir::ScalarType::Int;
    let mut target = CsvBoundaryDraft::target();
    target.path = output_path.to_str().unwrap().to_owned();
    target.quote_mode = mode;
    target.custom_quote = "'".to_owned();
    target.columns = vec![
        CsvColumnDraft {
            name: "Name".to_owned(),
            ty: ir::ScalarType::String,
        },
        CsvColumnDraft {
            name: "Age".to_owned(),
            ty: ir::ScalarType::Int,
        },
    ];
    setup.target = Some(MappingBoundary::Csv(target));
    assert!(setup.can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    bind_csv_columns(&mut app.project);
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    // Prove the normal GUI open path restores settings rather than retaining
    // the in-memory draft or the previous project.
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&project_path);
    assert!(cli::validate(&reopened.project).is_empty());
    for options in [
        &reopened.project.source_options,
        &reopened.project.target_options,
    ] {
        assert_eq!(
            options.csv_quote,
            (mode == CsvQuoteMode::Custom).then_some('\'')
        );
        assert_eq!(options.csv_quote_disabled, mode == CsvQuoteMode::Disabled);
        assert_eq!(options.has_header_row, Some(true));
    }
    let outcome = cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(outcome.output_path, output_path);
    assert_eq!(std::fs::read(&output_path)?, text.as_bytes());

    // Payload execution uses the saved settings, cannot reopen the source,
    // and does not publish bytes to the physical output identity.
    std::fs::remove_file(&source_path)?;
    std::fs::write(&output_path, b"sentinel")?;
    let payload = cli::PayloadDocument::new(std::path::Path::new("host.csv"), text.as_bytes())?;
    let options = cli::PayloadRunOptions::new(payload).with_output_path(&output_path);
    let outcome = cli::run_project_payloads(&project_path, &options)?;
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(outcome.artifacts[0].records_written, 1);
    assert_eq!(outcome.artifacts[0].bytes, text.as_bytes());
    assert_eq!(std::fs::read(&output_path)?, b"sentinel");
    Ok(())
}

#[test]
fn csv_custom_quote_wizard_preview_save_reopen_file_and_payload_run() -> anyhow::Result<()> {
    csv_quote_saved_wizard_run(
        "custom",
        CsvQuoteMode::Custom,
        "Name,Age\n'O''Neil, Jr.',42\n",
        "O'Neil, Jr.",
    )
}

#[test]
fn csv_disabled_quote_wizard_preview_save_reopen_file_and_payload_run() -> anyhow::Result<()> {
    csv_quote_saved_wizard_run(
        "disabled",
        CsvQuoteMode::Disabled,
        "Name,Age\n\"literal\",42\n",
        "\"literal\"",
    )
}

#[test]
fn csv_invalid_target_quote_keeps_wizard_open_without_replacing_project() -> anyhow::Result<()> {
    let files = CsvQuoteFiles::new("invalid-target")?;
    let source_path = files.0.join("source.csv");
    let output_path = files.0.join("output.csv");
    std::fs::write(&source_path, "Name,Age\nJane,42\n")?;
    let mut app = FerruleApp::default();
    let before = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_new_mapping();
    app.stage_mapping_csv_source(source_path);
    let setup = app.new_mapping_setup.as_mut().unwrap();
    let mut target = CsvBoundaryDraft::target();
    target.path = output_path.to_str().unwrap().to_owned();
    target.columns[0].name = "Name".to_owned();
    target.quote_mode = CsvQuoteMode::Custom;
    target.custom_quote = ",".to_owned();
    setup.target = Some(MappingBoundary::Csv(target));
    assert!(!setup.can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_some());
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, before);
    assert!(!output_path.exists());

    let setup = app.new_mapping_setup.as_mut().unwrap();
    let Some(MappingBoundary::Csv(target)) = setup.target.as_mut() else {
        panic!("CSV target was not retained");
    };
    target.custom_quote = "'".to_owned();
    assert!(setup.can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(app.project.target_options.csv_quote, Some('\''));
    assert!(!output_path.exists());
    Ok(())
}

fn staged_csv_source(app: &FerruleApp) -> &CsvBoundaryDraft {
    match app.new_mapping_setup.as_ref().unwrap().source.as_ref() {
        Some(MappingBoundary::Csv(source)) => source,
        _ => panic!("CSV source was not retained"),
    }
}

fn csv_target_with_age(path: &std::path::Path) -> CsvBoundaryDraft {
    let mut target = CsvBoundaryDraft::target();
    target.path = path.to_str().unwrap().to_owned();
    target.columns = vec![
        CsvColumnDraft {
            name: "Name".to_owned(),
            ty: ir::ScalarType::String,
        },
        CsvColumnDraft {
            name: "Age".to_owned(),
            ty: ir::ScalarType::Int,
        },
    ];
    target
}

#[test]
fn csv_quote_source_staging_retains_width_limit_draft_until_custom_resample() -> anyhow::Result<()>
{
    let files = CsvQuoteFiles::new("initial-width")?;
    let source_path = files.0.join("wide-under-default.csv");
    let output_path = files.0.join("output.csv");
    let name = format!("O'Neil, Jr.{}", ",".repeat(300));
    let text = format!("Name,Age\n'{}',42\n", name.replace('\'', "''"));
    std::fs::write(&source_path, text)?;
    assert!(matches!(
        cli::sample_csv(&source_path, Some(','), true),
        Err(cli::CsvFormatError::SampleTooWide)
    ));

    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_csv_source(source_path.clone());
    assert_eq!(app.status, "CSV sample needs format settings");
    let source = staged_csv_source(&app);
    assert_eq!(source.path, source_path.to_str().unwrap());
    assert!(source.sample_error.is_some());
    assert!(source.columns.is_empty());
    assert!(source.preview_rows.is_empty());
    assert!(source.validate().is_err());
    app.new_mapping_setup.as_mut().unwrap().target =
        Some(MappingBoundary::Csv(csv_target_with_age(&output_path)));
    assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_some());
    assert!(!output_path.exists());

    let setup = app.new_mapping_setup.as_mut().unwrap();
    let Some(MappingBoundary::Csv(source)) = setup.source.as_mut() else {
        panic!("CSV source was not retained after blocked creation");
    };
    source.quote_mode = CsvQuoteMode::Custom;
    source.custom_quote = "'".to_owned();
    source.refresh_sample()?;
    assert_eq!(source.columns.len(), 2);
    assert_eq!(source.preview_rows, vec![vec![name.as_str(), "42"]]);
    assert!(source.sample_error.is_none());
    assert!(setup.can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(app.project.source_options.csv_quote, Some('\''));
    assert!(!output_path.exists());
    Ok(())
}

#[test]
fn csv_quote_source_staging_retains_byte_limit_draft_until_disabled_resample() -> anyhow::Result<()>
{
    let files = CsvQuoteFiles::new("initial-byte-limit")?;
    let source_path = files.0.join("unmatched-literal-quotes.csv");
    let output_path = files.0.join("output.csv");
    // A literal double quote starts an unclosed quoted field under the default
    // dialect. Disabled quoting instead sees short, ordinary physical rows.
    let text = format!("Name,Age\n\"literal,42\n{}", "Jane,42\n".repeat(180_000));
    std::fs::write(&source_path, text)?;
    assert!(matches!(
        cli::sample_csv(&source_path, Some(','), true),
        Err(cli::CsvFormatError::SampleTooLarge)
    ));
    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_csv_source(source_path);
    assert_eq!(app.status, "CSV sample needs format settings");
    assert!(staged_csv_source(&app).sample_error.is_some());
    let setup = app.new_mapping_setup.as_mut().unwrap();
    setup.target = Some(MappingBoundary::Csv(csv_target_with_age(&output_path)));
    assert!(!setup.can_create());
    let Some(MappingBoundary::Csv(source)) = setup.source.as_mut() else {
        panic!("CSV source was not retained");
    };
    source.quote_mode = CsvQuoteMode::Disabled;
    source.refresh_sample()?;
    assert_eq!(source.columns.len(), 2);
    assert_eq!(source.preview_rows[0], ["\"literal", "42"]);
    assert_eq!(source.preview_rows[1], ["Jane", "42"]);
    assert!(source.sample_error.is_none());
    assert!(setup.can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    assert!(app.project.source_options.csv_quote_disabled);
    assert!(!output_path.exists());
    Ok(())
}

#[test]
fn csv_quote_fatal_source_selection_preserves_previous_staged_draft() -> anyhow::Result<()> {
    let files = CsvQuoteFiles::new("fatal-selection")?;
    let valid_path = files.0.join("source.csv");
    let missing_path = files.0.join("missing.csv");
    let directory_path = files.0.join("directory.csv");
    let invalid_path = files.0.join("invalid-utf8.csv");
    std::fs::write(&valid_path, "Name,Age\n'O''Neil, Jr.',42\n")?;
    std::fs::create_dir(&directory_path)?;
    std::fs::write(&invalid_path, b"Name,Age\n\xff,42\n")?;
    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_csv_source(valid_path.clone());
    let setup = app.new_mapping_setup.as_mut().unwrap();
    let Some(MappingBoundary::Csv(source)) = setup.source.as_mut() else {
        panic!("CSV source was not staged");
    };
    source.quote_mode = CsvQuoteMode::Custom;
    source.custom_quote = "'".to_owned();
    source.refresh_sample()?;
    source.columns[1].ty = ir::ScalarType::Int;

    for path in [missing_path, directory_path, invalid_path] {
        assert!(CsvBoundaryDraft::source(path.clone()).is_err());
        app.stage_mapping_csv_source(path);
        assert_eq!(app.status, "failed to sample CSV source");
        let source = staged_csv_source(&app);
        assert_eq!(source.path, valid_path.to_str().unwrap());
        assert_eq!(source.quote_mode, CsvQuoteMode::Custom);
        assert_eq!(source.custom_quote, "'");
        assert_eq!(source.columns.len(), 2);
        assert_eq!(source.columns[1].ty, ir::ScalarType::Int);
        assert_eq!(source.preview_rows, vec![vec!["O'Neil, Jr.", "42"]]);
        assert!(source.sample_error.is_none());
        source.validate()?;
    }
    Ok(())
}
