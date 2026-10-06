use super::*;

fn app() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = super::primary_xml_input::tests::schema();
    app.project.target = ir::SchemaNode::group(
        "Result",
        vec![ir::SchemaNode::scalar("Code", ir::ScalarType::String)],
    );
    // Absolute hints keep this policy persistence check independent of Save As
    // rebasing relative instance paths, which has separate coverage.
    let instance_hints = std::env::temp_dir().join("ferrule-root-policy-instance-hints");
    app.project.source_path = Some(
        instance_hints
            .join("input.xml")
            .to_string_lossy()
            .into_owned(),
    );
    app.project.target_path = Some(
        instance_hints
            .join("output.json")
            .to_string_lossy()
            .into_owned(),
    );
    app.project.source_options = Default::default();
    app.project.target_options = mapping::FormatOptions {
        json_document: true,
        ..Default::default()
    };
    app.project.graph.nodes.clear();
    app.project.graph.nodes.insert(
        1,
        mapping::Node::Const {
            value: ir::Value::String("safe".into()),
        },
    );
    app.project.root = mapping::Scope {
        bindings: vec![mapping::Binding {
            target_field: "Code".into(),
            node: 1,
        }],
        ..Default::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    app
}
fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_source_explorer(ui, editing),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}
fn click(app: &mut FerruleApp, context: &egui::Context, editing: bool, label: &str) {
    fn collect(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| collect(shape, label))
            }
            _ => None,
        }
    }
    let mut output = frame(app, context, editing, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing, Vec::new());
    }
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| collect(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing source policy {label:?}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            editing,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn pointer_root_policy_action_is_atomic_undoable_and_survives_save_reopen() -> anyhow::Result<()> {
    let mut app = app();
    let before = serde_json::to_value(&app.project)?;
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, true, "Primary XML root input");
    click(&mut app, &context, true, "Retain observed root annotations");
    let changed = serde_json::to_value(&app.project)?;
    assert!(
        app.project.source_options.xml_document
            && app
                .project
                .source_options
                .xml_allow_inactive_root_type_members
            && app.project.source_options.xml_root_view_read_policy
    );
    assert!(app.is_dirty());
    assert!(cli::validate(&app.project).is_empty());
    app.undo_project();
    assert_eq!(serde_json::to_value(&app.project)?, before);
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(serde_json::to_value(&app.project)?, changed);
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-root-policy-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory)?;
    let path = directory.join("project.json");
    let saved = app.save_document_to(&path)?;
    assert!(saved.validation_issues.is_empty());
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    assert_eq!(serde_json::to_value(&reopened.project)?, changed);
    assert!(!reopened.is_dirty());
    std::fs::remove_dir_all(path.parent().unwrap())?;
    Ok(())
}

#[test]
fn locked_source_policy_panel_keeps_project_and_history_unchanged() {
    let mut app = app();
    let before = serde_json::to_value(&app.project).unwrap();
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, false, "Primary XML root input");
    click(
        &mut app,
        &context,
        false,
        "Retain observed root annotations",
    );
    assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
}

#[test]
fn loaded_incompatible_root_policy_remains_visible_and_is_not_repaired_by_rendering() {
    let mut app = app();
    app.project.source_options.json_document = true;
    app.project.source_options.xml_root_view_read_policy = true;
    app.mark_clean();
    app.rebase_history();
    let before = serde_json::to_value(&app.project).unwrap();
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, true, "Primary XML root input");
    let output = frame(&mut app, &context, true, Vec::new());
    fn has_text(shape: &egui::epaint::Shape, needle: &str) -> bool {
        match shape {
            egui::epaint::Shape::Text(text) => text.galley.text().contains(needle),
            egui::epaint::Shape::Vec(shapes) => shapes.iter().any(|shape| has_text(shape, needle)),
            _ => false,
        }
    }
    assert!(output.shapes.iter().any(|shape| has_text(
        &shape.shape,
        "cannot be combined with other format settings"
    )));
    assert!(output.shapes.iter().any(|shape| has_text(
        &shape.shape,
        "missing its XML or declared-field prerequisite"
    )));
    assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
    click(&mut app, &context, true, "Retain observed root annotations");
    assert!(!app.project.source_options.xml_root_view_read_policy);
    assert!(app.project.source_options.json_document);
    assert!(app.is_dirty());
}

#[test]
fn source_field_popup_keyboard_edit_saves_pending_history_and_roundtrips_undo_redo()
-> anyhow::Result<()> {
    fn text_position(shape: &egui::epaint::Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(value) if value.galley.text() == text => {
                Some(value.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| text_position(shape, text))
            }
            _ => None,
        }
    }
    fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| text_position(&shape.shape, text))
            .unwrap_or_else(|| panic!("missing actual canvas widget {text:?}"))
    }
    fn canvas_frame(
        app: &mut FerruleApp,
        context: &egui::Context,
        now: std::time::Instant,
        coalesce: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let output = context.run_ui(
            egui::RawInput {
                time: Some(context.cumulative_frame_nr() as f64 / 10.0),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.show_main_canvas(ui, true),
        );
        eprintln!(
            "SourceField app canvas original: project={}; layout={:?}; shapes={:?}",
            serde_json::to_string(&app.project).unwrap(),
            editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace).layout,
            output.shapes,
        );
        app.observe_editor_history(now, coalesce);
        eprintln!(
            "SourceField app observed history: dirty={}; pending={}; undo={}; redo={}",
            app.is_dirty(),
            app.pending_history.is_some(),
            app.history.undo_len(),
            app.history.redo_len(),
        );
        output
    }
    fn click_canvas(
        app: &mut FerruleApp,
        context: &egui::Context,
        now: std::time::Instant,
        position: egui::Pos2,
    ) {
        for pressed in [true, false] {
            canvas_frame(
                app,
                context,
                now,
                true,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    let mut app = app();
    app.project.source = ir::SchemaNode::group(
        "Source",
        vec![ir::SchemaNode::scalar("known", ir::ScalarType::String)],
    );
    app.project.graph.nodes.clear();
    // An unresolved draft field is a visible ordinary mapping node. Saving it
    // must retain the authoring state independently of later schema correction.
    app.project.graph.nodes.insert(
        0,
        mapping::Node::SourceField {
            path: vec!["draft".into(), "name".into()],
            frame: None,
        },
    );
    app.project.root.bindings[0].node = 0;
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    let context = egui::Context::default();
    crate::icons::install(&context);
    let now = std::time::Instant::now();
    let mut output = canvas_frame(&mut app, &context, now, false, Vec::new());
    for _ in 0..7 {
        output = canvas_frame(&mut app, &context, now, false, Vec::new());
    }
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    click_canvas(&mut app, &context, now, position(&output, &pencil));
    for _ in 0..4 {
        output = canvas_frame(&mut app, &context, now, false, Vec::new());
    }
    click_canvas(&mut app, &context, now, position(&output, "draft/name"));
    // Establish the save point after the real canvas and editor have settled.
    app.mark_clean();
    app.rebase_history();
    let before = app.project.clone();
    let before_json = serde_json::to_value(&before)?;
    canvas_frame(
        &mut app,
        &context,
        now,
        true,
        vec![
            egui::Event::Key {
                key: egui::Key::End,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Text("_up".into()),
        ],
    );
    canvas_frame(
        &mut app,
        &context,
        now + std::time::Duration::from_millis(100),
        true,
        vec![egui::Event::Text("dated".into())],
    );
    let changed = serde_json::to_value(&app.project)?;
    let mut expected = before;
    expected.graph.nodes.insert(
        0,
        mapping::Node::SourceField {
            path: vec!["draft".into(), "name_updated".into()],
            frame: None,
        },
    );
    eprintln!("SourceField app complete edit originals: before={before_json}; after={changed}");
    assert_eq!(changed, serde_json::to_value(&expected)?);
    assert!(app.is_dirty());
    assert!(app.pending_history.is_some());
    assert_eq!(app.history.undo_len(), 0);
    assert!(app.can_undo());

    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-source-field-app-history-{}-{nonce}",
        std::process::id(),
    ));
    std::fs::create_dir(&directory)?;
    let path = directory.join("project.json");
    let save = app.save_document_to(&path)?;
    let saved_bytes = std::fs::read(&path)?;
    eprintln!(
        "SourceField app save originals: bytes={saved_bytes:?}; validation={:?}; layout_warning={:?}; pending={}",
        save.validation_issues,
        save.layout_warning,
        app.pending_history.is_some(),
    );
    assert!(save.layout_warning.is_none());
    assert!(app.pending_history.is_some());
    assert!(!app.is_dirty());
    app.undo_project();
    let undone = serde_json::to_value(&app.project)?;
    eprintln!(
        "SourceField app undo original: project={undone}; dirty={}",
        app.is_dirty()
    );
    assert_eq!(undone, before_json);
    assert!(app.is_dirty());
    assert_eq!(app.history.undo_len(), 0);
    assert_eq!(app.history.redo_len(), 1);
    app.redo_project();
    let redone = serde_json::to_value(&app.project)?;
    eprintln!(
        "SourceField app redo original: project={redone}; dirty={}",
        app.is_dirty()
    );
    assert_eq!(redone, changed);
    assert!(!app.is_dirty());
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.redo_len(), 0);
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    let reopened_json = serde_json::to_value(&reopened.project)?;
    eprintln!(
        "SourceField app reopen original: project={reopened_json}; dirty={}; status={}",
        reopened.is_dirty(),
        reopened.status,
    );
    assert_eq!(reopened_json, changed);
    assert!(!reopened.is_dirty());
    assert!(!reopened.can_undo());
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}
