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
