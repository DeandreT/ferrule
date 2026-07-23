//! Small widgets for editing `ir::Value`s in place: a `Const` node's literal,
//! and a `ValueMap` node's lookup table.

use egui::Ui;
use ir::Value;

const VALUE_EDIT_WIDTH: f32 = 150.0;
const VALUE_MAP_CELL_WIDTH: f32 = 92.0;

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

/// Edits a `ValueMap`'s lookup table. Entries are coerced to strings for
/// editing -- a deliberate v1 simplification, since the common case (seen in
/// real-world mapping projects) is string-keyed lookup tables.
pub fn show_value_map_editor(
    ui: &mut Ui,
    table: &mut Vec<(Value, Value)>,
    default: &mut Option<Value>,
) {
    ui.label("lookup table");
    let mut remove_idx = None;
    for (i, (from, to)) in table.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut from_s = display_string(from);
            let mut to_s = display_string(to);
            if ui
                .add_sized(
                    [VALUE_MAP_CELL_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut from_s),
                )
                .changed()
            {
                *from = Value::String(from_s);
            }
            ui.label("->");
            if ui
                .add_sized(
                    [VALUE_MAP_CELL_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut to_s),
                )
                .changed()
            {
                *to = Value::String(to_s);
            }
            if ui.small_button("x").clicked() {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(i) = remove_idx {
        table.remove(i);
    }
    if ui.small_button("+ entry").clicked() {
        table.push((Value::String(String::new()), Value::String(String::new())));
    }

    let mut has_default = default.is_some();
    if ui.checkbox(&mut has_default, "has default").changed() {
        *default = has_default.then(|| Value::String(String::new()));
    }
    if let Some(d) = default {
        let mut d_s = display_string(d);
        if ui
            .add_sized(
                [VALUE_EDIT_WIDTH, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(&mut d_s),
            )
            .changed()
        {
            *d = Value::String(d_s);
        }
    }
}
