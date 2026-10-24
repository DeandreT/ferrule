use super::*;
use crate::test_ui::ContextExt as _;

fn temporary_directory() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("ferrule-shell-{}-{index}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    crate::theme::ThemeState::default().apply(&context);
    context
}

fn command_frame(app: &mut FerruleApp, context: &egui::Context, events: Vec<egui::Event>) {
    let _ = context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let enabled = app.ui_project_editing_enabled();
            app.handle_history_shortcuts(ui.ctx(), enabled);
            let [undo, redo, _] = history_shortcuts();
            app.show_command_bar(ui, enabled, LayoutClass::Wide, &undo, &redo);
            app.show_mfd_import_dialog(ui.ctx());
            app.show_unsaved_confirmation(ui.ctx());
        },
    );
}

#[test]
fn shell_modals_block_global_shortcuts_without_editing_saving_or_starting_workflows() {
    let directory = temporary_directory();
    let path = directory.join("project.json");
    for modal in 0..3 {
        for (key, shift) in [
            (egui::Key::Z, false),
            (egui::Key::Z, true),
            (egui::Key::Y, false),
            (egui::Key::R, false),
            (egui::Key::N, false),
            (egui::Key::O, false),
            (egui::Key::S, false),
            (egui::Key::V, true),
        ] {
            let mut app = FerruleApp::default();
            app.save_document_to(&path).unwrap();
            app.project.source.name = "First edit".into();
            app.observe_editor_history(std::time::Instant::now(), false);
            app.project.source.name = "Second edit".into();
            app.observe_editor_history(std::time::Instant::now(), false);
            app.undo_project();
            assert!(app.can_undo() && app.history.can_redo());
            let project = crate::project_state::project_snapshot_key(&app.project);
            let saved = std::fs::read(&path).unwrap();
            match modal {
                0 => app.mfd_import_dialog = Some(MfdImportTarget::Mapping),
                1 => app.mfd_import_dialog = Some(MfdImportTarget::Pipeline),
                _ => app.pending_destructive_action = Some(DestructiveAction::NewProject),
            }
            app.status = "Modal open".into();
            let context = context();
            command_frame(&mut app, &context, Vec::new());
            let modifiers = egui::Modifiers {
                ctrl: true,
                command: true,
                shift,
                ..Default::default()
            };
            command_frame(
                &mut app,
                &context,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
            );
            assert!(!app.ui_project_editing_enabled(), "modal {modal}, {key:?}");
            assert_eq!(
                crate::project_state::project_snapshot_key(&app.project),
                project
            );
            assert_eq!(std::fs::read(&path).unwrap(), saved);
            assert_eq!(app.status, "Modal open");
            assert!(app.can_undo() && app.history.can_redo());
            assert!(app.pending_dialog.is_none());
            assert!(app.pending_file_run.is_none());
            assert!(app.new_mapping_setup.is_none());
            app.mfd_import_dialog = None;
            app.pending_destructive_action = None;
            assert!(app.ui_project_editing_enabled());
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

fn start_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::Panel::top("test_toolbar")
                .exact_size(80.0)
                .show(ui, |_| {});
            egui::Panel::bottom("test_status")
                .exact_size(26.0)
                .show(ui, |_| {});
            egui::CentralPanel::default().show(ui, |ui| app.show_start_view(ui));
        },
    )
}

fn visible_label(shape: &egui::epaint::Shape, label: &str, clip: egui::Rect) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            let center = text.visual_bounding_rect().center();
            clip.contains(center).then_some(center)
        }
        egui::epaint::Shape::Vec(shapes) => shapes
            .iter()
            .find_map(|shape| visible_label(shape, label, clip)),
        _ => None,
    }
}

#[test]
fn all_six_recent_projects_can_be_scrolled_to_and_opened_at_1280_by_600() {
    let directory = temporary_directory();
    let paths: Vec<_> = (0..6)
        .map(|index| directory.join(format!("recent-{index}.json")))
        .collect();
    let project =
        mapping::project_file::encode_pretty(&crate::new_mapping::blank_project()).unwrap();
    for path in &paths {
        std::fs::write(path, &project).unwrap();
    }
    for path in &paths {
        let mut app = FerruleApp {
            recent_projects: paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            ..Default::default()
        };
        let context = context();
        let label = crate::ui_kit::file_label(path);
        let mut position = None;
        for scroll in 0..8 {
            let events = if scroll == 0 {
                Vec::new()
            } else {
                vec![
                    egui::Event::PointerMoved(egui::pos2(640.0, 400.0)),
                    egui::Event::MouseWheel {
                        source: egui::MouseWheelSource::Unknown,
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -180.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            let mut output = start_frame(&mut app, &context, events);
            for _ in 0..3 {
                output = start_frame(&mut app, &context, Vec::new());
            }
            position = output
                .shapes
                .iter()
                .find_map(|shape| visible_label(&shape.shape, &label, shape.clip_rect));
            if position.is_some() {
                break;
            }
        }
        let pos = position.unwrap_or_else(|| panic!("{label} is unreachable at 1280x600"));
        for pressed in [true, false] {
            let _ = start_frame(
                &mut app,
                &context,
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
        assert_eq!(app.document.saved_path(), Some(path.as_path()), "{label}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}
