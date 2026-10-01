use super::*;
use mapping::{FormatOptions, NamedTarget, Scope};

#[derive(Clone, Copy)]
enum FunctionWindow {
    Navigator,
    Creation,
}

fn draft(name: &str) -> NewFunctionDraft {
    NewFunctionDraft {
        library: "local".to_owned(),
        name: name.to_owned(),
        description: String::new(),
        parameters: vec![ParameterDraft {
            name: "text".to_owned(),
            ty: ScalarType::String,
        }],
        output_name: "result".to_owned(),
        output_type: ScalarType::String,
        error: None,
    }
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    window: FunctionWindow,
    editing_enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| match window {
            FunctionWindow::Navigator => {
                app.show_function_navigator(ui.ctx(), editing_enabled);
            }
            FunctionWindow::Creation => {
                app.show_new_function_dialog(ui.ctx(), editing_enabled);
            }
        },
    )
}

fn label_center(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            Some(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            shapes.iter().find_map(|shape| label_center(shape, label))
        }
        _ => None,
    }
}

fn click(
    app: &mut FerruleApp,
    context: &egui::Context,
    window: FunctionWindow,
    editing_enabled: bool,
    label: &str,
) {
    // Let egui finish sizing the window before using its actual rendered label.
    let mut output = frame(app, context, window, editing_enabled, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, window, editing_enabled, Vec::new());
    }
    let position = output
        .shapes
        .iter()
        .find_map(|shape| label_center(&shape.shape, label))
        .unwrap_or_else(|| panic!("rendered control {label:?}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            window,
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

fn project_state(app: &FerruleApp) -> String {
    serde_json::to_string(&app.project).expect("project state")
}

fn named_target() -> NamedTarget {
    NamedTarget {
        name: "Other".to_owned(),
        path: Some("other.json".to_owned()),
        schema: SchemaNode::group("Other", Vec::new()),
        options: FormatOptions::default(),
        root: Scope::default(),
    }
}

#[test]
fn locked_navigator_cannot_add_calls_to_main_named_or_function_graphs() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = FerruleApp::default();
        let function = app.create_function(&draft("identity")).expect("function");
        app.project.extra_targets.push(named_target());
        app.mapping_workspace.active = document;
        app.show_function_navigator = true;
        let before = project_state(&app);
        let original_nodes = app.project.graph.nodes.len();
        let body_nodes = app.project.user_functions[&function].body.nodes.len();
        let context = egui::Context::default();

        click(
            &mut app,
            &context,
            FunctionWindow::Navigator,
            false,
            "Add call",
        );
        assert_eq!(project_state(&app), before, "locked {document:?}");

        click(
            &mut app,
            &context,
            FunctionWindow::Navigator,
            true,
            "Add call",
        );
        assert_eq!(app.project.graph.nodes.len(), original_nodes + 2);
        assert_eq!(
            app.project.user_functions[&function].body.nodes.len(),
            body_nodes,
        );
        assert!(app.project.graph.nodes.values().any(|node| {
            matches!(node, Node::UserFunctionCall { function: id, args } if *id == function && args.len() == 1)
        }));
    }

    let mut app = FerruleApp::default();
    let caller = app.create_function(&draft("caller")).expect("caller");
    let callee = app.create_function(&draft("callee")).expect("callee");
    app.open_function_tab(caller);
    app.show_function_navigator = true;
    app.function_search = "callee".to_owned();
    let before = project_state(&app);
    let body_nodes = app.project.user_functions[&caller].body.nodes.len();
    let context = egui::Context::default();

    click(
        &mut app,
        &context,
        FunctionWindow::Navigator,
        false,
        "Add call",
    );
    assert_eq!(project_state(&app), before, "locked isolated body");
    click(
        &mut app,
        &context,
        FunctionWindow::Navigator,
        true,
        "Add call",
    );
    assert!(app.project.graph.nodes.is_empty());
    let body = &app.project.user_functions[&caller].body;
    assert_eq!(body.nodes.len(), body_nodes + 2);
    assert!(body.nodes.values().any(|node| {
        matches!(node, Node::UserFunctionCall { function, args } if *function == callee && args.len() == 1)
    }));
}

#[test]
fn navigator_keeps_viewing_available_and_blocks_new_function_when_locked() {
    let mut app = FerruleApp::default();
    let function = app.create_function(&draft("identity")).expect("function");
    app.show_function_navigator = true;
    let before = project_state(&app);
    let context = egui::Context::default();

    click(
        &mut app,
        &context,
        FunctionWindow::Navigator,
        false,
        "New function",
    );
    assert!(app.new_function_draft.is_none());
    click(
        &mut app,
        &context,
        FunctionWindow::Navigator,
        false,
        "local:identity",
    );
    assert_eq!(
        app.mapping_workspace.active,
        MappingDocument::Function(function)
    );
    assert_eq!(project_state(&app), before, "opening is metadata-only");

    click(
        &mut app,
        &context,
        FunctionWindow::Navigator,
        true,
        "New function",
    );
    assert!(app.new_function_draft.is_some());
    assert_eq!(project_state(&app), before, "a draft is not yet a function");
}

#[test]
fn preexisting_new_function_draft_cannot_create_until_editing_resumes() {
    let mut app = FerruleApp {
        new_function_draft: Some(draft("created_after_unlock")),
        ..Default::default()
    };
    let before = project_state(&app);
    let context = egui::Context::default();

    click(
        &mut app,
        &context,
        FunctionWindow::Creation,
        false,
        "Create",
    );
    assert_eq!(project_state(&app), before);
    assert_eq!(
        app.new_function_draft
            .as_ref()
            .expect("retained draft")
            .name,
        "created_after_unlock",
    );
    click(&mut app, &context, FunctionWindow::Creation, true, "Create");
    assert!(app.new_function_draft.is_none());
    assert_eq!(app.project.user_functions.len(), 1);
    let (&function, definition) = app
        .project
        .user_functions
        .first_key_value()
        .expect("created");
    assert_eq!(definition.name, "created_after_unlock");
    assert_eq!(
        app.mapping_workspace.active,
        MappingDocument::Function(function)
    );
}

#[test]
fn locked_new_function_draft_can_still_be_cancelled() {
    let mut app = FerruleApp {
        new_function_draft: Some(draft("cancelled")),
        ..Default::default()
    };
    let before = project_state(&app);
    let context = egui::Context::default();
    click(
        &mut app,
        &context,
        FunctionWindow::Creation,
        false,
        "Cancel",
    );
    assert!(app.new_function_draft.is_none());
    assert_eq!(project_state(&app), before);
}
