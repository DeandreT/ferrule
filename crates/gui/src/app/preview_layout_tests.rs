use super::*;

const INPUT: &str = "{}\n";
const EXPECTED: &str = "{\n  \"Value\": \"complete preview result\"\n}\n";

fn preview_app() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Input", Vec::new());
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("complete preview result".into()),
        },
    );
    app.project.root.bindings.push(Binding {
        target_field: "Value".into(),
        node: 0,
    });
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(),
        output_identity: "preview.json".into(),
        input_text: INPUT.into(),
        debug_breakpoint: None,
    });
    app
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    viewport: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    app.theme.apply(context);
    context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, viewport)),
            time: Some(context.cumulative_frame_nr() as f64 / 60.0),
            events,
            ..Default::default()
        },
        |ui| app.show_preview_setup(ui.ctx()),
    )
}

fn visible_text(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    fn find(shape: &egui::Shape, clip: egui::Rect, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.visual_bounding_rect();
                clip.contains_rect(rect).then_some(rect)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, clip, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, shape.clip_rect, label))
}

fn action_after_scrolling(
    app: &mut FerruleApp,
    context: &egui::Context,
    viewport: egui::Vec2,
    action: &str,
) -> egui::Rect {
    crate::icons::install(context);
    for _ in 0..4 {
        frame(app, context, viewport, Vec::new());
    }
    let window = context
        .memory(|memory| {
            memory.area_rect(egui::Id::unique(("ferrule_dialog", "preview_mapping")))
        })
        .expect("actual Preview window");
    let view = egui::Rect::from_min_size(egui::Pos2::ZERO, viewport);
    assert!(window.is_finite() && view.contains_rect(window));
    let initial = frame(app, context, viewport, Vec::new());
    assert!(visible_text(&initial, "Preview").is_some());
    assert!(visible_text(&initial, "Debug preview").is_some());
    // Stay over the outer scrollbar rather than the nested input editor.
    let pointer = egui::pos2(window.right() - 12.0, window.center().y);
    for index in 0..40 {
        let mut events = vec![egui::Event::PointerMoved(pointer)];
        if index == 0 {
            events.push(egui::Event::MouseWheel {
                source: egui::MouseWheelSource::Unknown,
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -2_000.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let output = frame(app, context, viewport, events);
        if let Some(rect) = visible_text(&output, action) {
            assert!(view.contains_rect(rect));
            assert!(visible_text(&output, "Preview").is_some());
            assert!(visible_text(&output, "Debug preview").is_some());
            assert_eq!(app.preview_draft.as_ref().unwrap().input_text, INPUT);
            return rect;
        }
    }
    panic!("{action} must be reachable by scrolling the actual Preview setup");
}

fn click(app: &mut FerruleApp, context: &egui::Context, viewport: egui::Vec2, rect: egui::Rect) {
    for pressed in [true, false] {
        frame(
            app,
            context,
            viewport,
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn short_preview_setup_keeps_execute_visible_with_complete_input_and_output() {
    for viewport in [egui::vec2(1280.0, 699.0), egui::vec2(960.0, 560.0)] {
        let mut app = preview_app();
        let context = egui::Context::default();
        let button = action_after_scrolling(&mut app, &context, viewport, "Preview");
        click(&mut app, &context, viewport, button);
        wait_for_preview_completion(&mut app);
        assert!(app.diagnostics.is_empty());
        let actual = app
            .run_report
            .as_mut()
            .expect("clicked Preview executes")
            .report
            .outputs[0]
            .preview();
        assert_eq!(
            actual,
            &crate::run_report::OutputPreview::Text {
                content: EXPECTED.into(),
                total_bytes: EXPECTED.len() as u64,
                truncated: false,
            }
        );
    }
}

#[test]
fn short_preview_setup_keeps_cancel_visible_without_mutating_the_project() {
    for viewport in [egui::vec2(1280.0, 699.0), egui::vec2(960.0, 560.0)] {
        let mut app = preview_app();
        let before = serde_json::to_value(&app.project).unwrap();
        let context = egui::Context::default();
        let button = action_after_scrolling(&mut app, &context, viewport, "Cancel");
        click(&mut app, &context, viewport, button);
        assert!(app.preview_draft.is_none());
        assert!(app.pending_preview.is_none());
        assert!(app.run_report.is_none());
        assert_eq!(serde_json::to_value(&app.project).unwrap(), before);
    }
}
