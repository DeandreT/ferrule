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
