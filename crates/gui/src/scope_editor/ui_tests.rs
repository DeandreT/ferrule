use super::*;
use crate::test_ui::ContextExt as _;

fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.text().into(),
            text.visual_bounding_rect().center(),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}

#[test]
fn selecting_scope_does_not_toggle_disclosure_and_leaves_have_no_disclosure_state() {
    let root = Scope {
        children: vec![Scope {
            target_field: "Orders".into(),
            children: vec![Scope {
                target_field: "Lines".into(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let mut selected = Vec::new();
    {
        let mut frame = |events| {
            let output = ctx.run_test_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(next) = show_scope_tree(ui, &root, &selected, false) {
                        selected = next;
                    }
                },
            );
            let mut out = Vec::new();
            for shape in output.shapes {
                labels(&shape.shape, &mut out);
            }
            out
        };
        let out = frame(Vec::new());
        let point = out.iter().find(|(label, _)| label == "Orders").unwrap().1;
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(frame(Vec::new()).iter().any(|(label, _)| label == "Lines"));
    }
    assert_eq!(selected, vec![0]);
}

#[test]
fn picker_uses_canvas_names_and_weak_identity_suffix() {
    let ctx = egui::Context::default();
    let node = mapping::Node::Call {
        function: "pad_string_left".into(),
        args: vec![],
    };
    let mut text = None;
    ctx.run_test_ui(egui::RawInput::default(), |ui| {
        text = Some(picker_label(ui, &node, 12));
    });
    let egui::WidgetText::LayoutJob(job) = text.unwrap() else {
        panic!("styled picker text")
    };
    assert_eq!(job.text, "Pad Left · #12");
    assert_ne!(job.sections[0].format.color, job.sections[1].format.color);
}

#[test]
fn deeply_nested_scopes_keep_selection_and_disclosure_inside_the_panel() {
    let mut root = Scope {
        target_field: "Level 20".into(),
        ..Default::default()
    };
    for level in (0..20).rev() {
        root = Scope {
            target_field: format!("Level {level}"),
            children: vec![root],
            ..Default::default()
        };
    }
    for width in [260.0, 360.0] {
        let ctx = egui::Context::default();
        let mut selected = Vec::new();
        let mut frame = |events| {
            ctx.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1000.0),
                    )),
                    time: Some(ctx.cumulative_frame_nr() as f64 / 10.0),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(next) = show_scope_tree(ui, &root, &selected, false) {
                        selected = next;
                    }
                },
            )
        };
        fn bounds(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| bounds(shape, label)),
                _ => None,
            }
        }
        let find = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| bounds(&shape.shape, label))
        };
        let output = frame(Vec::new());
        let leaf = find(&output, "Level 20").expect("deep leaf is visible");
        assert!(
            leaf.right() < width && leaf.left() > 0.0,
            "deep row fits the Inspector"
        );
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(leaf.center()),
                egui::Event::PointerButton {
                    pos: leaf.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = frame(Vec::new());
        let parent = find(&output, "Level 19").unwrap();
        let arrow = egui::pos2(parent.left() - 24.0, parent.center().y);
        assert!(arrow.x > 0.0 && arrow.x < width);
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(arrow),
                egui::Event::PointerButton {
                    pos: arrow,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        for _ in 0..8 {
            frame(Vec::new());
        }
        let output = frame(Vec::new());
        assert!(
            find(&output, "Level 20").is_none(),
            "deep disclosure can collapse its child"
        );
        assert_eq!(
            selected,
            vec![0; 20],
            "disclosure does not change selection"
        );
    }
}
