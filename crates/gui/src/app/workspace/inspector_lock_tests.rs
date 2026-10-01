use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, NamedTarget, Scope};

fn inspector_app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    let rows = SchemaNode::group(
        "Rows",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .repeating();
    app.project.source = SchemaNode::group("Source", vec![rows.clone()]);
    app.project.target = SchemaNode::group("Target", vec![rows]);
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["Rows".into(), "Value".into()],
            frame: None,
        },
    );
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: mapping::ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    app.project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some("other.json".into()),
        schema: app.project.target.clone(),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: app.project.root.clone(),
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
    }
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn source() -> Instance {
    Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(
            ["first", "second"]
                .into_iter()
                .map(|value| {
                    Instance::Group(vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String(value.into())),
                    )])
                })
                .collect(),
        ),
    )])
}

fn active_scope(app: &FerruleApp) -> &Scope {
    let root = match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    };
    &root.children[0]
}

fn rows(instance: &Instance) -> usize {
    let Instance::Group(fields) = instance else {
        panic!("group output");
    };
    let (_, Instance::Repeated(rows)) = fields
        .iter()
        .find(|(name, _)| name == "Rows")
        .expect("Rows output")
    else {
        panic!("repeated Rows output");
    };
    rows.len()
}

fn rendered_label(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            Some(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            shapes.iter().find_map(|shape| rendered_label(shape, label))
        }
        _ => None,
    }
}

fn inspector_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, editing_enabled),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn click_inspector(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    label: &str,
) {
    let mut output = inspector_frame(app, context, editing_enabled, Vec::new());
    for _ in 0..3 {
        output = inspector_frame(app, context, editing_enabled, Vec::new());
    }
    let position = output
        .shapes
        .iter()
        .find_map(|shape| rendered_label(&shape.shape, label))
        .unwrap_or_else(|| panic!("rendered Inspector control {label:?}"));
    for pressed in [true, false] {
        let _ = inspector_frame(
            app,
            context,
            editing_enabled,
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

#[test]
fn locked_primary_and_named_inspectors_preserve_project_execution_and_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = inspector_app(document);
        app.begin_preview();
        assert!(app.preview_draft.is_some(), "preview setup takes edit lock");
        let before = mapping::project_file::encode_pretty(&app.project).expect("project state");
        let expected = engine::run_outputs(&app.project, &source()).expect("valid mapping");
        let context = egui::Context::default();
        crate::icons::install(&context);
        for label in ["filtered", "sorted", "iterates", "x"] {
            click_inspector(&mut app, &context, false, label);
            assert_eq!(
                mapping::project_file::encode_pretty(&app.project).expect("project state"),
                before,
                "locked {document:?} control {label:?}",
            );
            assert!(!app.is_dirty());
            assert!(!app.can_undo());
            assert!(!app.history.can_redo());
            assert!(app.pending_history.is_none());
            assert!(app.preview_draft.is_some());
        }
        assert_eq!(active_scope(&app).filter, None);
        let actual = engine::run_outputs(&app.project, &source()).expect("still valid mapping");
        assert_eq!(actual.primary, expected.primary);
        assert_eq!(actual.extras, expected.extras);
    }
}

#[test]
fn unlocked_primary_and_named_inspectors_filter_rows_and_support_undo_redo() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = inspector_app(document);
        let before = engine::run_outputs(&app.project, &source()).expect("valid mapping");
        assert_eq!(rows(&before.primary), 2);
        assert_eq!(rows(&before.extras[0].instance), 2);
        let context = egui::Context::default();
        crate::icons::install(&context);
        click_inspector(&mut app, &context, true, "filtered");
        assert_eq!(active_scope(&app).filter, Some(0));
        assert!(app.is_dirty());
        assert!(app.can_undo());
        assert!(cli::validate(&app.project).is_empty());
        let filtered = engine::run_outputs(&app.project, &source()).expect("filtered mapping");
        match document {
            MappingDocument::Main => {
                assert_eq!(rows(&filtered.primary), 0);
                assert_eq!(rows(&filtered.extras[0].instance), 2);
            }
            MappingDocument::Target(_) => {
                assert_eq!(rows(&filtered.primary), 2);
                assert_eq!(rows(&filtered.extras[0].instance), 0);
            }
            MappingDocument::Function(_) => unreachable!(),
        }
        app.undo_project();
        assert_eq!(active_scope(&app).filter, None);
        assert!(!app.is_dirty());
        let restored = engine::run_outputs(&app.project, &source()).expect("restored mapping");
        assert_eq!(restored.primary, before.primary);
        assert_eq!(restored.extras, before.extras);
        app.redo_project();
        assert!(app.is_dirty());
        let redone = engine::run_outputs(&app.project, &source()).expect("redone mapping");
        assert_eq!(redone.primary, filtered.primary);
        assert_eq!(redone.extras, filtered.extras);
    }
}
