use super::*;

const ACTIONS: [&str; 4] = [
    "Run pipeline",
    "Debug pipeline",
    "Preview pipeline",
    "Debug preview",
];

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
        |ui| app.show_pipeline_run_setup(ui.ctx()),
    )
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn collect(
        shape: &egui::Shape,
        clip: egui::Rect,
        texts: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::Shape::Text(text) => {
                texts.push((
                    text.galley.text().to_owned(),
                    text.visual_bounding_rect(),
                    clip,
                ));
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, texts);
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut texts);
    }
    texts
}

fn reports_id_clash(texts: &[(String, egui::Rect, egui::Rect)]) -> Option<&str> {
    texts
        .iter()
        .map(|(text, ..)| text.as_str())
        .find(|text| text.contains("use of ") && text.contains(" ID "))
}

#[test]
fn id_clash_check_sees_unsalted_scroll_areas() {
    let context = egui::Context::default();
    context.options_mut(|options| options.warn_on_id_clash = true);
    let output = context.run_test_ui(Default::default(), |ui| {
        for _ in 0..2 {
            egui::ScrollArea::vertical()
                .max_height(40.0)
                .show(ui, |ui| ui.label("rows"));
        }
    });
    assert!(reports_id_clash(&texts(&output)).is_some());
}

#[test]
fn pipeline_window_keeps_every_action_visible_without_id_clashes() -> anyhow::Result<()> {
    for viewport in [
        egui::vec2(1280.0, 800.0),
        egui::vec2(1600.0, 1000.0),
        egui::vec2(960.0, 560.0),
    ] {
        let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-window-actions")?;
        let directory = pipeline_path.parent().unwrap().to_path_buf();
        let context = egui::Context::default();
        context.options_mut(|options| options.warn_on_id_clash = true);
        crate::icons::install(&context);
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, viewport);
        let mut output = frame(&mut app, &context, viewport, Vec::new());
        for _ in 0..4 {
            output = frame(&mut app, &context, viewport, Vec::new());
        }
        let window = context
            .memory(|memory| memory.area_rect(egui::Id::unique(Some("Run or preview pipeline"))))
            .expect("pipeline run window is shown");
        assert!(
            view.contains_rect(window),
            "{viewport:?}: window {window:?} leaves the viewport"
        );
        let texts = texts(&output);
        if let Some(clash) = reports_id_clash(&texts) {
            panic!("{viewport:?}: egui reported an ID clash: {clash}");
        }
        let mut run = None;
        for action in ACTIONS {
            let (_, rect, clip) = texts
                .iter()
                .find(|(text, ..)| text == action)
                .unwrap_or_else(|| panic!("{viewport:?}: {action} is not drawn"));
            assert!(
                clip.contains_rect(*rect) && view.contains_rect(*rect),
                "{viewport:?}: {action} at {rect:?} is outside {clip:?}"
            );
            if action == "Run pipeline" {
                run = Some(*rect);
            }
        }

        // The pinned action is the real one: clicking it starts the run.
        let run = run.expect("Run pipeline is drawn");
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                viewport,
                vec![
                    egui::Event::PointerMoved(run.center()),
                    egui::Event::PointerButton {
                        pos: run.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(
            app.pending_pipeline_run.is_some(),
            "{viewport:?}: run starts"
        );
        wait_for_pipeline_completion(&mut app);
        assert_eq!(selected_run_value(&directory.join("finish.json"))?, "B");
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}
