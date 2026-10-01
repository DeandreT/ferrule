use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Node, Scope, ScopeIteration, TabularBoundaryKind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-named-csv-dialect-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn row_schema_file(&self) -> anyhow::Result<PathBuf> {
        let path = self.0.join("row-schema.json");
        std::fs::write(
            &path,
            r#"{"title":"Row","type":"object","properties":{"Name":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false}"#,
        )?;
        Ok(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
}

#[test]
fn named_csv_dialects_survive_history_save_reopen_and_both_execution_hosts() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let schema_path = directory.row_schema_file()?;
    let driver_path = directory.0.join("driver.json");
    let source_path = directory.0.join("catalog.csv");
    let primary_path = directory.0.join("primary.xml");
    let target_path = directory.0.join("report.txt");
    let project_path = directory.0.join("mapping.json");
    let driver = br#"{"driver":"go"}"#;
    let input = b"Name;Count\n'A;B|C';7\n'O''Neil;said \"hi\"|C';8\n";
    let expected = b"\xef\xbb\xbf~A;B|C~|7\n~O'Neil;said \"hi\"|C~|8\n";
    std::fs::write(&driver_path, driver)?;

    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Driver",
        vec![SchemaNode::scalar("driver", ScalarType::String)],
    );
    app.project.target = SchemaNode::group(
        "Main",
        vec![SchemaNode::scalar("driver", ScalarType::String)],
    );
    app.project.source_path = Some(driver_path.to_str().unwrap().to_owned());
    app.project.target_path = Some(primary_path.to_str().unwrap().to_owned());
    app.project.source_options.json_document = true;
    app.project.target_options.xml_document = true;
    app.mark_clean();
    app.rebase_history();
    let original = mapping::project_file::encode_pretty(&app.project)?;

    app.begin_extra_source();
    app.stage_extra_source_schema(schema_path.clone());
    let source = app.extra_source_draft.as_mut().unwrap();
    assert_eq!(source.schema.as_ref(), Some(&row_schema()));
    source.name = "catalog".into();
    source.set_instance_path(source_path.to_str().unwrap().to_owned());
    assert!(
        !source_path.exists(),
        "configuration must not open the data file"
    );
    source.begin_csv_dialect().map_err(anyhow::Error::msg)?;
    let source_dialect = source.csv_dialect_draft.as_mut().unwrap();
    source_dialect.delimiter = ";".into();
    source_dialect.quote_mode = crate::new_mapping::CsvQuoteMode::Custom;
    source_dialect.custom_quote = "'".into();
    source_dialect.headers = true;
    source_dialect.preserve_empty_strings = true;
    source_dialect.utf8_bom = false;
    assert!(source.schema_is_ready());
    let source_staged = format!("{:?}", app.extra_source_draft);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_source_setup(ui.ctx());
    });
    assert_eq!(format!("{:?}", app.extra_source_draft), source_staged);
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    app.finish_extra_source();
    assert_eq!(app.project.extra_sources.len(), 1);
    let source_options = app.project.extra_sources[0].options.clone();
    assert_eq!(source_options.tabular_kind, None);
    assert_eq!(source_options.delimiter, Some(';'));
    assert_eq!(source_options.csv_quote, Some('\''));
    assert!(!source_options.csv_quote_disabled);
    assert_eq!(source_options.has_header_row, None);
    assert!(source_options.csv_preserve_empty_strings);
    assert!(!source_options.csv_utf8_bom);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.extra_sources.is_empty());
    app.redo_project();
    assert_eq!(app.project.extra_sources[0].options, source_options);

    app.begin_extra_target();
    app.stage_extra_target_schema(schema_path.clone());
    let target = app.extra_target_draft.as_mut().unwrap();
    assert_eq!(target.schema.as_ref(), Some(&row_schema()));
    target.name = "report".into();
    target.output_path = target_path.to_str().unwrap().to_owned();
    target.root = Some(Scope {
        iteration: ScopeIteration::Source(vec!["catalog".into()]),
        bindings: vec![
            Binding {
                target_field: "Name".into(),
                node: 1,
            },
            Binding {
                target_field: "Count".into(),
                node: 2,
            },
        ],
        ..Scope::default()
    });
    target.begin_csv_dialect().map_err(anyhow::Error::msg)?;
    let target_dialect = target.csv_dialect_draft.as_mut().unwrap();
    target_dialect.delimiter = "|".into();
    target_dialect.quote_mode = crate::new_mapping::CsvQuoteMode::Custom;
    target_dialect.custom_quote = "~".into();
    target_dialect.headers = false;
    target_dialect.preserve_empty_strings = false;
    target_dialect.utf8_bom = true;
    assert!(target.schema_is_ready());
    let target_staged = format!("{:?}", app.extra_target_draft);
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_target_setup(ui.ctx());
    });
    assert_eq!(format!("{:?}", app.extra_target_draft), target_staged);
    app.finish_extra_target();
    assert_eq!(app.project.extra_targets.len(), 1);
    let target_options = app.project.extra_targets[0].options.clone();
    assert_eq!(target_options.tabular_kind, None);
    assert_eq!(target_options.delimiter, Some('|'));
    assert_eq!(target_options.csv_quote, Some('~'));
    assert!(!target_options.csv_quote_disabled);
    assert_eq!(target_options.has_header_row, Some(false));
    assert!(target_options.csv_utf8_bom);
    assert!(!target_options.csv_preserve_empty_strings);
    assert_ne!(source_options, target_options);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.extra_targets.is_empty());
    assert_eq!(app.project.extra_sources[0].options, source_options);
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].options, target_options);

    app.project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["driver".into()],
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["catalog".into(), "Name".into()],
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["catalog".into(), "Count".into()],
            frame: None,
        },
    );
    app.project.root.bindings = vec![Binding {
        target_field: "driver".into(),
        node: 0,
    }];
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    std::fs::remove_file(schema_path)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.extra_sources[0].schema, row_schema());
    assert_eq!(app.project.extra_targets[0].schema, row_schema());
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert_eq!(app.project.extra_targets[0].options, target_options);
    assert!(!app.is_dirty());

    std::fs::write(&source_path, input)?;
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&target_path)?, expected);
    std::fs::write(&target_path, b"sentinel")?;
    let extras = [cli::NamedPayloadInput::new(
        "catalog",
        cli::PayloadDocument::new(&source_path, input)?,
    )?];
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&driver_path, driver)?)
            .with_extra_sources(&extras),
    )?;
    let named = payload
        .artifacts
        .iter()
        .find(|artifact| artifact.target == "report")
        .ok_or_else(|| anyhow::anyhow!("missing named CSV output"))?;
    assert_eq!(named.path, target_path);
    assert_eq!(named.bytes, expected);
    assert_eq!(std::fs::read(&target_path)?, b"sentinel");
    Ok(())
}

#[test]
fn named_csv_invalid_dialects_abandon_and_path_precedence_are_transactional() -> anyhow::Result<()>
{
    let directory = TestDirectory::new()?;
    let schema_path = directory.row_schema_file()?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;

    app.begin_extra_source();
    app.stage_extra_source_schema(schema_path.clone());
    let source = app.extra_source_draft.as_mut().unwrap();
    source.name = "catalog".into();
    source.set_instance_path("catalog.csv".into());
    source.options.json_document = true;
    assert!(source.begin_csv_dialect().is_err());
    source.options = FormatOptions {
        has_header_row: Some(false),
        ..FormatOptions::default()
    };
    let saved_source_options = source.options.clone();
    source.begin_csv_dialect().map_err(anyhow::Error::msg)?;
    assert_eq!(source.options, saved_source_options);
    let pending = source.csv_dialect_draft.as_mut().unwrap();
    pending.delimiter = "é".into();
    assert!(!source.schema_is_ready());
    app.finish_extra_source();
    assert!(app.project.extra_sources.is_empty());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    let source = app.extra_source_draft.as_mut().unwrap();
    source.csv_dialect_draft.as_mut().unwrap().delimiter = ";".into();
    source.csv_dialect_draft.as_mut().unwrap().quote_mode =
        crate::new_mapping::CsvQuoteMode::Custom;
    source.csv_dialect_draft.as_mut().unwrap().custom_quote = ";".into();
    assert!(!source.schema_is_ready());
    source.csv_dialect_draft.as_mut().unwrap().custom_quote = "'".into();
    assert!(source.schema_is_ready());
    source.set_instance_path("catalog.db".into());
    assert!(
        !source.schema_is_ready(),
        "recognized .db path must not dispatch as CSV"
    );
    assert!(source.clone().build(&[]).is_err());
    source.set_instance_path("catalog.dat".into());
    assert!(
        source.schema_is_ready(),
        "unknown .dat path uses the explicit CSV fallback"
    );
    assert_eq!(
        source.clone().build(&[])?.options.tabular_kind,
        Some(TabularBoundaryKind::Csv)
    );
    source.abandon_csv_dialect();
    assert!(source.csv_dialect_draft.is_none());
    assert_eq!(source.options, saved_source_options);
    source.use_path_format();
    assert!(source.csv_dialect_draft.is_none());
    assert_eq!(source.options, FormatOptions::default());

    app.begin_extra_target();
    app.stage_extra_target_schema(schema_path);
    let target = app.extra_target_draft.as_mut().unwrap();
    target.name = "report".into();
    target.output_path = "report.csv".into();
    target.options.xml_document = true;
    assert!(target.begin_csv_dialect().is_err());
    target.options = FormatOptions {
        csv_utf8_bom: true,
        ..FormatOptions::default()
    };
    let saved_target_options = target.options.clone();
    target.begin_csv_dialect().map_err(anyhow::Error::msg)?;
    assert_eq!(target.options, saved_target_options);
    let pending = target.csv_dialect_draft.as_mut().unwrap();
    pending.delimiter = "|".into();
    pending.quote_mode = crate::new_mapping::CsvQuoteMode::Custom;
    pending.custom_quote = "|".into();
    assert!(!target.schema_is_ready());
    app.finish_extra_target();
    assert!(app.project.extra_targets.is_empty());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    let target = app.extra_target_draft.as_mut().unwrap();
    target.csv_dialect_draft.as_mut().unwrap().custom_quote = "~".into();
    assert!(target.schema_is_ready());
    target.output_path = "report.xlsx".into();
    assert!(
        !target.schema_is_ready(),
        "recognized .xlsx path must not dispatch as CSV"
    );
    assert!(target.clone().build(&[]).is_err());
    target.output_path = "report.tsv".into();
    assert!(
        target.schema_is_ready(),
        ".tsv needs the explicit CSV fallback"
    );
    assert_eq!(
        target.clone().build(&[])?.1.options.tabular_kind,
        Some(TabularBoundaryKind::Csv)
    );
    target.abandon_csv_dialect();
    assert!(target.csv_dialect_draft.is_none());
    assert_eq!(target.options, saved_target_options);
    target.use_path_format();
    assert_eq!(target.options, FormatOptions::default());
    assert!(target.csv_dialect_draft.is_none());
    assert!(app.project.extra_targets.is_empty());
    Ok(())
}
