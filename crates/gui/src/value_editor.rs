//! Small widgets for editing `ir::Value`s in place: a `Const` node's literal,
//! and a `ValueMap` node's lookup table.

use egui::Ui;
use ir::Value;

const VALUE_EDIT_WIDTH: f32 = 150.0;
const VALUE_MAP_CELL_WIDTH: f32 = 104.0;
const VALUE_MAP_CONTENT_WIDTH: f32 = 640.0;
const VALUE_MAP_MAX_HEIGHT: f32 = 170.0;
const CONST_TITLE_CHAR_LIMIT: usize = 28;

pub fn display_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::JsonNull(_) => "json:null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.clone(),
        Value::XmlNil(_) => "xsi:nil".to_string(),
    }
}

pub fn title_preview(value: &Value) -> String {
    let display = display_string(value)
        .chars()
        .map(|ch| if matches!(ch, '\r' | '\n') { ' ' } else { ch })
        .collect::<String>();
    if display.chars().count() <= CONST_TITLE_CHAR_LIMIT {
        return display;
    }

    let mut preview = display
        .chars()
        .take(CONST_TITLE_CHAR_LIMIT - 3)
        .collect::<String>();
    preview.push_str("...");
    preview
}

pub fn show_value_editor(ui: &mut Ui, value: &mut Value) {
    egui::ComboBox::from_id_salt(ui.id().with("value_kind"))
        .selected_text(value.type_name())
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(matches!(value, Value::Null), "null")
                .clicked()
            {
                *value = Value::Null;
            }
            if ui
                .selectable_label(value.is_json_null(), "JSON null")
                .clicked()
            {
                *value = Value::json_null();
            }
            if ui
                .selectable_label(matches!(value, Value::Bool(_)), "bool")
                .clicked()
            {
                *value = Value::Bool(false);
            }
            if ui
                .selectable_label(matches!(value, Value::Int(_)), "int")
                .clicked()
            {
                *value = Value::Int(0);
            }
            if ui
                .selectable_label(matches!(value, Value::Float(_)), "float")
                .clicked()
            {
                *value = Value::Float(0.0);
            }
            if ui
                .selectable_label(matches!(value, Value::String(_)), "string")
                .clicked()
            {
                *value = Value::String(String::new());
            }
            if ui.selectable_label(value.is_xml_nil(), "xsi:nil").clicked() {
                *value = Value::xml_nil();
            }
        });
    match value {
        Value::Null | Value::JsonNull(_) => {}
        Value::Bool(b) => {
            ui.checkbox(b, "");
        }
        Value::Int(i) => {
            ui.add(egui::DragValue::new(i));
        }
        Value::Float(f) => {
            ui.add(egui::DragValue::new(f));
        }
        Value::String(s) => {
            ui.add_sized(
                [VALUE_EDIT_WIDTH, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(s),
            )
            .on_hover_text(s.as_str());
        }
        Value::XmlNil(_) => {}
    }
}

/// Edits a `ValueMap`'s complete lookup table. Entries are coerced to strings
/// while editing, matching the original inline editor's behavior.
pub fn show_value_map_editor(
    ui: &mut Ui,
    table: &mut Vec<(Value, Value)>,
    default: &mut Option<Value>,
    wheel_delta_y: Option<f32>,
) {
    ui.vertical(|ui| {
        // Leave room for the solid scrollbar so the complete body stays at
        // the advertised node width when enough rows require scrolling.
        ui.set_min_width(VALUE_MAP_CONTENT_WIDTH);
        ui.set_max_width(VALUE_MAP_CONTENT_WIDTH);
        ui.horizontal(|ui| {
            ui.weak(match table.len() {
                1 => "1 entry".to_string(),
                count => format!("{count} entries"),
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(crate::icons::text(
                        lucide_icons::Icon::Plus,
                        14.0,
                    )))
                    .on_hover_text("Add entry")
                    .clicked()
                {
                    table.push((Value::String(String::new()), Value::String(String::new())));
                }
            });
        });

        let mut remove_idx = None;
        ui.scope(|ui| {
            let mut scroll_style = egui::style::ScrollStyle::solid();
            scroll_style.bar_width = 9.0;
            scroll_style.handle_min_length = 28.0;
            ui.style_mut().spacing.scroll = scroll_style;
            let scroll = egui::ScrollArea::vertical()
                .id_salt("value_map_table_scroll")
                .max_height(VALUE_MAP_MAX_HEIGHT)
                .auto_shrink([false, true])
                .animated(false)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .scroll_source(egui::scroll_area::ScrollSource {
                    scroll_bar: true,
                    drag: egui::scroll_area::DragScroll::Never,
                    mouse_wheel: false,
                })
                .show(ui, |ui| {
                    egui::Grid::new("value_map_table")
                        .num_columns(9)
                        .striped(true)
                        .spacing([4.0, 3.0])
                        .show(ui, |ui| {
                            for (row, entries) in table.chunks_mut(2).enumerate() {
                                show_value_map_entry(ui, row * 2, &mut entries[0], &mut remove_idx);
                                ui.weak("|");
                                if let Some(entry) = entries.get_mut(1) {
                                    show_value_map_entry(ui, row * 2 + 1, entry, &mut remove_idx);
                                } else {
                                    for _ in 0..4 {
                                        ui.label("");
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
            apply_value_map_wheel(ui, &scroll, wheel_delta_y);
        });
        if let Some(i) = remove_idx {
            table.remove(i);
        }

        let mut has_default = default.is_some();
        ui.horizontal(|ui| {
            if ui.checkbox(&mut has_default, "Default").changed() {
                *default = has_default.then(|| Value::String(String::new()));
            }
            if let Some(value) = default {
                edit_map_value(ui, value);
            }
        });
    });
}

fn show_value_map_entry(
    ui: &mut Ui,
    index: usize,
    (from, to): &mut (Value, Value),
    remove_idx: &mut Option<usize>,
) {
    edit_map_value(ui, from);
    ui.weak("->");
    edit_map_value(ui, to);
    if ui
        .add(egui::Button::new(crate::icons::text(
            lucide_icons::Icon::Trash2,
            14.0,
        )))
        .on_hover_text("Remove entry")
        .clicked()
    {
        *remove_idx = Some(index);
    }
}

fn apply_value_map_wheel(
    ui: &mut Ui,
    scroll: &egui::scroll_area::ScrollAreaOutput<()>,
    wheel_delta_y: Option<f32>,
) {
    let Some(delta_y) = wheel_delta_y else {
        return;
    };
    if delta_y == 0.0 {
        return;
    }

    let next_offset = value_map_wheel_offset(
        scroll.state.offset.y,
        delta_y,
        scroll.content_size.y,
        scroll.inner_rect.height(),
    );
    if next_offset == scroll.state.offset.y {
        return;
    }

    let mut state = scroll.state;
    state.offset.y = next_offset;
    state.store(ui.ctx(), scroll.id);
    ui.ctx().request_repaint();
}

fn value_map_wheel_offset(
    current: f32,
    delta_y: f32,
    content_height: f32,
    viewport_height: f32,
) -> f32 {
    let max_offset = (content_height - viewport_height).max(0.0);
    (current - delta_y).clamp(0.0, max_offset)
}

fn edit_map_value(ui: &mut Ui, value: &mut Value) {
    let mut text = display_string(value);
    if ui
        .add_sized(
            [VALUE_MAP_CELL_WIDTH, ui.spacing().interact_size.y],
            egui::TextEdit::singleline(&mut text),
        )
        .on_hover_text(if text.is_empty() { "<empty>" } else { &text })
        .changed()
    {
        *value = Value::String(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_title_preview_is_bounded_and_unicode_safe() {
        let value = Value::String("abcdefghijklmnopqrstuvwxyz0123456789".to_string());
        let preview = title_preview(&value);
        assert_eq!(preview.chars().count(), CONST_TITLE_CHAR_LIMIT);
        assert!(preview.ends_with("..."));

        let unicode = Value::String("配送日時を生成する非常に長い定数値".repeat(3));
        assert!(title_preview(&unicode).chars().count() <= CONST_TITLE_CHAR_LIMIT);
    }

    #[test]
    fn const_title_preview_stays_on_one_line() {
        let value = Value::String("first\nsecond\rthird".to_string());
        let preview = title_preview(&value);
        assert_eq!(preview, "first second third");
    }

    #[test]
    fn value_map_editor_bounds_large_tables() {
        let mut table = (0..41)
            .map(|index| {
                (
                    Value::String(format!("input-{index}")),
                    Value::String(format!("output-{index}")),
                )
            })
            .collect::<Vec<_>>();
        let mut default = None;
        let mut size = egui::Vec2::ZERO;
        let context = egui::Context::default();
        crate::icons::install(&context);

        let _ = context.run_ui(Default::default(), |ui| {
            size = ui
                .with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.scope(|ui| show_value_map_editor(ui, &mut table, &mut default, None))
                        .response
                        .rect
                        .size()
                })
                .inner;
        });

        assert!(
            size.x <= VALUE_MAP_CONTENT_WIDTH + 22.0,
            "value map editor was too wide: {size:?}"
        );
        assert!(
            size.y <= VALUE_MAP_MAX_HEIGHT + 60.0,
            "value map editor was too tall: {size:?}"
        );
    }

    #[test]
    fn value_map_wheel_offset_scrolls_and_clamps() {
        assert_eq!(value_map_wheel_offset(0.0, -90.0, 600.0, 170.0), 90.0);
        assert_eq!(value_map_wheel_offset(400.0, -90.0, 600.0, 170.0), 430.0);
        assert_eq!(value_map_wheel_offset(40.0, 90.0, 600.0, 170.0), 0.0);
        assert_eq!(value_map_wheel_offset(0.0, -90.0, 120.0, 170.0), 0.0);
    }
}
