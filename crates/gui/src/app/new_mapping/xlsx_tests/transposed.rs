use super::*;

const SCHEMA_N: &str = r#"{"title":"Row","type":"object","properties":{"Name":{"type":"string"},"n":{"type":"integer"},"Count":{"type":"integer"}},"required":["Name","n"],"additionalProperties":false}"#;

fn setup_n(app: &mut FerruleApp, directory: &TestDirectory) -> anyhow::Result<()> {
    let source = directory.path.join("position-source.json");
    let target = directory.path.join("position-target.json");
    std::fs::write(&source, SCHEMA_N)?;
    std::fs::write(&target, SCHEMA_N)?;
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source);
    app.stage_mapping_schema(SchemaSide::Target, target);
    configure(app);
    Ok(())
}

fn expected_position_rows() -> Vec<Instance> {
    vec![
        Instance::Group(
            vec![
                ("Name".into(), Instance::Scalar(Value::String("Zoë".into()))),
                ("n".into(), Instance::Scalar(Value::Int(2))),
                ("Count".into(), Instance::Scalar(Value::Int(7))),
            ]
            .into(),
        ),
        Instance::Group(
            vec![
                ("Name".into(), Instance::Scalar(Value::String("Mia".into()))),
                ("n".into(), Instance::Scalar(Value::Int(4))),
                ("Count".into(), Instance::Scalar(Value::Null)),
            ]
            .into(),
        ),
    ]
}

fn independent_matrix(directory: &TestDirectory) -> anyhow::Result<Vec<u8>> {
    // Independent ordinary-writer cells: row 6 drives B/D; row 2 has B7/F99 and no D.
    let fields = ["A", "B", "C", "D", "E", "F"];
    let schema = SchemaNode::group(
        "Physical",
        fields
            .iter()
            .map(|name| SchemaNode::scalar(*name, ScalarType::String))
            .collect(),
    );
    let rows = (1..=6)
        .map(|row| {
            Instance::Group(
                fields
                    .iter()
                    .enumerate()
                    .map(|(column, name)| {
                        let value = match (row, column + 1) {
                            (2, 2) => Value::String("7".into()),
                            (2, 6) => Value::String("99".into()),
                            (6, 2) => Value::String("Zoë".into()),
                            (6, 4) => Value::String("Mia".into()),
                            _ => Value::Null,
                        };
                        ((*name).into(), Instance::Scalar(value))
                    })
                    .collect::<Vec<_>>()
                    .into(),
            )
        })
        .collect::<Vec<_>>();
    directory.record("independent-physical-matrix.txt", format!("{rows:#?}"))?;
    let bytes = format_xlsx::to_bytes(
        &schema,
        &rows,
        Some("Matrix"),
        1,
        &[1, 2, 3, 4, 5, 6],
        false,
    )?;
    directory.record("independent-matrix.xlsx", &bytes)?;
    Ok(bytes)
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    directory: &TestDirectory,
    height: f32,
    events: Vec<egui::Event>,
) -> anyhow::Result<egui::FullOutput> {
    let output = desktop_wizard_frame(app, context, directory, height, events, "transposed")?;
    directory.record(
        &format!(
            "transposed-accessibility-{height:.0}-{}.txt",
            context.cumulative_frame_nr()
        ),
        format!("{:#?}", output.platform_output.accesskit_update),
    )?;
    Ok(output)
}

fn input_rect(output: &egui::FullOutput, field: &str, current: &str, height: f32) -> egui::Rect {
    let label = format!("Worksheet row for {field}");
    let update = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("actual widget tree");
    let controls = update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::TextInput
                && node.label() == Some(label.as_str()))
            .then_some(node)
        })
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1, "one actual row input for {field}");
    let node = controls[0];
    assert!(!node.is_disabled());
    assert_eq!(node.value(), Some(current));
    let bounds = node.bounds().expect("actual input bounds");
    let rect = egui::Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    );
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, height)).contains_rect(rect)
    );
    let painted = desktop_wizard_texts(output)
        .into_iter()
        .find_map(|(text, text_rect, clip)| {
            (text == current && rect.contains_rect(text_rect) && clip.contains_rect(text_rect))
                .then_some(text_rect)
        })
        .expect("the complete current input text must be painted inside its widget and clip");
    assert!(
        painted.is_positive(),
        "a visible input gesture needs a painted point"
    );
    painted
}

fn edit_row(
    app: &mut FerruleApp,
    context: &egui::Context,
    directory: &TestDirectory,
    height: f32,
    field: &str,
    current: &str,
    next: &str,
) -> anyhow::Result<()> {
    desktop_wizard_reveal(app, context, directory, height, "Worksheet row", 150.0)?;
    let output = frame(app, context, directory, height, Vec::new())?;
    let rect = input_rect(&output, field, current, height);
    for pressed in [true, false] {
        frame(
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
        )?;
    }
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    frame(
        app,
        context,
        directory,
        height,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            },
            egui::Event::Text(next.into()),
        ],
    )?;
    frame(app, context, directory, height, Vec::new())?;
    Ok(())
}

fn select_transposed(
    app: &mut FerruleApp,
    context: &egui::Context,
    directory: &TestDirectory,
    height: f32,
) -> anyhow::Result<()> {
    desktop_wizard_click(app, context, directory, height, "Rows as records", 150.0)?;
    desktop_wizard_click(
        app,
        context,
        directory,
        height,
        "Columns as records (transposed)",
        150.0,
    )?;
    assert!(staged(app, SchemaSide::Source).transposed);
    Ok(())
}

#[test]
fn transposed_real_layout_and_row_gestures_keep_ordinary_source_and_all_target_choices()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup(&mut app, &directory)?;
    configure(&mut app);
    let source = staged(&mut app, SchemaSide::Source);
    source.sheet = "Matrix".into();
    source.start_row = "3".into();
    source.columns = vec!["4".into(), "1".into()];
    source.has_header_row = false;
    source.headers = vec!["parked Name".into(), "parked Count".into()];
    let ordinary = source.options(false)?;
    let target = staged(&mut app, SchemaSide::Target);
    target.sheet = "Results".into();
    target.start_row = "5".into();
    target.columns = vec!["2".into(), "6".into()];
    target.headers = vec!["Person".into(), "Total".into()];
    let target_before = target.options(true)?;
    let original = mapping::project_file::encode_pretty(&app.project)?;
    let original_document = app.document.clone();
    let context = egui::Context::default();
    context.enable_accesskit();
    crate::icons::install(&context);
    select_transposed(&mut app, &context, &directory, 800.0)?;
    edit_row(&mut app, &context, &directory, 800.0, "Name", "1", "6")?;
    edit_row(&mut app, &context, &directory, 800.0, "Count", "2", "2")?;
    let transposed = staged(&mut app, SchemaSide::Source).options(false)?;
    directory.record(
        "gesture-transposed-options.json",
        serde_json::to_vec_pretty(&transposed)?,
    )?;
    assert_eq!(
        transposed,
        mapping::FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            xlsx_sheet: Some("Matrix".into()),
            xlsx_rows: vec![6, 2],
            ..Default::default()
        }
    );
    assert_eq!(
        staged(&mut app, SchemaSide::Target).options(true)?,
        target_before
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
        800.0,
        "Columns as records (transposed)",
        150.0,
    )?;
    desktop_wizard_click(
        &mut app,
        &context,
        &directory,
        800.0,
        "Rows as records",
        150.0,
    )?;
    assert_eq!(
        staged(&mut app, SchemaSide::Source).options(false)?,
        ordinary
    );
    assert_eq!(
        staged(&mut app, SchemaSide::Source).headers,
        ["parked Name", "parked Count"]
    );
    select_transposed(&mut app, &context, &directory, 800.0)?;
    assert_eq!(
        staged(&mut app, SchemaSide::Source).options(false)?,
        transposed
    );
    desktop_wizard_click(
        &mut app,
        &context,
        &directory,
        800.0,
        "Create mapping",
        -150.0,
    )?;
    directory.record(
        "gesture-created-project.json",
        mapping::project_file::encode_pretty(&app.project)?,
    )?;
    assert!(app.new_mapping_setup.is_none());
    assert_eq!(app.project.source_options, transposed);
    assert_eq!(app.project.target_options, target_before);
    directory.completed = true;
    Ok(())
}

#[test]
fn transposed_row_validation_matches_typed_reader_refusals_without_replacing_open_mapping()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup(&mut app, &directory)?;
    configure(&mut app);
    let original = mapping::project_file::encode_pretty(&app.project)?;
    let original_document = app.document.clone();
    staged(&mut app, SchemaSide::Source).transposed = true;
    let schema = staged(&mut app, SchemaSide::Source).schema.clone();
    for (label, raw, numeric) in [
        ("count", vec!["1"], vec![1]),
        ("duplicate", vec!["2", "2"], vec![2, 2]),
        ("zero", vec!["0", "2"], vec![0, 2]),
        ("past maximum", vec!["1048577", "2"], vec![1048577, 2]),
    ] {
        staged(&mut app, SchemaSide::Source).rows = raw.into_iter().map(str::to_owned).collect();
        let options = staged(&mut app, SchemaSide::Source).options(false);
        let reader = format_xlsx::from_bytes_transposed(&[], &schema, None, &numeric);
        directory.record(
            &format!("invalid-{label}-options.txt"),
            format!("{options:#?}"),
        )?;
        directory.record(
            &format!("invalid-{label}-reader.txt"),
            format!("{reader:#?}"),
        )?;
        assert!(options.is_err());
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        if label == "count" {
            assert!(matches!(
                reader,
                Err(format_xlsx::XlsxFormatError::RowCount {
                    expected: 2,
                    got: 1
                })
            ));
        } else {
            assert!(matches!(
                reader,
                Err(format_xlsx::XlsxFormatError::InvalidCoordinate)
            ));
        }
        app.finish_new_mapping();
        assert!(app.new_mapping_setup.is_some());
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert_eq!(app.document, original_document);
    }
    for raw in [vec!["", "2"], vec!["1.5", "2"]] {
        staged(&mut app, SchemaSide::Source).rows = raw.into_iter().map(str::to_owned).collect();
        assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
        app.finish_new_mapping();
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        assert_eq!(app.document, original_document);
    }
    staged(&mut app, SchemaSide::Source).rows = vec!["1048576".into(), "2".into()];
    assert!(app.new_mapping_setup.as_ref().unwrap().can_create());
    let bytes = independent_matrix(&directory)?;
    let beyond = format_xlsx::from_bytes_transposed(&bytes, &schema, Some("Matrix"), &[1048576, 2]);
    directory.record("valid-row-beyond-used-extent.txt", format!("{beyond:#?}"))?;
    assert_eq!(beyond?, Vec::<Instance>::new());
    staged(&mut app, SchemaSide::Target).transposed = true;
    assert!(!app.new_mapping_setup.as_ref().unwrap().can_create());
    app.finish_new_mapping();
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        original
    );
    assert_eq!(app.document, original_document);
    directory.completed = true;
    Ok(())
}

#[test]
fn transposed_position_is_existing_integer_n_and_selector_count_excludes_it() -> anyhow::Result<()>
{
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup_n(&mut app, &directory)?;
    let source = staged(&mut app, SchemaSide::Source);
    assert!(source.supports_transposed());
    assert_eq!(source.rows, ["1", "2"]);
    source.transposed = true;
    source.rows = vec!["6".into(), "2".into()];
    let bytes = independent_matrix(&directory)?;
    let actual = format_xlsx::from_bytes_transposed(
        &bytes,
        &source.schema,
        Some("Matrix"),
        &source.options(false)?.xlsx_rows,
    );
    directory.record(
        "position-complete-reader-result.txt",
        format!("{actual:#?}"),
    )?;
    let expected = expected_position_rows();
    directory.record("position-manual-oracle.txt", format!("{expected:#?}"))?;
    assert_eq!(actual?, expected);
    let plain_schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    );
    let plain = format_xlsx::from_bytes_transposed(&bytes, &plain_schema, Some("Matrix"), &[6, 2]);
    let plain_expected = vec![
        row("Zoë", 7),
        Instance::Group(
            vec![
                ("Name".into(), Instance::Scalar(Value::String("Mia".into()))),
                ("Count".into(), Instance::Scalar(Value::Null)),
            ]
            .into(),
        ),
    ];
    directory.record(
        "without-position-complete-reader-result.txt",
        format!("{plain:#?}"),
    )?;
    directory.record(
        "without-position-manual-oracle.txt",
        format!("{plain_expected:#?}"),
    )?;
    assert_eq!(plain?, plain_expected);
    for schema in [
        SchemaNode::group(
            "Row",
            vec![
                SchemaNode::scalar("n", ScalarType::Float),
                SchemaNode::scalar("Name", ScalarType::String),
            ],
        ),
        SchemaNode::group("Row", vec![SchemaNode::scalar("n", ScalarType::Int)]),
    ] {
        let imported = crate::new_mapping::ImportedSchema {
            path: directory.path.join("not-opened.json"),
            schema,
        };
        let reader_rows = if matches!(&imported.schema.kind,
            ir::SchemaKind::Group { children, .. } if children.len() == 1)
        {
            vec![]
        } else {
            vec![1]
        };
        let reader = format_xlsx::from_bytes_transposed(&[], &imported.schema, None, &reader_rows);
        directory.record(
            &format!("ineligible-reader-{}.txt", reader_rows.len()),
            format!("{reader:#?}"),
        )?;
        if reader_rows.is_empty() {
            assert!(matches!(
                reader,
                Err(format_xlsx::XlsxFormatError::InvalidCoordinate)
            ));
        } else {
            assert!(matches!(
                reader,
                Err(format_xlsx::XlsxFormatError::UnsupportedSchema)
            ));
        }
        let mut draft = XlsxBoundaryDraft::from_imported(&imported)?;
        assert!(draft.validate().is_ok(), "ordinary mode remains available");
        assert!(!draft.supports_transposed());
        draft.transposed = true;
        let selected = draft.options(false);
        directory.record(
            &format!("ineligible-latest-result-{}.txt", reader_rows.len()),
            format!("{selected:#?}"),
        )?;
        assert!(selected.is_err());
    }
    assert!(!XlsxBoundaryDraft::supports_schema(&SchemaNode::group(
        "Row",
        Vec::new()
    )));
    directory.completed = true;
    Ok(())
}

#[test]
fn transposed_created_options_history_save_reopen_file_and_payload_keep_literal_cells()
-> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup_n(&mut app, &directory)?;
    let input = directory.path.join("matrix.xlsx");
    let output = directory.path.join("results.xlsx");
    let project_path = directory.path.join("transposed.json");
    let source = staged(&mut app, SchemaSide::Source);
    source.transposed = true;
    source.rows = vec!["6".into(), "2".into()];
    source.sheet = "Matrix".into();
    source.data_path = input.to_str().unwrap().into();
    let source_options = source.options(false)?;
    let target = staged(&mut app, SchemaSide::Target);
    target.sheet = "Results".into();
    target.start_row = "5".into();
    target.columns = vec!["2".into(), "4".into(), "6".into()];
    target.headers = vec!["Person".into(), "Position".into(), "Total".into()];
    target.data_path = output.to_str().unwrap().into();
    let target_options = target.options(true)?;
    assert!(
        !input.exists() && !output.exists(),
        "staging does not acquire a workbook"
    );
    app.finish_new_mapping();
    assert!(app.new_mapping_setup.is_none());
    // Creation is not an undo command: add ordinary graph bindings afterwards.
    app.project.root.iteration = ScopeIteration::Source(Vec::new());
    for (id, name) in [(1, "Name"), (2, "n"), (3, "Count")] {
        app.project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.into()],
                frame: None,
            },
        );
        app.project.root.bindings.push(Binding {
            target_field: name.into(),
            node: id,
        });
    }
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    app.redo_project();
    assert_eq!(app.project.graph.nodes.len(), 3);
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    let validation = cli::validate(&app.project);
    directory.record(
        "complete-created-project-validation.txt",
        format!("{validation:#?}"),
    )?;
    assert!(validation.is_empty());
    app.save_document_to(&project_path)?;
    let saved = mapping::project_file::encode_pretty(&app.project)?;
    std::fs::remove_file(directory.path.join("position-source.json"))?;
    std::fs::remove_file(directory.path.join("position-target.json"))?;
    app.load_project_from(&project_path);
    directory.record("saved-complete-project.json", &saved)?;
    assert_eq!(mapping::project_file::encode_pretty(&app.project)?, saved);
    assert_eq!(app.project.source_options, source_options);
    assert_eq!(app.project.target_options, target_options);
    assert!(!app.is_dirty());
    let bytes = independent_matrix(&directory)?;
    directory.record("matrix.xlsx", &bytes)?;
    let expected = expected_position_rows();
    let typed = engine::run(&app.project, &Instance::Repeated(expected.clone()));
    directory.record(
        "manual-complete-target-oracle.txt",
        format!("{expected:#?}"),
    )?;
    directory.record("original-engine-result.txt", format!("{typed:#?}"))?;
    assert_eq!(typed?, Instance::Repeated(expected));
    let file = cli::run_project_with_paths(&project_path, None, None);
    directory.record("original-file-run-result.txt", format!("{file:#?}"))?;
    file?;
    let written = std::fs::read(&output)?;
    check_output(&directory, "file", &written)?;
    let payload = cli::run_project_value_payloads(
        &app.project,
        &project_path,
        &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&input, &bytes)?),
    );
    directory.record("original-payload-run-result.txt", format!("{payload:#?}"))?;
    let payload = payload?;
    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].path, output);
    directory.record("payload.xlsx", &payload.artifacts[0].bytes)?;
    check_output(&directory, "payload", &payload.artifacts[0].bytes)?;
    assert_eq!(std::fs::read(&input)?, bytes);
    assert_eq!(std::fs::read(&output)?, written);
    directory.completed = true;
    Ok(())
}

fn check_output(directory: &TestDirectory, tag: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let sheets = workbook.sheet_names().to_vec();
    let range = workbook.worksheet_range("Results")?;
    directory.record(
        &format!("{tag}-complete-physical-readback.txt"),
        format!("{sheets:?}\n{range:#?}"),
    )?;
    assert_eq!(sheets, ["Results"]);
    assert_eq!(range.start(), Some((4, 1)));
    assert_eq!(range.end(), Some((6, 5)));
    for row in 0..8 {
        for column in 0..8 {
            let expected = match (row, column) {
                (4, 1) => Data::String("Person".into()),
                (4, 3) => Data::String("Position".into()),
                (4, 5) => Data::String("Total".into()),
                (5, 1) => Data::String("Zoë".into()),
                (5, 3) => Data::Float(2.0),
                (5, 5) => Data::Float(7.0),
                (6, 1) => Data::String("Mia".into()),
                (6, 3) => Data::Float(4.0),
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
fn transposed_mode_reaches_real_rows_and_create_at_fixed_desktop_viewports() -> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    for height in [760.0, 800.0] {
        let mut app = FerruleApp::default();
        setup_n(&mut app, &directory)?;
        let original = mapping::project_file::encode_pretty(&app.project)?;
        let target = staged(&mut app, SchemaSide::Target).options(true)?;
        let context = egui::Context::default();
        context.enable_accesskit();
        crate::icons::install(&context);
        select_transposed(&mut app, &context, &directory, height)?;
        edit_row(&mut app, &context, &directory, height, "Name", "1", "6")?;
        edit_row(&mut app, &context, &directory, height, "Count", "2", "2")?;
        let output = frame(&mut app, &context, &directory, height, Vec::new())?;
        assert!(desktop_wizard_visible(&output, "Physical column number (gaps kept)").is_some());
        assert!(desktop_wizard_visible(&output, "Skip header row").is_none());
        assert_eq!(
            mapping::project_file::encode_pretty(&app.project)?,
            original
        );
        desktop_wizard_click(
            &mut app,
            &context,
            &directory,
            height,
            "Create mapping",
            -150.0,
        )?;
        assert!(app.new_mapping_setup.is_none());
        assert_eq!(app.project.source_options.xlsx_rows, [6, 2]);
        assert_eq!(app.project.target_options, target);
        assert_eq!(context.content_rect().height(), height);
    }
    directory.completed = true;
    Ok(())
}

#[test]
fn transposed_read_layout_under_disabled_ui_preserves_all_staged_values() -> anyhow::Result<()> {
    let mut directory = TestDirectory::new()?;
    let mut app = FerruleApp::default();
    setup_n(&mut app, &directory)?;
    let draft = staged(&mut app, SchemaSide::Source);
    let before = draft.options(false)?;
    let rows = draft.rows.clone();
    let context = egui::Context::default();
    context.enable_accesskit();
    crate::icons::install(&context);
    let render = |events, draft: &mut XlsxBoundaryDraft| {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(false, |ui| show_options(ui, draft, false));
            },
        )
    };
    let mut output = render(Vec::new(), draft);
    for _ in 0..4 {
        output = render(Vec::new(), draft);
    }
    directory.record(
        "disabled-complete-widget-tree.txt",
        format!("{:#?}", output.platform_output.accesskit_update),
    )?;
    let update = output.platform_output.accesskit_update.as_ref().unwrap();
    let controls = update
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::ComboBox
                && node.value() == Some("Rows as records"))
            .then_some(node)
        })
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1);
    let node = controls[0];
    assert!(node.is_disabled());
    let b = node.bounds().unwrap();
    let point = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
    for pressed in [true, false] {
        render(
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            draft,
        );
    }
    assert!(!egui::Popup::is_any_open(&context));
    assert!(!draft.transposed);
    assert_eq!(draft.rows, rows);
    assert_eq!(draft.options(false)?, before);
    directory.completed = true;
    Ok(())
}
