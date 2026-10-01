use super::*;
use mapping::{Binding, Node, ScopeIteration};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-primary-fixed-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn schema_files(&self) -> anyhow::Result<(PathBuf, PathBuf)> {
        let source = self.0.join("source.xsd");
        let target = self.0.join("target.json");
        std::fs::write(
            &source,
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Row"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Count" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
        )?;
        std::fs::write(
            &target,
            r#"{"title":"Row","type":"object","properties":{"Name":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false}"#,
        )?;
        Ok((source, target))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn staged(
    app: &mut FerruleApp,
    side: SchemaSide,
) -> &mut crate::new_mapping::FixedWidthBoundaryDraft {
    let setup = app.new_mapping_setup.as_mut().unwrap();
    let boundary = match side {
        SchemaSide::Source => setup.source.as_mut(),
        SchemaSide::Target => setup.target.as_mut(),
    };
    match boundary {
        Some(MappingBoundary::FixedWidth(draft)) => draft,
        _ => panic!("expected a staged fixed-width boundary"),
    }
}

fn bind_rows(project: &mut mapping::Project) {
    project.root.iteration = ScopeIteration::Source(Vec::new());
    for (id, name) in [(1, "Name"), (2, "Count")] {
        project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.to_owned()],
                frame: None,
            },
        );
        project.root.bindings.push(Binding {
            target_field: name.to_owned(),
            node: id,
        });
    }
}

#[test]
fn primary_fixed_width_layouts_save_reopen_run_and_native_reimport() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let (source_schema, target_schema) = directory.schema_files()?;
    let source_path = directory.0.join("input.xlsx");
    let target_path = directory.0.join("output.db");
    let project_path = directory.0.join("mapping.json");
    let source_bytes = "Zoë___7__Mia___9__".as_bytes();
    let expected = "Zoë**7***\nMia**9***\n".as_bytes();
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source_schema.clone());
    app.stage_mapping_schema(SchemaSide::Target, target_schema.clone());
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    app.configure_mapping_fixed_width(SchemaSide::Source);
    app.configure_mapping_fixed_width(SchemaSide::Target);
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    assert!(
        !source_path.exists(),
        "configuring never opens the data file"
    );

    let source = staged(&mut app, SchemaSide::Source);
    source.layout.widths = vec!["6".into(), "3".into()];
    source.layout.fill = "_".into();
    source.layout.record_delimiters = false;
    source.layout.treat_empty_as_absent = true;
    source.data_path = source_path.to_str().unwrap().to_owned();
    let target = staged(&mut app, SchemaSide::Target);
    target.layout.widths = vec!["5".into(), "4".into()];
    target.layout.fill = "*".into();
    target.layout.record_delimiters = true;
    target.layout.treat_empty_as_absent = false;
    target.data_path = target_path.to_str().unwrap().to_owned();
    let source_options = staged(&mut app, SchemaSide::Source).options()?;
    let target_options = staged(&mut app, SchemaSide::Target).options()?;
    assert_ne!(source_options, target_options);
    let context = egui::Context::default();
    let _ = context.run_ui(Default::default(), |ui| {
        app.show_new_mapping_setup(ui.ctx())
    });
    assert_eq!(
        staged(&mut app, SchemaSide::Source).options()?,
        source_options
    );
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options()?,
        target_options
    );
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert_eq!(app.project.source_path.as_deref(), source_path.to_str());
    assert_eq!(app.project.target_path.as_deref(), target_path.to_str());
    bind_rows(&mut app.project);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
    assert_eq!(app.project.source_options, source_options);
    app.redo_project();
    assert_eq!(app.project.graph.nodes.len(), 2);
    assert_eq!(app.project.target_options, target_options);
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    std::fs::remove_file(source_schema)?;
    std::fs::remove_file(target_schema)?;
    app.load_project_from(&project_path);
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert!(!app.is_dirty());

    // The source data path did not exist while the wizard was open.
    std::fs::write(&source_path, source_bytes)?;
    cli::run_project_with_paths(&project_path, None, None)?;
    assert_eq!(std::fs::read(&target_path)?, expected);
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, source_bytes)?),
    )?;
    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].path, target_path);
    assert_eq!(payload.artifacts[0].bytes, expected);

    // Local native-shaped export and strict reimport certify this closed shape.
    let design = directory.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&app.project, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible());
    let restored = mfd::import_with_profile(
        &design,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    assert!(restored.imported.warnings.is_empty());
    assert_eq!(restored.imported.project.source_options, source_options);
    assert_eq!(restored.imported.project.target_options, target_options);
    let rerun = cli::run_project_value_payloads(
        &restored.imported.project,
        &design,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, source_bytes)?),
    )?;
    assert_eq!(rerun.artifacts.len(), 1);
    assert_eq!(rerun.artifacts[0].bytes, expected);
    Ok(())
}

#[test]
fn invalid_fixed_width_setup_preserves_staged_boundaries_and_open_mapping() -> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let (source_schema, target_schema) = directory.schema_files()?;
    let nested_schema = directory.0.join("nested.json");
    let malformed_schema = directory.0.join("malformed.json");
    std::fs::write(
        &nested_schema,
        r#"{"title":"Nested","type":"object","properties":{"Child":{"type":"object","properties":{"Value":{"type":"string"}}}}}"#,
    )?;
    std::fs::write(&malformed_schema, "{")?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, nested_schema);
    app.configure_mapping_fixed_width(SchemaSide::Source);
    assert!(matches!(
        app.new_mapping_setup.as_ref().unwrap().source.as_ref(),
        Some(MappingBoundary::Schema(_))
    ));
    assert!(app.status.contains("cannot configure"));

    app.stage_mapping_schema(SchemaSide::Source, source_schema);
    app.stage_mapping_schema(SchemaSide::Target, target_schema);
    app.configure_mapping_fixed_width(SchemaSide::Source);
    app.configure_mapping_fixed_width(SchemaSide::Target);
    let source_options = staged(&mut app, SchemaSide::Source).options()?;
    let target_options = staged(&mut app, SchemaSide::Target).options()?;
    app.stage_mapping_schema(SchemaSide::Target, malformed_schema);
    assert!(app.status.contains("failed to load"));
    assert_eq!(
        staged(&mut app, SchemaSide::Source).options()?,
        source_options
    );
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options()?,
        target_options
    );

    staged(&mut app, SchemaSide::Source).layout.widths[0] = "0".into();
    staged(&mut app, SchemaSide::Target).layout.fill = "\n".into();
    assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_some());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    staged(&mut app, SchemaSide::Source).layout.widths[0] = "8".into();
    staged(&mut app, SchemaSide::Target).layout.fill = " ".into();
    staged(&mut app, SchemaSide::Source).data_path = "bad\0path".into();
    assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    staged(&mut app, SchemaSide::Source).data_path.clear();
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    app.abandon_mapping_fixed_width(SchemaSide::Source);
    assert!(matches!(
        app.new_mapping_setup.as_ref().unwrap().source.as_ref(),
        Some(MappingBoundary::Schema(_))
    ));
    assert!(matches!(
        app.new_mapping_setup.as_ref().unwrap().target.as_ref(),
        Some(MappingBoundary::FixedWidth(_))
    ));
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    app.finish_new_mapping();
    assert!(app.project.source_options.fixed_width.is_none());
    assert!(app.project.target_options.fixed_width.is_some());
    assert!(app.project.source_path.is_none());
    assert!(app.project.target_path.is_none());
    Ok(())
}
