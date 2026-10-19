//! Accessible-widget interactions and opt-in images without a desktop window.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};
use std::path::Path;

fn click(harness: &mut Harness<'_, FerruleApp>, label: &str) {
    harness.get_by_label(label).click();
    harness.run();
}

fn key(harness: &mut Harness<'_, FerruleApp>, key: egui::Key) {
    harness.key_press(key);
    harness.run();
}

fn capture(harness: &mut Harness<'_, FerruleApp>, directory: Option<&Path>, name: &str) {
    let Some(directory) = directory else {
        return;
    };
    let image = harness.render().expect("offscreen rendering");
    assert!(
        image
            .pixels()
            .map(|pixel| pixel.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 100,
        "rendered frame is blank or lacks detail"
    );
    let file = std::fs::File::create_new(directory.join(format!("{name}.png")))
        .expect("new offscreen PNG");
    image
        .write_to(&mut std::io::BufWriter::new(file), image::ImageFormat::Png)
        .unwrap();
}

fn source_label_clears_badge(harness: &Harness<'_, FerruleApp>) {
    let badge = harness.get_by_label("Choose one of 3 connections").rect();
    let mut found = false;
    for clipped in &harness.output().shapes {
        if let egui::epaint::Shape::Text(text) = &clipped.shape
            && text.galley.text() == "name"
        {
            let bounds = text.visual_bounding_rect();
            assert!(
                clipped.clip_rect.expand(0.5).contains_rect(bounds),
                "source name is clipped"
            );
            assert!(
                !badge.intersects(bounds),
                "fan-out badge covers the source name"
            );
            found = true;
        }
    }
    assert!(
        found,
        "the complete source name must be painted beside its badge"
    );
}

fn scenarios(directory: Option<&Path>) {
    for density in crate::appearance::CanvasDensity::ALL {
        for scale in [1.0, 1.5] {
            let mut app = FerruleApp {
                project: serde_json::from_str(include_str!(
                    "../../../../../examples/canvas-density/project.json"
                ))
                .unwrap(),
                ..Default::default()
            };
            app.theme.preference = crate::theme::ThemePreference::Dark;
            app.appearance.set_density(density);
            app.main_canvas = CanvasDocumentState::main(&app.project);
            let original = serde_json::to_value(&app.project).unwrap();
            let mut initialized = false;
            let mut harness = Harness::builder()
                .with_size((1600.0, 1000.0))
                .with_pixels_per_point(scale)
                .with_os(egui::os::OperatingSystem::Windows)
                .with_max_steps(30)
                .with_context_setup(|ctx| {
                    // Match native eframe's font fallback, including CJK literals.
                    ctx.add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                })
                .build_ui_state(
                    move |ui, app: &mut FerruleApp| {
                        if !initialized {
                            crate::fonts::install(ui.ctx());
                            crate::icons::install(ui.ctx());
                            app.theme.apply(ui.ctx());
                            ui.ctx().all_styles_mut(|style| {
                                style.visuals.text_cursor.blink = false;
                                style.scroll_animation = egui::style::ScrollAnimation::none();
                                style.animation_time = 0.0;
                            });
                            initialized = true;
                        }
                        app.show_main_canvas(ui, true);
                    },
                    app,
                );
            harness.run();
            assert_eq!(harness.output().pixels_per_point, scale);
            source_label_clears_badge(&harness);
            let prefix = format!("{density:?}-{}", (scale * 100.0) as u32);
            let image = |harness: &mut Harness<'_, FerruleApp>, name: &str| {
                capture(harness, directory, &format!("{prefix}-{name}"));
            };
            image(&mut harness, "initial");
            let before = positions(harness.state());
            let wires = harness
                .state()
                .main_canvas
                .snarl
                .wires()
                .collect::<std::collections::BTreeSet<_>>();

            click(&mut harness, "Choose one of 3 connections");
            assert!(
                crate::canvas_tools::picker_open(&harness.ctx),
                "{prefix}: badge did not open picker"
            );
            harness.get_by_label("Choose connection");
            image(&mut harness, "picker");
            key(&mut harness, egui::Key::ArrowDown);
            harness.get_by_label("Choose connection");
            key(&mut harness, egui::Key::Enter);
            harness.get_by_label("Clear focus");
            image(&mut harness, "exact-focus");
            click(&mut harness, "Both");
            harness.get_by_label(
                "Static wiring only; all connected branches, not an execution trace.",
            );
            image(&mut harness, "trace");
            key(&mut harness, egui::Key::Escape);
            assert!(harness.query_by_label("Clear focus").is_none());

            click(&mut harness, "Auto Compact · Preview");
            assert_eq!(positions(harness.state()), before);
            image(&mut harness, "preview");
            click(&mut harness, "Cancel");
            assert_eq!(positions(harness.state()), before);
            click(&mut harness, "Auto Compact · Preview");
            click(&mut harness, "Apply");
            let compact = positions(harness.state());
            assert_ne!(compact, before);
            source_label_clears_badge(&harness);
            image(&mut harness, "applied");
            click(&mut harness, "Undo layout");
            assert_eq!(positions(harness.state()), before);
            click(&mut harness, "Redo layout");
            assert_eq!(positions(harness.state()), compact);
            assert_eq!(
                harness
                    .state()
                    .main_canvas
                    .snarl
                    .wires()
                    .collect::<std::collections::BTreeSet<_>>(),
                wires
            );

            // Both endpoints have this control; choose the source endpoint by
            // its measured bounds instead of depending on accessibility order.
            let source_id = harness
                .state()
                .main_canvas
                .snarl
                .node_ids()
                .find_map(|(id, node)| (*node == CanvasNode::SourceBlock(0)).then_some(id))
                .unwrap();
            let (_, rects) = crate::canvas_keyboard::current_fit_rects(
                &harness.ctx,
                egui::Id::unique("main_mapping_canvas"),
                harness.state().main_canvas.view_generation,
                &harness.state().main_canvas.snarl,
            )
            .unwrap();
            let source_rect = rects.iter().find(|(id, _)| *id == source_id).unwrap().1;
            assert!(source_rect.is_finite());
            harness
                .get_all_by_label("Connected fields")
                .find(|control| source_rect.contains(control.rect().center()))
                .expect("source endpoint filter inside its measured bounds")
                .click();
            harness.run();
            source_label_clears_badge(&harness);
            image(&mut harness, "connected");
            assert_eq!(positions(harness.state()), compact);
            assert_eq!(
                harness
                    .state()
                    .main_canvas
                    .endpoint_scroll
                    .visible_indices(CanvasNode::SourceBlock(0), 18),
                [0, 17]
            );
            assert_eq!(
                serde_json::to_value(&harness.state().project).unwrap(),
                original
            );
        }
    }
}

#[test]
fn canvas_kittest_interactions_preserve_mapping() {
    scenarios(None);
}

#[test]
fn connection_picker_keyboard_keeps_the_exact_row_visible() {
    let mut app = FerruleApp {
        project: blank_project(),
        ..Default::default()
    };
    app.project.source = ir::SchemaNode::group(
        "Source",
        vec![ir::SchemaNode::scalar("value", ir::ScalarType::String)],
    );
    app.project.target = ir::SchemaNode::group(
        "Target",
        (0..60)
            .map(|index| ir::SchemaNode::scalar(format!("t{index:02}"), ir::ScalarType::String))
            .collect(),
    );
    app.project.graph.nodes.insert(
        0,
        mapping::Node::SourceField {
            path: vec!["value".into()],
            frame: None,
        },
    );
    app.project.root.bindings = (0..60)
        .map(|index| mapping::Binding {
            target_field: format!("t{index:02}"),
            node: 0,
        })
        .collect();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    let original = serde_json::to_value(&app.project).unwrap();
    let mut initialized = false;
    let mut harness = Harness::builder()
        .with_size((1600.0, 1000.0))
        .with_max_steps(30)
        .build_ui_state(
            move |ui, app: &mut FerruleApp| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.scroll_animation = egui::style::ScrollAnimation::none();
                        style.animation_time = 0.0;
                    });
                    initialized = true;
                }
                app.show_main_canvas(ui, true);
            },
            app,
        );
    harness.run();
    let before = positions(harness.state());
    // Revealing an endpoint changes its visible/proxy pins, not the connections.
    let canonical_wires = |app: &FerruleApp| {
        semantic_canvas_wires(
            &app.project.graph,
            &app.project.root,
            &crate::canvas::source_blocks(&app.project.source),
            &crate::canvas::target_blocks(&app.project.target),
            &app.main_canvas.snarl,
            SourcePinPolicy {
                owned_items: &Default::default(),
                unavailable_blocks: &Default::default(),
            },
        )
    };
    let wires = canonical_wires(harness.state());
    assert_eq!(wires.len(), 60);
    let visible = |harness: &Harness<'_, FerruleApp>, field: &str| {
        let mut found = false;
        for clipped in &harness.output().shapes {
            if let egui::epaint::Shape::Text(text) = &clipped.shape
                && text.galley.text().contains(field)
                && text.galley.text().contains('→')
            {
                let bounds = text.visual_bounding_rect();
                found |= bounds.top() >= clipped.clip_rect.top() - 0.5
                    && bounds.bottom() <= clipped.clip_rect.bottom() + 0.5;
            }
        }
        assert!(found, "connection row {field} is not fully visible");
    };
    click(&mut harness, "Choose one of 60 connections");
    visible(&harness, "t00 · port 1");
    key(&mut harness, egui::Key::ArrowUp);
    visible(&harness, "t59 · port 60");
    key(&mut harness, egui::Key::ArrowDown);
    visible(&harness, "t00 · port 1");
    for _ in 0..35 {
        key(&mut harness, egui::Key::ArrowDown);
    }
    visible(&harness, "t35 · port 36");
    key(&mut harness, egui::Key::Enter);
    assert!(!crate::canvas_tools::picker_open(&harness.ctx));
    let chosen = Some(crate::canvas_tools::Edge {
        from: crate::canvas::CanvasNode::SourceBlock(0),
        output: 0,
        to: crate::canvas::CanvasNode::TargetBlock(0),
        input: 35,
    });
    assert_eq!(crate::canvas_tools::focused_edge(&harness.ctx), chosen);
    harness.get_by_label_contains("t35 · port 36");
    harness.get_by_label("Clear focus");
    click(&mut harness, "Choose one of 60 connections");
    visible(&harness, "t00 · port 1");
    key(&mut harness, egui::Key::Escape);
    assert!(!crate::canvas_tools::picker_open(&harness.ctx));
    assert_eq!(crate::canvas_tools::focused_edge(&harness.ctx), chosen);
    harness.get_by_label("Clear focus");
    assert_eq!(positions(harness.state()), before);
    assert_eq!(canonical_wires(harness.state()), wires);
    assert_eq!(
        serde_json::to_value(&harness.state().project).unwrap(),
        original
    );
}

#[test]
#[ignore = "opt-in graphics check: set FERRULE_OFFSCREEN_DIR to a new output directory"]
fn canvas_offscreen_release_interactions() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("FERRULE_OFFSCREEN_DIR").expect("explicit offscreen output directory"),
    );
    assert!(directory.is_absolute());
    std::fs::create_dir(&directory).expect("new offscreen output directory");
    eprintln!("Offscreen frames: {}", directory.display());
    scenarios(Some(&directory));
}
