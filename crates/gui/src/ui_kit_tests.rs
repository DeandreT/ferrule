use super::*;
use crate::test_ui::ContextExt as _;

#[test]
fn plural_counts_read_as_english() {
    assert_eq!(plural(0, "record"), "0 records");
    assert_eq!(plural(1, "record"), "1 record");
    assert_eq!(plural(2, "issue"), "2 issues");
    assert_eq!(plural(3, "match"), "3 matches");
    assert_eq!(plural(2, "box"), "2 boxes");
    assert_eq!(plural(4, "entry"), "4 entries");
    assert_eq!(plural(2, "key"), "2 keys");
    assert_eq!(plural(1, "entry"), "1 entry");
}

#[test]
fn sentence_capitalizes_only_a_leading_lowercase_letter() {
    assert_eq!(sentence("canvas fitted"), "Canvas fitted");
    assert_eq!(sentence("Already capitalized"), "Already capitalized");
    assert_eq!(sentence("3 issues"), "3 issues");
    assert_eq!(sentence(""), "");
    assert!(matches!(sentence("Saved"), Cow::Borrowed(_)));
}

fn run(context: &egui::Context, add: impl FnMut(&mut Ui)) {
    let _ = context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            ..Default::default()
        },
        add,
    );
}

#[test]
fn table_grid_fields_keep_their_requested_width() {
    let context = egui::Context::default();
    crate::theme::ThemeState::default().apply(&context);
    let mut name = String::new();
    let mut value = String::new();
    let mut widths = (0.0, 0.0);
    // The first frame sizes the columns; egui reuses that size afterwards.
    for _ in 0..2 {
        run(&context, |ui| {
            table_grid(ui, "fields", 3, |ui| {
                let name_rect = ui.add(text_field(&mut name, FieldWidth::Medium)).rect;
                let value_rect = ui.add(text_field(&mut value, FieldWidth::Small)).rect;
                let _ = ui.button("Remove");
                ui.end_row();
                widths = (name_rect.width(), value_rect.width());
            });
        });
    }
    assert!(widths.0 >= 180.0, "name field collapsed to {}", widths.0);
    assert!(widths.1 >= 80.0, "value field collapsed to {}", widths.1);
}

#[test]
fn form_grid_rows_align_labels_and_fill_fields() {
    let context = egui::Context::default();
    crate::theme::ThemeState::default().apply(&context);
    let mut input = String::new();
    let mut output = String::new();
    let mut rects = Vec::new();
    for _ in 0..2 {
        rects.clear();
        run(&context, |ui| {
            form_grid(ui, "paths", |form| {
                rects.push(form.row("Input", |ui| {
                    ui.add(text_field(&mut input, FieldWidth::Fill)).rect
                }));
                rects.push(form.row("Output file", |ui| {
                    ui.add(text_field(&mut output, FieldWidth::Fill)).rect
                }));
            });
        });
    }
    assert_eq!(rects[0].left(), rects[1].left());
    assert!(
        rects[0].width() > 800.0,
        "fill field is {}",
        rects[0].width()
    );
}

#[test]
fn closable_tab_keeps_its_close_button_on_the_label_row() {
    let context = egui::Context::default();
    crate::theme::ThemeState::default().apply(&context);
    let mut plain = Rect::NOTHING;
    let mut closable = Rect::NOTHING;
    for _ in 0..2 {
        run(&context, |ui| {
            plain = tab(ui, "Main mapping", true, false).response.rect;
            closable = tab(ui, "Main mapping", true, true).response.rect;
        });
    }
    assert!(
        closable.height() <= plain.height() + METRICS.control_height / 2.0,
        "close button stacked below the label: plain {plain:?}, closable {closable:?}"
    );
    assert!(closable.width() > plain.width());
}

#[test]
fn modal_dialogs_centre_in_the_viewport_and_fit_inside_its_margins() {
    let viewport = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
    let workspace = Rect::from_min_max(Pos2::new(0.0, 80.0), Pos2::new(1600.0, 974.0));
    let placement = dialog_placement(DialogKind::Modal, DialogWidth::Narrow, viewport, workspace);
    assert_eq!(placement.pivot, Align2::CENTER_CENTER);
    assert_eq!(placement.position, viewport.center());
    assert_eq!(placement.width, DialogWidth::Narrow.px());
    assert_eq!(
        placement.max_height,
        viewport.height() - 2.0 * METRICS.dialog_margin
    );

    let small = Rect::from_min_size(Pos2::ZERO, vec2(500.0, 400.0));
    let placement = dialog_placement(DialogKind::Modal, DialogWidth::Wide, small, small);
    assert_eq!(placement.width, 500.0 - 2.0 * METRICS.dialog_margin);
}

#[test]
fn docked_dialogs_hang_from_the_workspace_top_right_below_the_toolbar() {
    let viewport = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
    let workspace = Rect::from_min_max(Pos2::new(0.0, 80.0), Pos2::new(1600.0, 974.0));
    let placement = dialog_placement(DialogKind::Docked, DialogWidth::Medium, viewport, workspace);
    assert_eq!(placement.pivot, Align2::RIGHT_TOP);
    assert_eq!(
        placement.position,
        Pos2::new(1600.0 - METRICS.dialog_margin, 80.0 + METRICS.dialog_margin)
    );
    assert_eq!(placement.width, DialogWidth::Medium.px());
    assert_eq!(
        placement.max_height,
        workspace.height() - 2.0 * METRICS.dialog_margin
    );
    // The dialog's top edge stays below the toolbar and its bottom above
    // the status bar.
    assert!(placement.position.y >= workspace.top());
    assert!(placement.position.y + placement.max_height <= workspace.bottom());
}

#[test]
fn dialog_body_scrolls_instead_of_growing_past_the_viewport() {
    let context = egui::Context::default();
    crate::theme::ThemeState::default().apply(&context);
    let mut frame = None;
    for _ in 0..3 {
        run(&context, |ui| {
            let response = dialog(ui.ctx(), DialogKind::Modal, "Tall").show(
                |ui| {
                    for row in 0..200 {
                        ui.label(format!("row {row}"));
                    }
                },
                |ui| dialog_buttons(ui, "Save".into(), Some("Cancel".into()), None),
            );
            assert!(!response.close_requested);
        });
        frame =
            context.memory(|memory| memory.area_rect(egui::Id::unique(("ferrule_dialog", "Tall"))));
    }
    let rect = frame.expect("the modal area is laid out");
    assert!(rect.top() >= 0.0 && rect.bottom() <= 800.0, "{rect:?}");
}

/// A button that opens a modal with optional body field and footer state.
#[derive(Default)]
struct ModalHost {
    open: bool,
    body_field: bool,
    primary_disabled: bool,
    name: String,
    action: Option<DialogAction>,
}

fn modal_host(host: ModalHost) -> egui_kittest::Harness<'static, ModalHost> {
    let mut initialized = false;
    let mut harness = egui_kittest::Harness::builder()
        .with_size((800.0, 600.0))
        .build_ui_state(
            move |ui, host: &mut ModalHost| {
                if !initialized {
                    crate::icons::install(ui.ctx());
                    initialized = true;
                }
                if ui.button("Open dialog").clicked() {
                    host.open = true;
                }
                if !host.open {
                    return;
                }
                let response = dialog(ui.ctx(), DialogKind::Modal, "Focus").show(
                    |ui| {
                        ui.label("Body text");
                        if host.body_field {
                            ui.add(text_field(&mut host.name, FieldWidth::Medium))
                                .widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Name")
                                });
                        }
                    },
                    |ui| {
                        dialog_buttons(
                            ui,
                            DialogButton::new("Apply")
                                .disabled_if(host.primary_disabled, "Not ready"),
                            Some("Cancel".into()),
                            Some("Remove".into()),
                        )
                    },
                );
                if response.footer.is_some() {
                    host.action = response.footer;
                }
                if response.close_requested || response.footer.is_some() {
                    host.open = false;
                }
            },
            host,
        );
    harness.run_steps(2);
    harness
}

fn press(harness: &mut egui_kittest::Harness<'_, ModalHost>, key: egui::Key) {
    harness.key_press(key);
    harness.run_steps(3);
}

fn focused(harness: &egui_kittest::Harness<'_, ModalHost>) -> Option<String> {
    use egui_kittest::kittest::NodeT as _;
    harness
        .root()
        .children_recursive()
        .find(|node| node.is_focused())
        .and_then(|node| node.accesskit_node().label())
}

fn open_from_button(host: ModalHost) -> egui_kittest::Harness<'static, ModalHost> {
    let mut harness = modal_host(host);
    press(&mut harness, egui::Key::Tab);
    assert_eq!(focused(&harness).as_deref(), Some("Open dialog"));
    press(&mut harness, egui::Key::Enter);
    assert!(harness.state().open);
    // The Enter that opened the dialog does not also activate its default.
    assert_eq!(harness.state().action, None);
    harness
}

#[test]
fn modal_dialog_focuses_its_default_action_and_tabs_in_visual_order() {
    let mut harness = open_from_button(ModalHost::default());
    assert_eq!(focused(&harness).as_deref(), Some("Apply"));
    let mut order = Vec::new();
    for _ in 0..4 {
        press(&mut harness, egui::Key::Tab);
        order.extend(focused(&harness));
    }
    assert_eq!(order, ["Close", "Remove", "Cancel", "Apply"]);
}

#[test]
fn modal_dialog_focuses_its_first_body_control() {
    let mut harness = open_from_button(ModalHost {
        body_field: true,
        ..ModalHost::default()
    });
    assert_eq!(focused(&harness).as_deref(), Some("Name"));
    harness.event(egui::Event::Text("Ada".into()));
    harness.run_steps(2);
    assert_eq!(harness.state().name, "Ada");
}

#[test]
fn modal_dialog_focuses_cancel_while_its_default_action_is_disabled() {
    let harness = open_from_button(ModalHost {
        primary_disabled: true,
        ..ModalHost::default()
    });
    assert_eq!(focused(&harness).as_deref(), Some("Cancel"));
}

#[test]
fn modal_dialog_closes_on_escape_and_returns_focus_to_its_opener() {
    let mut harness = open_from_button(ModalHost {
        body_field: true,
        ..ModalHost::default()
    });
    press(&mut harness, egui::Key::Escape);
    assert!(!harness.state().open);
    assert_eq!(harness.state().action, None);
    assert_eq!(focused(&harness).as_deref(), Some("Open dialog"));

    // Reopening focuses the dialog again; closing from the footer also restores.
    press(&mut harness, egui::Key::Enter);
    assert_eq!(focused(&harness).as_deref(), Some("Name"));
    for _ in 0..2 {
        press(&mut harness, egui::Key::Tab);
    }
    assert_eq!(focused(&harness).as_deref(), Some("Cancel"));
    press(&mut harness, egui::Key::Enter);
    assert_eq!(harness.state().action, Some(DialogAction::Secondary));
    assert_eq!(focused(&harness).as_deref(), Some("Open dialog"));
}
