use super::*;
use calamine::{Data, Reader, Xlsx};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Node, ScopeIteration, TabularBoundaryKind};
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const SCHEMA: &str = r#"{"title":"Row","type":"object","properties":{"Name":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false}"#;

struct TestDirectory {
    path: PathBuf,
    completed: bool,
}
impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-flat-workbook-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            completed: false,
        })
    }
    fn schemas(&self) -> anyhow::Result<(PathBuf, PathBuf)> {
        let source = self.path.join("source.json");
        let target = self.path.join("target.json");
        std::fs::write(&source, SCHEMA)?;
        std::fs::write(&target, SCHEMA)?;
        Ok((source, target))
    }
    fn record(&self, name: &str, text: impl AsRef<[u8]>) -> anyhow::Result<()> {
        std::fs::write(self.path.join(name), text)?;
        Ok(())
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.completed && std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() != Ok("1") {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn staged(app: &mut FerruleApp, side: SchemaSide) -> &mut XlsxBoundaryDraft {
    let setup = app.new_mapping_setup.as_mut().expect("staged wizard");
    match match side {
        SchemaSide::Source => setup.source.as_mut(),
        SchemaSide::Target => setup.target.as_mut(),
    } {
        Some(MappingBoundary::Xlsx(draft)) => draft,
        _ => panic!("staged workbook"),
    }
}
fn setup(app: &mut FerruleApp, directory: &TestDirectory) -> anyhow::Result<()> {
    let (source, target) = directory.schemas()?;
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source);
    app.stage_mapping_schema(SchemaSide::Target, target);
    Ok(())
}
fn configure(app: &mut FerruleApp) {
    app.configure_mapping_xlsx(SchemaSide::Source);
    app.configure_mapping_xlsx(SchemaSide::Target);
}
fn bind(project: &mut mapping::Project) {
    project.root.iteration = ScopeIteration::Source(Vec::new());
    for (id, name) in [(1, "Name"), (2, "Count")] {
        project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.into()],
                frame: None,
            },
        );
        project.root.bindings.push(Binding {
            target_field: name.into(),
            node: id,
        });
    }
}
fn row(name: &str, count: i64) -> Instance {
    Instance::Group(
        vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ("Count".into(), Instance::Scalar(Value::Int(count))),
        ]
        .into(),
    )
}
fn click(
    app: &mut FerruleApp,
    ctx: &egui::Context,
    directory: &TestDirectory,
    wanted: &str,
    direction: f32,
) -> anyhow::Result<()> {
    // Keep these original controls in their unchanged 1200x1800 viewport.
    // Reuse the full-clip, real outer-wheel path exercised separately at
    // desktop heights; a clipped control is never treated as clickable.
    desktop_wizard_click(app, ctx, directory, 1800.0, wanted, direction)
}

#[test]
fn flat_workbook_wizard_real_controls_keep_drafts_separate_and_capture_headers()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    let original_document = app.document.clone();
    setup(&mut app, &directory)?;
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, &directory, "Configure workbook", -150.0)?;
    assert!(matches!(
        app.new_mapping_setup.as_ref().unwrap().source,
        Some(MappingBoundary::Xlsx(_))
    ));
    click(&mut app, &context, &directory, "Configure workbook", -150.0)?;
    click(&mut app, &context, &directory, "Skip header row", 150.0)?;
    click(&mut app, &context, &directory, "Write header row", -150.0)?;
    let source = staged(&mut app, SchemaSide::Source).options(false)?;
    let target = staged(&mut app, SchemaSide::Target).options(true)?;
    directory.record(
        "before-create-source-options.json",
        serde_json::to_vec_pretty(&source)?,
    )?;
    directory.record(
        "before-create-target-options.json",
        serde_json::to_vec_pretty(&target)?,
    )?;
    assert_eq!(source.tabular_kind, Some(TabularBoundaryKind::Xlsx));
    assert_eq!(source.xlsx_sheet, None);
    assert_eq!(source.xlsx_start_row, Some(1));
    assert_eq!(source.xlsx_columns, [1, 2]);
    assert_eq!(source.has_header_row, Some(false));
    assert_eq!(target.has_header_row, Some(false));
    assert!(source.xlsx_headers.is_empty() && target.xlsx_headers.is_empty());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    assert_eq!(app.document, original_document);
    assert!(
        !app.project_editing_enabled(),
        "the wizard retains the ordinary editing lock"
    );
    click(&mut app, &context, &directory, "Write header row", -150.0)?;
    click(&mut app, &context, &directory, "Create mapping", -150.0)?;
    directory.record(
        "created.json",
        mapping::project_file::encode_pretty(&app.project)?,
    )?;
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(app.project.source_options, source);
    assert_eq!(app.project.target_options.has_header_row, Some(true));
    assert_eq!(app.project.target_options.xlsx_headers, ["Name", "Count"]);
    assert!(!app.project.target_options.xlsx_update_existing);
    assert_eq!(app.project.source_path, None);
    assert_eq!(app.project.target_path, None);
    assert!(app.project_editing_enabled());
    assert!(app.is_dirty());
    directory.completed = true;
    Ok(())
}

#[test]
fn flat_workbook_wizard_save_reopen_history_file_and_payload_use_literal_cells()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup(&mut app, &directory)?;
    configure(&mut app);
    let input = directory.path.join("source.xlsx");
    let output = directory.path.join("output.xlsx");
    let project_path = directory.path.join("mapping.json");
    let source = staged(&mut app, SchemaSide::Source);
    source.sheet = "Incoming".into();
    source.start_row = "3".into();
    source.columns = vec!["4".into(), "1".into()];
    source.data_path = input.to_str().unwrap().into();
    let source_options = source.options(false)?;
    let target = staged(&mut app, SchemaSide::Target);
    target.sheet = "Résultats".into();
    target.start_row = "5".into();
    target.columns = vec!["2".into(), "5".into()];
    target.headers = vec!["Person".into(), "Total".into()];
    target.data_path = output.to_str().unwrap().into();
    let target_options = target.options(true)?;
    assert!(
        !input.exists() && !output.exists(),
        "setup never opens a data workbook"
    );
    app.finish_new_mapping();
    bind(&mut app.project);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
    assert_eq!(app.project.source_options, source_options);
    app.redo_project();
    assert_eq!(app.project.graph.nodes.len(), 2);
    assert_eq!(app.project.target_options, target_options);
    assert!(cli::validate(&app.project).is_empty());
    app.save_document_to(&project_path)?;
    std::fs::remove_file(directory.path.join("source.json"))?;
    std::fs::remove_file(directory.path.join("target.json"))?;
    app.load_project_from(&project_path);
    directory.record(
        "reopened.json",
        mapping::project_file::encode_pretty(&app.project)?,
    )?;
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert_eq!(app.project.source_path.as_deref(), input.to_str());
    assert_eq!(app.project.target_path.as_deref(), output.to_str());
    assert!(!app.is_dirty());
    let expected_rows = vec![row("Zoë", 7), row("Mia", 9)];
    let source_bytes = format_xlsx::to_bytes(
        &app.project.source,
        &expected_rows,
        Some("Incoming"),
        3,
        &[4, 1],
        true,
    )?;
    directory.record("source.xlsx", &source_bytes)?;
    let file_result = cli::run_project_with_paths(&project_path, None, None);
    directory.record("file-result.txt", format!("{file_result:#?}"))?;
    file_result?;
    let file_bytes = std::fs::read(&output)?;
    assert_physical_output(&directory, "file", &file_bytes)?;
    let payload_result = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&input, &source_bytes)?),
    );
    directory.record("payload-result.txt", format!("{payload_result:#?}"))?;
    let payload = payload_result?;
    assert_eq!(payload.artifacts.len(), 1);
    directory.record("payload.xlsx", &payload.artifacts[0].bytes)?;
    assert_physical_output(&directory, "payload", &payload.artifacts[0].bytes)?;
    assert_eq!(payload.artifacts[0].path, output);
    assert_eq!(std::fs::read(&input)?, source_bytes);
    assert_eq!(
        std::fs::read(&output)?,
        file_bytes,
        "payload execution never replaces the output file"
    );
    directory.completed = true;
    Ok(())
}

fn assert_physical_output(
    directory: &TestDirectory,
    tag: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let sheets = workbook.sheet_names().to_vec();
    let range = workbook.worksheet_range("Résultats")?;
    directory.record(
        &format!("{tag}-physical-cells.txt"),
        format!("sheets={sheets:?}\n{range:#?}"),
    )?;
    assert_eq!(sheets, ["Résultats"]);
    assert_eq!(range.start(), Some((4, 1)));
    assert_eq!(range.end(), Some((6, 4)));
    for row in 0..8 {
        for column in 0..6 {
            let expected = match (row, column) {
                (4, 1) => Data::String("Person".into()),
                (4, 4) => Data::String("Total".into()),
                (5, 1) => Data::String("Zoë".into()),
                (5, 4) => Data::Float(7.0),
                (6, 1) => Data::String("Mia".into()),
                (6, 4) => Data::Float(9.0),
                _ => Data::Empty,
            };
            assert_eq!(
                range.get_value((row, column)).unwrap_or(&Data::Empty),
                &expected,
                "{tag} cell {row},{column}"
            );
        }
    }
    Ok(())
}

#[test]
fn flat_workbook_invalid_options_and_abandon_keep_the_open_project_and_other_boundary()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    let original = mapping::project_file::encode_pretty(&app.project)?;
    setup(&mut app, &directory)?;
    configure(&mut app);
    let target_before = staged(&mut app, SchemaSide::Target).options(true)?;
    for invalid in ["0", "1048577", "-1", "1.5", "", "999999999999"] {
        staged(&mut app, SchemaSide::Source).start_row = invalid.into();
        let refusal = app.new_mapping_setup.as_ref().unwrap().build_project();
        directory.record(
            &format!("bad-row-{}.txt", invalid.replace(['.', '-'], "_")),
            format!("{refusal:#?}"),
        )?;
        assert!(refusal.is_err());
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert!(app.new_mapping_setup.is_some());
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
    }
    staged(&mut app, SchemaSide::Source).start_row = "1048576".into();
    staged(&mut app, SchemaSide::Source).columns = vec!["1".into(), "16384".into()];
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    for columns in [
        ["0", "2"],
        ["1", "16385"],
        ["2", "2"],
        ["", "2"],
        ["1.5", "2"],
    ] {
        staged(&mut app, SchemaSide::Source).columns = columns.map(str::to_owned).into();
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
    }
    staged(&mut app, SchemaSide::Source).columns = vec!["1".into(), "2".into()];
    for invalid in [
        "bad/name",
        "'first",
        "last'",
        "bad\0name",
        "12345678901234567890123456789012",
    ] {
        staged(&mut app, SchemaSide::Source).sheet = invalid.into();
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    }
    staged(&mut app, SchemaSide::Source).sheet = "Data β".into();
    for invalid in [
        "input.csv",
        "input.json",
        "input.XML",
        "input.db",
        "bad\0.xlsx",
    ] {
        staged(&mut app, SchemaSide::Source).data_path = invalid.into();
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    }
    for valid in ["", "input.xlsx", "input.XLSX", "input.table", "input"] {
        staged(&mut app, SchemaSide::Source).data_path = valid.into();
        assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    }
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options(true)?,
        target_before
    );
    app.abandon_mapping_xlsx(SchemaSide::Source);
    assert!(matches!(
        app.new_mapping_setup.as_ref().unwrap().source,
        Some(MappingBoundary::Schema(_))
    ));
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options(true)?,
        target_before
    );
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, &directory, "Cancel", -150.0)?;
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    directory.completed = true;
    Ok(())
}

#[test]
fn flat_workbook_requires_supported_schema_and_respects_dialog_lock_and_header_identity()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup(&mut app, &directory)?;
    let (_, receiver) = std::sync::mpsc::channel::<Option<String>>();
    app.pending_dialog = Some((DialogKind::BrowseSourceSchema, receiver));
    app.configure_mapping_xlsx(SchemaSide::Source);
    assert!(matches!(
        app.new_mapping_setup.as_ref().unwrap().source,
        Some(MappingBoundary::Schema(_))
    ));
    app.pending_dialog = None;
    configure(&mut app);
    let original = mapping::project_file::encode_pretty(&app.project)?;
    let source_before = staged(&mut app, SchemaSide::Source).options(false)?;
    let target = staged(&mut app, SchemaSide::Target);
    target.headers = vec!["Same".into(), "Same".into()];
    assert_eq!(target.options(true)?.xlsx_headers, ["Same", "Same"]);
    target.headers = vec!["".into(), "Count β".into()];
    let target_before = target.options(true)?;
    let (_, receiver) = std::sync::mpsc::channel::<Option<String>>();
    app.pending_dialog = Some((DialogKind::BrowseTargetSchema, receiver));
    app.abandon_mapping_xlsx(SchemaSide::Target);
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, &directory, "Write header row", -150.0)?;
    click(&mut app, &context, &directory, "Create mapping", -150.0)?;
    assert!(app.new_mapping_setup.is_some());
    assert_eq!(
        staged(&mut app, SchemaSide::Source).options(false)?,
        source_before
    );
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options(true)?,
        target_before
    );
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    app.pending_dialog = None;
    click(&mut app, &context, &directory, "Write header row", -150.0)?;
    assert!(
        staged(&mut app, SchemaSide::Target)
            .options(true)?
            .xlsx_headers
            .is_empty()
    );
    assert_eq!(
        staged(&mut app, SchemaSide::Target).headers,
        ["", "Count β"]
    );
    click(&mut app, &context, &directory, "Write header row", -150.0)?;
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options(true)?,
        target_before
    );
    let mut malformed = SchemaNode::group("Row", vec![SchemaNode::group("Nested", vec![])]);
    for schema in [
        malformed.clone(),
        SchemaNode::scalar("Scalar", ScalarType::String),
        SchemaNode::group("Empty", vec![]),
        SchemaNode::group("Row", vec![SchemaNode::scalar("Name", ScalarType::String)]).repeating(),
    ] {
        let imported = crate::new_mapping::ImportedSchema {
            path: "invalid.json".into(),
            schema,
        };
        assert!(XlsxBoundaryDraft::from_imported(&imported).is_err());
    }
    malformed = SchemaNode::group(
        "Row",
        vec![SchemaNode::scalar("Name", ScalarType::String).attribute()],
    );
    let imported = crate::new_mapping::ImportedSchema {
        path: "attribute.xsd".into(),
        schema: malformed,
    };
    assert!(XlsxBoundaryDraft::from_imported(&imported).is_err());
    let full = app.new_mapping_setup.as_ref().unwrap().build_project()?;
    directory.record(
        "default-project.json",
        mapping::project_file::encode_pretty(&full)?,
    )?;
    assert_eq!(full.target_options, target_before);
    assert_eq!(full.source_options, source_before);
    assert!(full.target_options.fixed_width.is_none());
    assert!(full.target_options.xlsx_rows.is_empty());
    assert!(full.target_options.xlsx_hierarchical.is_none());
    assert!(full.target_options.xlsx_composite.is_none());
    assert!(full.target_options.xlsx_grid.is_none());
    assert!(full.target_options.xlsx_worksheet_set.is_none());
    assert!(!full.target_options.xlsx_update_existing);
    assert_eq!(full.target_options.delimiter, None);
    assert_eq!(full.target_options.csv_quote, None);
    let reopened =
        mapping::project_file::decode_str(&mapping::project_file::encode_pretty(&full)?)?;
    assert_eq!(
        mapping::project_file::encode_pretty(&reopened)?,
        mapping::project_file::encode_pretty(&full)?
    );
    assert_eq!(
        full.source_options.tabular_kind,
        Some(TabularBoundaryKind::Xlsx)
    );
    assert_eq!(
        full.target_options.tabular_kind,
        Some(TabularBoundaryKind::Xlsx)
    );
    directory.completed = true;
    Ok(())
}

#[test]
fn flat_workbook_first_sheet_without_headers_and_missing_sheet_preserve_output()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup(&mut app, &directory)?;
    configure(&mut app);
    let input = directory.path.join("source.table");
    let output = directory.path.join("result.data");
    let project_path = directory.path.join("mapping.json");
    let source = staged(&mut app, SchemaSide::Source);
    source.data_path = input.to_str().unwrap().into();
    source.start_row = "2".into();
    source.columns = vec!["3".into(), "1".into()];
    source.has_header_row = false;
    let target = staged(&mut app, SchemaSide::Target);
    target.data_path = output.to_str().unwrap().into();
    target.start_row = "2".into();
    target.columns = vec!["1".into(), "3".into()];
    target.has_header_row = false;
    target.headers = vec!["Ignored".into(), "Ignored".into()];
    app.finish_new_mapping();
    bind(&mut app.project);
    assert_eq!(app.project.source_options.xlsx_sheet, None);
    assert_eq!(app.project.target_options.xlsx_sheet, None);
    assert!(app.project.target_options.xlsx_headers.is_empty());
    let rows = vec![row("A", 2), row("B", 3)];
    let bytes = format_xlsx::to_bytes(
        &app.project.source,
        &rows,
        Some("Only sheet"),
        2,
        &[3, 1],
        false,
    )?;
    directory.record("source.table", &bytes)?;
    app.save_document_to(&project_path)?;
    let first = cli::run_project_with_paths(&project_path, None, None);
    directory.record("first-sheet-result.txt", format!("{first:#?}"))?;
    assert_eq!(first?.records_written, 2);
    let result = std::fs::read(&output)?;
    let mut workbook = Xlsx::new(Cursor::new(&result))?;
    let sheets = workbook.sheet_names().to_vec();
    let range = workbook.worksheet_range("Sheet1")?;
    directory.record(
        "first-sheet-no-header-cells.txt",
        format!("{sheets:?}\n{range:#?}"),
    )?;
    assert_eq!(sheets, ["Sheet1"]);
    assert_eq!(range.start(), Some((1, 0)));
    assert_eq!(range.end(), Some((2, 2)));
    assert_eq!(range.get_value((1, 0)), Some(&Data::String("A".into())));
    assert_eq!(range.get_value((1, 2)), Some(&Data::Float(2.0)));
    assert_eq!(range.get_value((2, 0)), Some(&Data::String("B".into())));
    assert_eq!(range.get_value((2, 2)), Some(&Data::Float(3.0)));
    assert_eq!(range.get_value((1, 1)), Some(&Data::Empty));
    assert_eq!(range.get_value((2, 1)), Some(&Data::Empty));
    assert_eq!(std::fs::read(&input)?, bytes);
    app.project.source_options.xlsx_sheet = Some("Missing".into());
    app.save_document_to(&project_path)?;
    directory.record("result.data", b"existing output must remain")?;
    let refusal = cli::run_project_with_paths(&project_path, None, None);
    directory.record("missing-sheet-result.txt", format!("{refusal:#?}"))?;
    let error = refusal.expect_err("missing named worksheet refuses");
    assert!(
        matches!(error.downcast_ref::<format_xlsx::XlsxFormatError>(),
        Some(format_xlsx::XlsxFormatError::MissingWorksheet(name)) if name == "Missing")
    );
    assert_eq!(std::fs::read(&output)?, b"existing output must remain");
    assert_eq!(std::fs::read(&input)?, bytes);
    let absent = directory.path.join("absent.data");
    let refusal = cli::run_project_with_paths(&project_path, None, Some(&absent));
    directory.record("missing-sheet-fresh-result.txt", format!("{refusal:#?}"))?;
    assert!(
        matches!(refusal.expect_err("missing worksheet refuses before fresh publication")
        .downcast_ref::<format_xlsx::XlsxFormatError>(),
        Some(format_xlsx::XlsxFormatError::MissingWorksheet(name)) if name == "Missing")
    );
    assert!(!absent.exists());
    directory.completed = true;
    Ok(())
}

fn desktop_wizard_texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn collect(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        found: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => {
                found.push((
                    text.galley.text().to_owned(),
                    text.visual_bounding_rect(),
                    clip,
                ));
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut found);
    }
    found
}

fn desktop_wizard_visible(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
    desktop_wizard_texts(output)
        .into_iter()
        .find_map(|(text, rect, clip)| (text == wanted && clip.contains_rect(rect)).then_some(rect))
}

fn desktop_wizard_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    directory: &TestDirectory,
    height: f32,
    events: Vec<egui::Event>,
    tag: &str,
) -> anyhow::Result<egui::FullOutput> {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, height),
            )),
            time: Some(context.cumulative_frame_nr() as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| app.show_new_mapping_setup(ui.ctx()),
    );
    directory.record(
        &format!(
            "desktop-{height:.0}-{tag}-{}.txt",
            context.cumulative_frame_nr()
        ),
        format!(
            "fixed viewport=1200x{height}\n{:#?}\n",
            desktop_wizard_texts(&output)
        ),
    )?;
    assert_eq!(
        context.content_rect().height(),
        height,
        "the fixture never enlarges the viewport"
    );
    Ok(output)
}

fn desktop_wizard_reveal(
    app: &mut FerruleApp,
    context: &egui::Context,
    directory: &TestDirectory,
    height: f32,
    wanted: &str,
    direction: f32,
) -> anyhow::Result<egui::Rect> {
    let mut output = desktop_wizard_frame(app, context, directory, height, Vec::new(), "settle")?;
    for _ in 0..4 {
        output = desktop_wizard_frame(app, context, directory, height, Vec::new(), "settle")?;
    }
    for _ in 0..20 {
        if let Some(rect) = desktop_wizard_visible(&output, wanted) {
            return Ok(rect);
        }
        // These ordinary captions are outside both nested field scroll areas.
        // Choose a currently painted full caption, never an off-screen galley
        // or an injected scroll-state offset.
        let anchor = [
            "Workbook table",
            "Input file (optional)",
            "Output file (optional)",
            "Sheet name (optional)",
            "Skip header row",
            "Write header row",
            "Header row",
            "Abandon workbook setup",
            "Source",
            "Target",
        ]
        .iter()
        .find_map(|label| desktop_wizard_visible(&output, label))
        .expect("visible outer-wizard caption for real wheel input")
        .center();
        output = desktop_wizard_frame(
            app,
            context,
            directory,
            height,
            vec![
                egui::Event::PointerMoved(anchor),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, direction),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            "wheel",
        )?;
        for _ in 0..4 {
            output =
                desktop_wizard_frame(app, context, directory, height, Vec::new(), "wheel-idle")?;
        }
    }
    panic!("full visible desktop wizard control {wanted:?} at viewport height {height}");
}

fn desktop_wizard_click(
    app: &mut FerruleApp,
    context: &egui::Context,
    directory: &TestDirectory,
    height: f32,
    wanted: &str,
    direction: f32,
) -> anyhow::Result<()> {
    let rect = desktop_wizard_reveal(app, context, directory, height, wanted, direction)?;
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, height)).contains_rect(rect)
    );
    for pressed in [true, false] {
        desktop_wizard_frame(
            app,
            context,
            directory,
            height,
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            "click",
        )?;
    }
    Ok(())
}

#[test]
fn flat_workbook_outer_scroll_reaches_configure_and_create_at_desktop_heights() -> anyhow::Result<()>
{
    let mut directory = TestDirectory::new()?;
    // Enough ordinary fields to fill each bounded 160-pixel inner field list.
    // The new outer scrolling must expose the real footer in both desktop
    // heights; this is deliberately independent of the old 1800-pixel helper.
    let schema = r#"{"title":"Row","type":"object","properties":{"Name":{"type":"string"},"Count":{"type":"integer"},"City":{"type":"string"},"Country":{"type":"string"},"Product":{"type":"string"},"Code":{"type":"string"},"Region":{"type":"string"},"Note":{"type":"string"}},"additionalProperties":false}"#;
    let source_path = directory.path.join("desktop-source.json");
    let target_path = directory.path.join("desktop-target.json");
    std::fs::write(&source_path, schema)?;
    std::fs::write(&target_path, schema)?;
    for height in [760.0, 800.0] {
        let mut app = FerruleApp::default();
        let original = mapping::project_file::encode_pretty(&app.project)?;
        let original_document = app.document.clone();
        app.begin_new_mapping();
        app.stage_mapping_schema(SchemaSide::Source, source_path.clone());
        app.stage_mapping_schema(SchemaSide::Target, target_path.clone());
        let context = egui::Context::default();
        crate::icons::install(&context);
        desktop_wizard_click(
            &mut app,
            &context,
            &directory,
            height,
            "Configure workbook",
            -150.0,
        )?;
        assert!(matches!(
            app.new_mapping_setup.as_ref().unwrap().source,
            Some(MappingBoundary::Xlsx(_))
        ));
        assert!(matches!(
            app.new_mapping_setup.as_ref().unwrap().target,
            Some(MappingBoundary::Schema(_))
        ));
        desktop_wizard_click(
            &mut app,
            &context,
            &directory,
            height,
            "Configure workbook",
            -150.0,
        )?;
        assert!(matches!(
            app.new_mapping_setup.as_ref().unwrap().target,
            Some(MappingBoundary::Xlsx(_))
        ));
        // Return through real outer-wheel input to the source panel before
        // proving that its distant footer requires scrolling, regardless of
        // the previous target-configuration scroll offset.
        desktop_wizard_reveal(
            &mut app,
            &context,
            &directory,
            height,
            "Input file (optional)",
            150.0,
        )?;
        let mut before_scroll = desktop_wizard_frame(
            &mut app,
            &context,
            &directory,
            height,
            Vec::new(),
            "both-configured",
        )?;
        for _ in 0..4 {
            before_scroll = desktop_wizard_frame(
                &mut app,
                &context,
                &directory,
                height,
                Vec::new(),
                "both-configured",
            )?;
        }
        assert!(
            desktop_wizard_visible(&before_scroll, "Create mapping").is_none(),
            "the full footer begins outside this fixed desktop viewport"
        );
        let footer = desktop_wizard_reveal(
            &mut app,
            &context,
            &directory,
            height,
            "Create mapping",
            -150.0,
        )?;
        assert!(footer.top() >= 0.0 && footer.bottom() <= height);
        desktop_wizard_click(
            &mut app,
            &context,
            &directory,
            height,
            "Skip header row",
            150.0,
        )?;
        assert_eq!(
            staged(&mut app, SchemaSide::Source)
                .options(false)?
                .has_header_row,
            Some(false)
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert_eq!(app.document, original_document);
        assert!(!app.project_editing_enabled());
        desktop_wizard_click(
            &mut app,
            &context,
            &directory,
            height,
            "Create mapping",
            -150.0,
        )?;
        directory.record(
            &format!("desktop-{height:.0}-created.json"),
            mapping::project_file::encode_pretty(&app.project)?,
        )?;
        assert!(app.new_mapping_setup.is_none());
        assert_eq!(
            app.project.source_options.tabular_kind,
            Some(TabularBoundaryKind::Xlsx)
        );
        assert_eq!(
            app.project.target_options.tabular_kind,
            Some(TabularBoundaryKind::Xlsx)
        );
        assert_eq!(
            app.project.source_options.xlsx_columns,
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            app.project.target_options.xlsx_columns,
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(app.project.source_options.has_header_row, Some(false));
        assert_eq!(app.project.target_options.has_header_row, Some(true));
        assert_eq!(
            app.project.target_options.xlsx_headers,
            [
                "Name", "Count", "City", "Country", "Product", "Code", "Region", "Note"
            ]
        );
        assert_eq!(app.project.source_path, None);
        assert_eq!(app.project.target_path, None);
        assert!(app.project.graph.nodes.is_empty());
        assert!(app.project_editing_enabled() && app.is_dirty());
    }
    directory.completed = true;
    Ok(())
}
