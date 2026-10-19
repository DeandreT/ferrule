use crate::test_ui::ContextExt as _;

use super::*;
use std::collections::BTreeMap;

mod offscreen;

fn frame(app: &mut FerruleApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            time: Some(ctx.cumulative_frame_nr() as f64 / 10.0),
            events,
            ..Default::default()
        },
        |ui| app.show_main_canvas(ui, true),
    )
}

fn settle(app: &mut FerruleApp, ctx: &egui::Context) -> egui::FullOutput {
    let mut output = frame(app, ctx, Vec::new());
    for _ in 0..8 {
        output = frame(app, ctx, Vec::new());
    }
    output
}

fn button(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing {label}"))
}

fn click(app: &mut FerruleApp, ctx: &egui::Context, pos: egui::Pos2) -> egui::FullOutput {
    frame(app, ctx, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    settle(app, ctx)
}

fn positions(app: &FerruleApp) -> BTreeMap<CanvasNode, egui::Pos2> {
    app.main_canvas
        .snarl
        .nodes_pos()
        .map(|(pos, node)| (*node, pos))
        .collect()
}

#[test]
fn authored_canvas_densities_preview_cancel_apply_and_one_step_undo_preserve_mapping() {
    for density in [
        crate::appearance::CanvasDensity::Standard,
        crate::appearance::CanvasDensity::Compact,
        crate::appearance::CanvasDensity::Dense,
    ] {
        let project = serde_json::from_str(include_str!(
            "../../../../examples/canvas-density/project.json"
        ))
        .unwrap();
        let mut app = FerruleApp {
            project,
            ..Default::default()
        };
        assert!(
            cli::validate(&app.project).is_empty(),
            "{:?}",
            cli::validate(&app.project)
        );
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.appearance.set_density(density);
        let original = serde_json::to_value(&app.project).unwrap();
        let nodes = app
            .main_canvas
            .snarl
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<BTreeMap<_, _>>();
        let ctx = egui::Context::default();
        crate::icons::install(&ctx);
        let mut output = settle(&mut app, &ctx);
        let before = positions(&app);
        let wires = app
            .main_canvas
            .snarl
            .wires()
            .collect::<std::collections::BTreeSet<_>>();
        let (_, before_rects) = crate::canvas_keyboard::current_fit_rects(
            &ctx,
            egui::Id::unique("main_mapping_canvas"),
            app.main_canvas.view_generation,
            &app.main_canvas.snarl,
        )
        .unwrap();
        output = click(&mut app, &ctx, button(&output, "Auto Compact · Preview"));
        assert_eq!(positions(&app), before);
        output = click(&mut app, &ctx, button(&output, "Cancel"));
        assert_eq!(positions(&app), before);
        output = click(&mut app, &ctx, button(&output, "Auto Compact · Preview"));
        output = click(&mut app, &ctx, button(&output, "Apply"));
        let compact = positions(&app);
        assert_ne!(compact, before);
        let (_, rects) = crate::canvas_keyboard::current_fit_rects(
            &ctx,
            egui::Id::unique("main_mapping_canvas"),
            app.main_canvas.view_generation,
            &app.main_canvas.snarl,
        )
        .expect("measured canvas");
        assert!(
            (rects[0].1.width() - before_rects[0].1.width()).abs() < 0.1,
            "Auto Compact must preserve camera zoom"
        );
        for (index, (id, rect)) in rects.iter().enumerate() {
            for (other, other_rect) in &rects[index + 1..] {
                assert!(
                    !rect.intersects(*other_rect),
                    "{density:?}: {id:?} overlaps {other:?}"
                );
            }
        }
        output = click(&mut app, &ctx, button(&output, "Undo layout"));
        assert_eq!(positions(&app), before);
        click(&mut app, &ctx, button(&output, "Redo layout"));
        assert_eq!(positions(&app), compact);
        assert_eq!(serde_json::to_value(&app.project).unwrap(), original);
        assert_eq!(
            app.main_canvas
                .snarl
                .wires()
                .collect::<std::collections::BTreeSet<_>>(),
            wires
        );
        assert_eq!(
            app.main_canvas
                .snarl
                .node_ids()
                .map(|(id, node)| (id, *node))
                .collect::<BTreeMap<_, _>>(),
            nodes
        );
    }
}
