use super::*;
use crate::test_ui::ContextExt as _;
use ir::{ScalarType, SchemaNode, Value};

fn fixture() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("Example".into()),
        },
    );
    app.project.root.bindings.push(mapping::Binding {
        target_field: "Value".into(),
        node: 0,
    });
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "Copy".into(),
        path: Some("copy.json".into()),
        schema: app.project.target.clone(),
        options: mapping::FormatOptions {
            json_document: true,
            ..Default::default()
        },
        root: app.project.root.clone(),
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    app
}

fn frame(
    app: &mut FerruleApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            egui::Panel::right("inspector_test_panel")
                .exact_size(360.0)
                .show(ui, |ui| {
                    crate::ui_kit::pane(ui, "inspector", |ui| app.show_inspector(ui, true));
                });
        },
    )
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn visit(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) if clip.contains(text.visual_bounding_rect().center()) => {
                out.push((text.galley.text().into(), text.visual_bounding_rect()))
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut out);
    }
    out
}

fn click(app: &mut FerruleApp, ctx: &egui::Context, size: egui::Vec2, label: &str) {
    let output = frame(app, ctx, size, Vec::new());
    let point = texts(&output)
        .into_iter()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("visible {label}"))
        .1
        .center();
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            size,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn inspector_gives_scope_editor_flexible_height_at_desktop_sizes() {
    for size in [egui::vec2(1600.0, 1000.0), egui::vec2(1280.0, 800.0)] {
        let mut app = fixture();
        let ctx = egui::Context::default();
        crate::icons::install(&ctx);
        for _ in 0..4 {
            frame(&mut app, &ctx, size, Vec::new());
        }
        let output = frame(&mut app, &ctx, size, Vec::new());
        let visible = texts(&output);
        let binding = visible
            .iter()
            .find(|(text, _)| text == "Binding 1")
            .expect("binding editor visible without scrolling");
        assert!(
            binding.1.bottom() < size.y - 200.0,
            "the editor retains useful height"
        );
        assert!(
            !visible
                .iter()
                .any(|(text, _)| text == "Target schema" || text.starts_with("Failure rules"))
        );
        assert!(!app.is_dirty());
        click(&mut app, &ctx, size, "Schema");
        assert!(
            texts(&frame(&mut app, &ctx, size, Vec::new()))
                .iter()
                .any(|(text, _)| text == "Target schema")
        );
        click(&mut app, &ctx, size, "Rules");
        let visible = texts(&frame(&mut app, &ctx, size, Vec::new()));
        assert!(visible.iter().any(|(text, _)| text == "Failure rules (0)"));
        assert!(
            !visible.iter().any(|(text, _)| text == "Add rule"),
            "empty rules start collapsed"
        );
        assert!(!app.is_dirty());
    }
}

#[test]
fn target_navigation_is_not_an_edit_but_canvas_moves_still_are() {
    let mut app = fixture();
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(!app.is_dirty());
    assert!(!app.observed_dirty());
    assert!(!app.can_undo());
    let live = app.live_state();
    let saved = app.saved_state.as_ref().unwrap();
    assert!(
        saved == &live && live == *saved,
        "navigation equality is symmetric"
    );
    let canvas = app.mapping_workspace.target_canvases.get_mut(&0).unwrap();
    let id = canvas.snarl.node_ids().next().unwrap().0;
    let before = canvas.snarl.get_node_info(id).unwrap().pos;
    canvas.snarl.get_node_info_mut(id).unwrap().pos.x += 50.0;
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(app.is_dirty());
    assert!(app.observed_dirty());
    assert!(app.can_undo());
    let layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    assert!(
        layout.target_nodes[&0]
            .iter()
            .any(|node| node.x == before.x + 50.0)
    );
    app.undo_project();
    assert!(!app.is_dirty());
    app.redo_project();
    assert!(app.is_dirty());
}

#[test]
fn function_inspector_shows_signature_instead_of_mapping_controls() {
    let mut app = fixture();
    let id = mapping::FunctionId::new(100);
    app.project.user_functions.insert(
        id,
        mapping::UserFunction {
            library: "local".into(),
            name: "Format amount".into(),
            description: Some("Formats the output amount.".into()),
            parameters: vec![mapping::FunctionParameter {
                id: mapping::FunctionParameterId::new(1),
                name: "Amount".into(),
                ty: ScalarType::Float,
            }],
            output_name: "Text".into(),
            output_type: ScalarType::String,
            body: mapping::Graph {
                nodes: [(
                    0,
                    Node::Const {
                        value: Value::String("Example".into()),
                    },
                )]
                .into(),
            },
            output: 0,
        },
    );
    app.mark_clean();
    app.rebase_history();
    app.open_function_tab(id);
    assert!(app.ensure_function_canvas(id));
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(
        !app.is_dirty() && !app.can_undo(),
        "opening a function is navigation, not an edit"
    );
    let ctx = egui::Context::default();
    let output = frame(&mut app, &ctx, egui::vec2(1280.0, 800.0), Vec::new());
    let visible = texts(&output);
    for expected in [
        "Format amount",
        "Inputs",
        "Amount · Float",
        "Output",
        "Text · String",
    ] {
        assert!(
            visible.iter().any(|(text, _)| text == expected),
            "signature contains {expected}"
        );
    }
    assert!(
        !visible
            .iter()
            .any(|(text, _)| matches!(text.as_str(), "Target" | "Scopes" | "Rules" | "Bindings"))
    );
}

#[test]
fn visiting_target_after_unsaved_mapping_edit_does_not_poison_undo_save_point() {
    let mut app = fixture();
    app.project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(7),
        },
    );
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.open_target_tab(0);
    app.ensure_target_canvas(0);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.undo_project();
    assert!(!app.project.graph.nodes.contains_key(&2));
    assert!(
        !app.is_dirty(),
        "visiting a target must not change an earlier save point"
    );
    assert!(!app.observed_dirty());
    app.redo_project();
    assert!(app.project.graph.nodes.contains_key(&2));
    assert!(app.is_dirty());
}

#[test]
fn default_target_views_and_custom_positions_roundtrip_without_navigation_edits() {
    let mut app = fixture();
    app.open_target_tab(0);
    app.ensure_target_canvas(0);
    let capture = |app: &FerruleApp| {
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
    };
    let default = capture(&app);
    assert!(
        default.target_nodes[&0].is_empty(),
        "untouched defaults need no custom positions"
    );
    let reload = |layout: &CanvasLayout| -> CanvasLayout {
        serde_json::from_str(&serde_json::to_string(layout).unwrap()).unwrap()
    };
    app.mapping_workspace = MappingWorkspace::from_layout(&app.project, Some(&reload(&default)));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));
    app.ensure_target_canvas(0);
    assert!(
        app.mapping_workspace.target_canvases.contains_key(&0),
        "the open target is restored"
    );
    assert_eq!(capture(&app), default);
    let canvas = app.mapping_workspace.target_canvases.get_mut(&0).unwrap();
    let id = canvas.snarl.node_ids().next().unwrap().0;
    canvas.snarl.get_node_info_mut(id).unwrap().pos.x += 90.0;
    let custom = capture(&app);
    assert!(!custom.target_nodes[&0].is_empty());
    app.mapping_workspace = MappingWorkspace::from_layout(&app.project, Some(&reload(&custom)));
    assert_eq!(
        capture(&app),
        custom,
        "loaded custom positions are always retained"
    );
}

#[test]
fn scope_tree_resize_stays_inside_a_narrow_inspector() {
    for width in [260.0, 360.0] {
        let mut app = fixture();
        let ctx = egui::Context::default();
        crate::icons::install(&ctx);
        for _ in 0..5 {
            ctx.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let edge = ui.available_rect_before_wrap().right();
                    app.show_inspector(ui, true);
                    assert!(
                        ui.min_rect().right() <= edge + 1.0,
                        "the resize grip and content must fit a {width}px Inspector"
                    );
                },
            );
        }
    }
}
