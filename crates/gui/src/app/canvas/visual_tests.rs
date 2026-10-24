//! Opt-in, authored before/after evidence for the canvas workbench.
use super::super::*;
use egui_kittest::{Harness, kittest::Queryable as _};
use std::path::PathBuf;

fn harness(size: (f32, f32), complex: bool, dialog: bool) -> Harness<'static, FerruleApp> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let path = if complex {
        "crates/cli/tests/code_generation/static_document_adapters/fixtures/projects/base.json"
    } else {
        "examples/canvas-density/project.json"
    };
    let mut app = FerruleApp {
        project: serde_json::from_slice(&std::fs::read(root.join(path)).unwrap()).unwrap(),
        ..Default::default()
    };
    if !complex {
        app.project.graph.nodes.insert(
            20,
            Node::Position {
                collection: Vec::new(),
            },
        );
        app.project.graph.nodes.insert(
            21,
            Node::Call {
                function: "concat".into(),
                args: vec![3, 4, 7, 3, 4],
            },
        );
        for id in 30..42 {
            app.project.graph.nodes.insert(
                id,
                Node::Const {
                    value: ir::Value::Int(id as i64),
                },
            );
        }
    }
    app.theme.preference = crate::theme::ThemePreference::Dark;
    app.main_canvas = CanvasDocumentState::main(&app.project);
    let mut initialized = false;
    Harness::builder()
        .with_size(size)
        .with_max_steps(40)
        .with_accessibility_check(std::env::var_os("FERRULE_BASELINE_SHOTS").is_none())
        .with_context_setup(|ctx| {
            ctx.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            move |ui, app: &mut FerruleApp| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.visuals.text_cursor.blink = false;
                        style.animation_time = 0.0;
                    });
                    initialized = true;
                }
                app.palette = app.theme.apply(ui.ctx());
                egui::Panel::top("evidence_label").show(ui, |ui| {
                    ui.strong(format!(
                        "Canvas workbench · {} × {}",
                        size.0 as u32, size.1 as u32
                    ));
                });
                egui::CentralPanel::default().show(ui, |ui| app.show_main_canvas(ui, true));
                if dialog {
                    egui::Window::new("Canvas dialog stacking")
                        .fixed_pos(egui::pos2(size.0 - 400.0, size.1 - 260.0))
                        .fixed_size(egui::vec2(350.0, 220.0))
                        .collapsible(false)
                        .show(ui.ctx(), |ui| {
                            ui.label("This dialog must cover the canvas minimap.");
                            ui.add_space(140.0);
                            let _ = ui.button("Close");
                        });
                }
            },
            app,
        )
}

fn directory() -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os("FERRULE_CANVAS_SHOTS").expect("set screenshot directory"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn capture(harness: &mut Harness<'_, FerruleApp>, directory: &std::path::Path, name: &str) {
    harness.run();
    harness
        .render()
        .unwrap()
        .save(directory.join(format!("{name}.png")))
        .unwrap();
}

fn click(harness: &mut Harness<'_, FerruleApp>, label: &str) {
    harness.get_by_label(label).click();
    harness.run();
}

fn secondary(harness: &mut Harness<'_, FerruleApp>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
}

#[test]
#[ignore = "opt-in screenshots: FERRULE_CANVAS_SHOTS"]
fn record_canvas_overviews() {
    let directory = directory();
    for size in [(1600.0, 1000.0), (1280.0, 800.0)] {
        let suffix = format!("{}x{}", size.0 as u32, size.1 as u32);
        capture(
            &mut harness(size, true, false),
            &directory,
            &format!("initial-fit-{suffix}"),
        );
        capture(
            &mut harness(size, false, false),
            &directory,
            &format!("cards-endpoints-{suffix}"),
        );
        capture(
            &mut harness(size, false, true),
            &directory,
            &format!("minimap-dialog-{suffix}"),
        );
    }
}

#[test]
#[ignore = "opt-in screenshots: FERRULE_CANVAS_SHOTS"]
fn record_canvas_editors() {
    let directory = directory();
    for size in [(1600.0, 1000.0), (1280.0, 800.0)] {
        let suffix = format!("{}x{}", size.0 as u32, size.1 as u32);
        let mut harness = harness(size, false, false);
        harness.run();
        let edit = harness.get_by_label_contains("Edit Const: A longer");
        edit.click();
        harness.run();
        capture(
            &mut harness,
            &directory,
            &format!("constant-editor-{suffix}"),
        );
        harness.key_press(egui::Key::Escape);
        harness.run();
        let header = harness.get_by_label_contains("Edit Const: A longer").rect();
        secondary(&mut harness, header.center() - egui::vec2(75.0, 0.0));
        capture(&mut harness, &directory, &format!("node-menu-{suffix}"));
    }
}

#[test]
#[ignore = "opt-in screenshots: FERRULE_CANVAS_SHOTS"]
fn record_canvas_palette_layout() {
    let directory = directory();
    for size in [(1600.0, 1000.0), (1280.0, 800.0)] {
        let suffix = format!("{}x{}", size.0 as u32, size.1 as u32);
        let mut harness = harness(size, false, false);
        harness.run();
        secondary(&mut harness, egui::pos2(size.0 - 24.0, 100.0));
        harness.get_by_label("Add node");
        capture(&mut harness, &directory, &format!("palette-{suffix}"));
        harness.event(egui::Event::Text("join".into()));
        harness.run();
        capture(
            &mut harness,
            &directory,
            &format!("palette-search-{suffix}"),
        );
        harness.key_press(egui::Key::Escape);
        harness.run();
        if harness.query_by_label("Layout").is_some() {
            click(&mut harness, "Layout");
            click(&mut harness, "Auto-arrange · preview");
        } else {
            click(&mut harness, "Auto Compact · Preview");
        }
        capture(
            &mut harness,
            &directory,
            &format!("layout-preview-{suffix}"),
        );
    }
}
