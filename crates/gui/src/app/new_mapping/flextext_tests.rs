use super::*;
use mapping::{FlexLineEnding, ScopeConstruction};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-flextext-workflow-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write_layout(
        &self,
        name: &str,
        delimiter: &str,
        bom: bool,
        line_ending: &str,
    ) -> anyhow::Result<PathBuf> {
        let path = self.0.join(name);
        let record_separator = if line_ending == "CRLF" {
            "%0D%0A"
        } else {
            "%0A"
        };
        std::fs::write(
            &path,
            format!(
                r#"<FlexText><Commands><Project FileName="not-present.txt" LineEnding="{line_ending}" ByteOrderMark="{}">
<RootName Value="document"/><Connections><Connection><CSV>
<RecordName Value="rows"/><FieldSeparator Value="{delimiter}"/><RecordSeparator Value="{record_separator}"/>
<Fields><Field Type="string"><Name Value="text"/></Field><Field Type="integer"><Name Value="count"/></Field></Fields>
</CSV></Connection></Connections></Project></Commands></FlexText>"#,
                u8::from(bom)
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

fn staged_draft(app: &mut FerruleApp, side: SchemaSide) -> &mut FlexTextBoundaryDraft {
    let setup = app.new_mapping_setup.as_mut().unwrap();
    let boundary = match side {
        SchemaSide::Source => setup.source.as_mut(),
        SchemaSide::Target => setup.target.as_mut(),
    };
    match boundary {
        Some(MappingBoundary::FlexText(draft)) => draft,
        _ => panic!("expected a staged FlexText boundary"),
    }
}

#[test]
fn flextext_boundaries_keep_independent_layouts_through_history_save_reopen_and_both_hosts()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_config = directory.write_layout("source.mft", ";", false, "LF")?;
    let target_config = directory.write_layout("target.mft", "|", true, "CRLF")?;
    let source_path = directory.0.join("source.db");
    let target_path = directory.0.join("target.xlsx");
    let project_path = directory.0.join("mapping.json");
    let input = "Café;2\n\"München;West\";7".as_bytes();
    let expected = "\u{feff}Café|2\r\nMünchen;West|7".as_bytes();
    std::fs::write(&source_path, input)?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source_config.clone());
    assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    app.stage_mapping_schema(SchemaSide::Target, target_config.clone());
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    assert!(
        staged_draft(&mut app, SchemaSide::Source)
            .instance_path
            .is_empty()
    );
    assert!(
        staged_draft(&mut app, SchemaSide::Target)
            .instance_path
            .is_empty()
    );
    assert!(!directory.0.join("not-present.txt").exists());
    staged_draft(&mut app, SchemaSide::Source).instance_path =
        source_path.to_str().unwrap().to_owned();
    staged_draft(&mut app, SchemaSide::Target).instance_path =
        target_path.to_str().unwrap().to_owned();
    let source_schema = staged_draft(&mut app, SchemaSide::Source).schema()?;
    let target_schema = staged_draft(&mut app, SchemaSide::Target).schema()?;
    assert_eq!(source_schema, target_schema);
    let source_options = staged_draft(&mut app, SchemaSide::Source).options()?;
    let target_options = staged_draft(&mut app, SchemaSide::Target).options()?;
    assert_ne!(source_options, target_options);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_new_mapping_setup(ui.ctx())
    });
    assert_eq!(
        staged_draft(&mut app, SchemaSide::Source).options()?,
        source_options
    );
    assert_eq!(
        staged_draft(&mut app, SchemaSide::Target).options()?,
        target_options
    );
    assert_eq!(
        staged_draft(&mut app, SchemaSide::Source).instance_path,
        source_path.to_str().unwrap()
    );
    assert_eq!(
        staged_draft(&mut app, SchemaSide::Target).instance_path,
        target_path.to_str().unwrap()
    );
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(app.project.source, source_schema);
    assert_eq!(app.project.target, target_schema);
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert!(app.project.graph.nodes.is_empty());
    app.project.root.construction = ScopeConstruction::CopyCurrentSource;
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert_ne!(
        app.project.root.construction,
        ScopeConstruction::CopyCurrentSource
    );
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    app.redo_project();
    assert_eq!(
        app.project.root.construction,
        ScopeConstruction::CopyCurrentSource
    );
    std::fs::remove_file(&source_config)?;
    std::fs::remove_file(&target_config)?;
    app.undo_project();
    app.redo_project();
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert_eq!(app.project.source_path.as_deref(), source_path.to_str());
    assert_eq!(app.project.target_path.as_deref(), target_path.to_str());
    assert!(!app.is_dirty());
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&target_path)?, expected);
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, input)?),
    )?;
    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].path, target_path);
    assert_eq!(payload.artifacts[0].bytes, expected);
    Ok(())
}

#[test]
fn invalid_oversized_or_unsupported_flextext_imports_preserve_both_staged_layouts()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_config = directory.write_layout("source.mft", ";", false, "LF")?;
    let target_config = directory.write_layout("target.mft", "|", true, "CRLF")?;
    let malformed = directory.0.join("malformed.mft");
    let unsupported = directory.0.join("unsupported.mft");
    let oversized = directory.0.join("oversized.mft");
    std::fs::write(&malformed, "<FlexText>")?;
    std::fs::write(
        &unsupported,
        r#"<FlexText><Commands><Project><RootName Value="document"/><Connections><Connection><LoadFile/></Connection></Connections></Project></Commands></FlexText>"#,
    )?;
    std::fs::File::create(&oversized)?
        .set_len((mfd::MAX_FLEXTEXT_CONFIGURATION_BYTES + 1) as u64)?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source_config.clone());
    app.stage_mapping_schema(SchemaSide::Target, target_config.clone());
    let source_options = staged_draft(&mut app, SchemaSide::Source).options()?;
    let target_options = staged_draft(&mut app, SchemaSide::Target).options()?;
    for side in [SchemaSide::Source, SchemaSide::Target] {
        for failed in [&malformed, &unsupported, &oversized] {
            app.stage_mapping_schema(side, failed.clone());
            assert!(app.status.contains("failed to load"));
            assert_eq!(
                staged_draft(&mut app, SchemaSide::Source).configuration_path,
                source_config
            );
            assert_eq!(
                staged_draft(&mut app, SchemaSide::Target).configuration_path,
                target_config
            );
            assert_eq!(
                staged_draft(&mut app, SchemaSide::Source).options()?,
                source_options
            );
            assert_eq!(
                staged_draft(&mut app, SchemaSide::Target).options()?,
                target_options
            );
            assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
            assert_eq!(
                mapping::project_file::encode_pretty(&app.project)?,
                original
            );
        }
    }
    Ok(())
}

#[test]
fn flextext_optional_data_paths_stay_empty_and_invalid_paths_cannot_finish() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let source_config = directory.write_layout("source.MFT", ";", false, "LF")?;
    let target_config = directory.write_layout("target.mft", "|", true, "CRLF")?;
    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source_config);
    app.stage_mapping_schema(SchemaSide::Target, target_config);
    assert_eq!(
        staged_draft(&mut app, SchemaSide::Source)
            .layout()
            .output_line_ending(),
        FlexLineEnding::Lf
    );
    assert!(
        !staged_draft(&mut app, SchemaSide::Source)
            .layout()
            .write_bom()
    );
    assert_eq!(
        staged_draft(&mut app, SchemaSide::Target)
            .layout()
            .output_line_ending(),
        FlexLineEnding::Crlf
    );
    assert!(
        staged_draft(&mut app, SchemaSide::Target)
            .layout()
            .write_bom()
    );
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_new_mapping_setup(ui.ctx())
    });
    assert!(
        staged_draft(&mut app, SchemaSide::Source)
            .instance_path
            .is_empty()
    );
    assert!(
        staged_draft(&mut app, SchemaSide::Target)
            .instance_path
            .is_empty()
    );
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    let original = mapping::project_file::encode_pretty(&app.project)?;
    staged_draft(&mut app, SchemaSide::Target).instance_path = "bad\0path".into();
    assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_some());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    staged_draft(&mut app, SchemaSide::Target)
        .instance_path
        .clear();
    app.finish_new_mapping();
    assert!(app.project.source_path.is_none());
    assert!(app.project.target_path.is_none());
    assert!(app.project.source_options.flextext.is_some());
    assert!(app.project.target_options.flextext.is_some());
    Ok(())
}
