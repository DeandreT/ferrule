//! Small widgets for editing `ir::Value`s in place: a `Const` node's literal,
//! and a `ValueMap` node's lookup table.

use egui::Ui;
use ir::Value;

const VALUE_EDIT_WIDTH: f32 = 150.0;
const VALUE_MAP_CELL_WIDTH: f32 = 240.0;
const VALUE_MAP_PREVIEW_CELL_WIDTH: f32 = 116.0;
const VALUE_MAP_PREVIEW_ROWS: usize = 3;

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
            );
        }
        Value::XmlNil(_) => {}
    }
}

pub fn show_value_map_preview(
    ui: &mut Ui,
    table: &[(Value, Value)],
    default: Option<&Value>,
) -> bool {
    ui.set_min_width(280.0);
    ui.set_max_width(300.0);
    let mut edit = false;
    ui.horizontal(|ui| {
        ui.weak(match table.len() {
            1 => "1 entry".to_string(),
            count => format!("{count} entries"),
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(crate::icons::text(
                    lucide_icons::Icon::Pencil,
                    14.0,
                )))
                .on_hover_text("Edit value map")
                .clicked()
            {
                edit = true;
            }
        });
    });
    if table.is_empty() {
        ui.weak("No mappings");
    } else {
        egui::Grid::new(ui.id().with("value_map_preview"))
            .num_columns(3)
            .spacing([6.0, 3.0])
            .show(ui, |ui| {
                for (from, to) in table.iter().take(VALUE_MAP_PREVIEW_ROWS) {
                    preview_value(ui, from);
                    ui.weak("->");
                    preview_value(ui, to);
                    ui.end_row();
                }
            });
        if table.len() > VALUE_MAP_PREVIEW_ROWS {
            ui.weak(format!("+{} more", table.len() - VALUE_MAP_PREVIEW_ROWS));
        }
    }
    match default {
        Some(value) => {
            ui.horizontal(|ui| {
                ui.weak("Default");
                preview_value(ui, value);
            });
        }
        None => {
            ui.weak("No default");
        }
    }
    edit
}

fn preview_value(ui: &mut Ui, value: &Value) {
    let text = display_string(value);
    ui.add_sized(
        [VALUE_MAP_PREVIEW_CELL_WIDTH, ui.spacing().interact_size.y],
        egui::Label::new(&text).truncate(),
    )
    .on_hover_text(if text.is_empty() { "<empty>" } else { &text });
}

/// Edits a `ValueMap`'s complete lookup table. Entries are coerced to strings
/// while editing, matching the original inline editor's behavior.
pub fn show_value_map_editor(
    ui: &mut Ui,
    table: &mut Vec<(Value, Value)>,
    default: &mut Option<Value>,
) {
    ui.horizontal(|ui| {
        ui.strong("Lookup table");
        ui.weak(match table.len() {
            1 => "1 entry".to_string(),
            count => format!("{count} entries"),
        });
    });
    let mut remove_idx = None;
    egui::ScrollArea::both()
        .id_salt("value_map_table_scroll")
        .max_height(440.0)
        .min_scrolled_width(VALUE_MAP_CELL_WIDTH * 2.0 + 110.0)
        .show(ui, |ui| {
            egui::Grid::new("value_map_table")
                .num_columns(4)
                .striped(true)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.weak("#");
                    ui.strong("Input");
                    ui.strong("Output");
                    ui.end_row();
                    for (i, (from, to)) in table.iter_mut().enumerate() {
                        ui.weak((i + 1).to_string());
                        edit_map_value(ui, from);
                        edit_map_value(ui, to);
                        if ui
                            .add(egui::Button::new(crate::icons::text(
                                lucide_icons::Icon::Trash2,
                                14.0,
                            )))
                            .on_hover_text("Remove entry")
                            .clicked()
                        {
                            remove_idx = Some(i);
                        }
                        ui.end_row();
                    }
                });
        });
    if let Some(i) = remove_idx {
        table.remove(i);
    }
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

    ui.separator();
    let mut has_default = default.is_some();
    if ui.checkbox(&mut has_default, "Use default value").changed() {
        *default = has_default.then(|| Value::String(String::new()));
    }
    if let Some(value) = default {
        ui.horizontal(|ui| {
            ui.label("Default");
            edit_map_value(ui, value);
        });
    }
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
