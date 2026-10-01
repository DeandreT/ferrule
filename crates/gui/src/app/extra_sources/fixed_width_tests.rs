use super::*;
use crate::extra_sources::ExtraSourceDraftError;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FixedFieldWidth, FixedWidthLayout, Node, Scope, ScopeIteration};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-named-fixed-source-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn row_schema_file(&self, filename: &str, reversed: bool) -> anyhow::Result<PathBuf> {
        let path = self.0.join(filename);
        let properties = if reversed {
            r#""Count":{"type":"integer"},"Name":{"type":"string"}"#
        } else {
            r#""Name":{"type":"string"},"Count":{"type":"integer"}"#
        };
        std::fs::write(
            &path,
            format!(
                r#"{{"title":"Row","type":"object","properties":{{{properties}}},"additionalProperties":false}}"#
            ),
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

fn width(value: u32) -> FixedFieldWidth {
    FixedFieldWidth::new(value).unwrap()
}

fn fixed_options(widths: &[u32], fill: char, delimited: bool) -> mapping::FormatOptions {
    mapping::FormatOptions {
        fixed_width: Some(
            FixedWidthLayout::new(
                widths.iter().copied().map(width).collect(),
                fill,
                delimited,
                false,
            )
            .unwrap(),
        ),
        ..mapping::FormatOptions::default()
    }
}

#[test]
fn named_fixed_width_input_survives_history_save_reopen_and_both_hosts() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let schema_path = directory.row_schema_file("source.json", false)?;
    let source_path = directory.0.join("records.db");
    let target_path = directory.0.join("output.xlsx");
    let driver_path = directory.0.join("driver.json");
    let project_path = directory.0.join("mapping.json");
    let driver = br#"{"driver":"go"}"#;
    let input = "Zoë___7__\nMia___9__\n".as_bytes();
    let expected = "Zoë**7***Mia**9***".as_bytes();
    std::fs::write(&driver_path, driver)?;
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Driver",
        vec![SchemaNode::scalar("driver", ScalarType::String)],
    );
    app.project.target = row_schema();
    app.project.source_path = Some(driver_path.to_str().unwrap().to_owned());
    app.project.target_path = Some(target_path.to_str().unwrap().to_owned());
    app.project.target_options = fixed_options(&[5, 4], '*', false);
    app.mark_clean();
    app.rebase_history();
    let original = mapping::project_file::encode_pretty(&app.project)?;

    app.begin_extra_source();
    app.stage_extra_source_schema(schema_path.clone());
    let draft = app.extra_source_draft.as_mut().unwrap();
    assert_eq!(draft.schema.as_ref(), Some(&row_schema()));
    draft.name = "fixed".into();
    draft.set_instance_path(source_path.to_str().unwrap().to_owned());
    assert!(
        !source_path.exists(),
        "setup must not open the named data file"
    );
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    let pending = draft.fixed_width_draft.as_mut().unwrap();
    pending.widths = vec!["6".into(), "3".into()];
    pending.fill = "_".into();
    pending.record_delimiters = true;
    pending.treat_empty_as_absent = true;
    let saved_options = draft.options.clone();
    assert!(draft.schema_is_ready());
    let staged = format!("{:?}", app.extra_source_draft);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_extra_source_setup(ui.ctx())
    });
    assert_eq!(format!("{:?}", app.extra_source_draft), staged);
    assert_eq!(
        app.extra_source_draft.as_ref().unwrap().options,
        saved_options
    );
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    app.finish_extra_source();
    assert_eq!(app.project.extra_sources.len(), 1);
    let source_options = app.project.extra_sources[0].options.clone();
    assert_ne!(source_options, app.project.target_options);
    assert_eq!(
        source_options.fixed_width.as_ref().unwrap().fill_char(),
        '_'
    );
    assert_eq!(
        app.project
            .target_options
            .fixed_width
            .as_ref()
            .unwrap()
            .fill_char(),
        '*'
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.extra_sources.is_empty());
    app.redo_project();
    assert_eq!(app.project.extra_sources[0].options, source_options);

    app.project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["fixed".into(), "Name".into()],
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["fixed".into(), "Count".into()],
            frame: None,
        },
    );
    app.project.root = Scope {
        iteration: ScopeIteration::Source(vec!["fixed".into()]),
        bindings: vec![
            Binding {
                target_field: "Name".into(),
                node: 0,
            },
            Binding {
                target_field: "Count".into(),
                node: 1,
            },
        ],
        ..Scope::default()
    };
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    std::fs::remove_file(schema_path)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.extra_sources[0].schema, row_schema());
    assert_eq!(app.project.extra_sources[0].options, source_options);
    assert!(!app.is_dirty());

    std::fs::write(&source_path, input)?;
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&target_path)?, expected);
    let extras = [cli::NamedPayloadInput::new(
        "fixed",
        cli::PayloadDocument::new(&source_path, input)?,
    )?];
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&driver_path, driver)?)
            .with_extra_sources(&extras),
    )?;
    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].path, target_path);
    assert_eq!(payload.artifacts[0].bytes, expected);
    Ok(())
}

#[test]
fn named_fixed_width_invalid_schema_settings_and_sqlite_transitions_are_transactional()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let schema_path = directory.row_schema_file("source.json", false)?;
    let reordered = directory.row_schema_file("reordered.json", true)?;
    let malformed = directory.0.join("malformed.json");
    std::fs::write(&malformed, "{")?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_extra_source();
    app.stage_extra_source_schema(schema_path);
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.name = "fixed".into();
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    assert_eq!(
        draft.clone().build(&[]).unwrap_err(),
        ExtraSourceDraftError::EmptyInstancePath
    );
    draft.abandon_fixed_width();
    draft.set_instance_path(directory.0.join("records.db").to_str().unwrap().into());
    draft.options = mapping::FormatOptions {
        json_document: true,
        ..Default::default()
    };
    let old_options = draft.options.clone();
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    assert_eq!(draft.options, old_options);
    draft.fixed_width_draft.as_mut().unwrap().widths[0] = "0".into();
    assert!(!draft.schema_is_ready());
    app.finish_extra_source();
    assert!(app.project.extra_sources.is_empty());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.fixed_width_draft.as_mut().unwrap().widths[0] = "6".into();
    draft.fixed_width_draft.as_mut().unwrap().fill = "\n".into();
    assert!(!draft.schema_is_ready());
    draft.fixed_width_draft.as_mut().unwrap().fill = "_".into();
    let staged = format!("{:?}", app.extra_source_draft);
    app.stage_extra_source_schema(reordered.clone());
    assert!(app.status.contains("failed"));
    assert_eq!(format!("{:?}", app.extra_source_draft), staged);
    app.stage_extra_source_schema(malformed);
    assert_eq!(format!("{:?}", app.extra_source_draft), staged);
    assert!(app.project.extra_sources.is_empty());
    app.stage_extra_source_sqlite_table();
    assert!(app.status.contains("blocked"));
    assert_eq!(format!("{:?}", app.extra_source_draft), staged);
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.abandon_fixed_width();
    assert_eq!(draft.options, old_options);
    draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
    draft.use_path_format();
    assert!(draft.fixed_width_draft.is_none());
    assert_eq!(draft.options, mapping::FormatOptions::default());

    // A previously loaded SQLite table must not revive after a path or table
    // change, even if a pending codec editor is abandoned afterward.
    for change_table in [false, true] {
        let mut draft = ExtraSourceDraft::default();
        draft.name = "sqlite".into();
        draft.set_instance_path("old.db".into());
        draft.sqlite_table = "People".into();
        draft.set_sqlite_schema(row_schema());
        draft.begin_fixed_width().map_err(anyhow::Error::msg)?;
        if change_table {
            draft.set_sqlite_table("Other".into());
        } else {
            draft.set_instance_path("new.db".into());
        }
        draft.abandon_fixed_width();
        assert!(draft.schema.is_none());
        assert!(draft.fixed_width_draft.is_none());
        assert_eq!(
            draft.build(&[]).unwrap_err(),
            ExtraSourceDraftError::MissingSchema
        );
    }

    // Existing saved widths are positional; a same-count reorder cannot be
    // loaded as a replacement schema without an explicit format change.
    let mut saved = ExtraSourceDraft::default();
    saved.name = "fixed".into();
    saved.set_instance_path("records.dat".into());
    saved.set_schema(row_schema());
    saved.options = fixed_options(&[6, 3], '_', true);
    app.extra_source_draft = Some(saved);
    let staged = format!("{:?}", app.extra_source_draft);
    app.stage_extra_source_schema(reordered);
    assert!(app.status.contains("failed"));
    assert_eq!(format!("{:?}", app.extra_source_draft), staged);
    Ok(())
}
