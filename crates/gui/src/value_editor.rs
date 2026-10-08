//! Small widgets for editing `ir::Value`s in place: a `Const` node's literal,
//! and a `ValueMap` node's lookup table.

use egui::Ui;
use ir::Value;

const VALUE_EDIT_WIDTH: f32 = 150.0;
const VALUE_MAP_CELL_WIDTH: f32 = 104.0;
const VALUE_MAP_CONTENT_WIDTH: f32 = 640.0;
const VALUE_MAP_COMPACT_CONTENT_WIDTH: f32 = 320.0;
const VALUE_MAP_MAX_HEIGHT: f32 = 170.0;
const CONST_TITLE_CHAR_LIMIT: usize = 28;

fn parse_finite_float(text: &str) -> Option<f64> {
    let normalized = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| if ch == '−' { '-' } else { ch })
        .collect::<String>();
    normalized
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn accept_finite_float(value: &mut f64, proposed: Option<f64>) -> f64 {
    if let Some(proposed) = proposed.filter(|number| number.is_finite()) {
        *value = proposed;
    }
    *value
}

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
            ui.add(
                egui::DragValue::from_get_set(move |proposed| accept_finite_float(f, proposed))
                    .range(f64::MIN..=f64::MAX)
                    .clamp_existing_to_range(false)
                    .custom_parser(parse_finite_float),
            );
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

pub(super) fn value_map_editor_width(entry_count: usize) -> f32 {
    if entry_count <= 1 {
        VALUE_MAP_COMPACT_CONTENT_WIDTH
    } else {
        VALUE_MAP_CONTENT_WIDTH
    }
}

/// Edits a `ValueMap`'s ordered lookup table using validated cell drafts.
/// New entries and newly enabled defaults start as ordinary empty strings.
pub fn show_value_map_editor(
    ui: &mut Ui,
    table: &mut Vec<(Value, Value)>,
    default: &mut Option<Value>,
    wheel_delta_y: Option<f32>,
) {
    ui.vertical(|ui| {
        // Leave room for the solid scrollbar without reserving a second
        // entry column for an empty or single-entry table.
        let mut compact = table.len() <= 1;
        let content_width = value_map_editor_width(table.len());
        ui.set_min_width(content_width);
        ui.set_max_width(content_width);
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

        if compact && table.len() > 1 {
            compact = false;
            ui.set_min_width(VALUE_MAP_CONTENT_WIDTH);
            ui.set_max_width(VALUE_MAP_CONTENT_WIDTH);
            ui.ctx().request_repaint();
        }

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
                    mouse_wheel: true,
                })
                .show(ui, |ui| {
                    let grid = if compact {
                        egui::Grid::new("value_map_table_compact").num_columns(4)
                    } else {
                        egui::Grid::new("value_map_table").num_columns(9)
                    };
                    grid.striped(true).spacing([4.0, 3.0]).show(ui, |ui| {
                        for (row, entries) in table.chunks_mut(2).enumerate() {
                            show_value_map_entry(ui, row * 2, &mut entries[0], &mut remove_idx);
                            if !compact {
                                ui.weak("|");
                                if let Some(entry) = entries.get_mut(1) {
                                    show_value_map_entry(ui, row * 2 + 1, entry, &mut remove_idx);
                                } else {
                                    for _ in 0..4 {
                                        ui.label("");
                                    }
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
            if table.len() == 1 {
                ui.ctx().request_repaint();
            }
        }

        let mut has_default = default.is_some();
        ui.horizontal(|ui| {
            let changed = ui.checkbox(&mut has_default, "Default").changed();
            if changed {
                *default = has_default.then(|| Value::String(String::new()));
            }
            ui.push_id("value_map_default_cell", |ui| {
                if changed {
                    let id = ui.id().with("value_map_cell_draft");
                    ui.ctx().data_mut(|data| data.remove::<MapCellDraft>(id));
                }
                if let Some(value) = default {
                    edit_map_value(ui, value, "Default", false);
                }
            });
        });
    });
}

fn show_value_map_entry(
    ui: &mut Ui,
    index: usize,
    (from, to): &mut (Value, Value),
    remove_idx: &mut Option<usize>,
) {
    let key_draft_id = ui
        .push_id(("value_map_key", index), |ui| {
            let draft_id = ui.id().with("value_map_cell_draft");
            edit_map_value(ui, from, &format!("Entry {} key", index + 1), true);
            draft_id
        })
        .inner;
    ui.weak("->");
    let value_draft_id = ui
        .push_id(("value_map_value", index), |ui| {
            let draft_id = ui.id().with("value_map_cell_draft");
            edit_map_value(ui, to, &format!("Entry {} value", index + 1), true);
            draft_id
        })
        .inner;
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
    if remove_idx.as_ref().is_some_and(|removed| index >= *removed) {
        // Entries render in order before removal shifts their indices. Discard
        // this row's real child-UI drafts once its index ownership will change.
        ui.ctx().data_mut(|data| {
            data.remove::<MapCellDraft>(key_draft_id);
            data.remove::<MapCellDraft>(value_draft_id);
        });
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum MapCellKind {
    String,
    Int,
    Float,
    Bool,
    Absent,
    Imported,
}
impl MapCellKind {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::String(_) => Self::String,
            Value::Int(_) => Self::Int,
            Value::Float(_) => Self::Float,
            Value::Bool(_) => Self::Bool,
            Value::Null => Self::Absent,
            Value::JsonNull(_) | Value::XmlNil(_) => Self::Imported,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Absent => "absent",
            Self::Imported => "saved",
        }
    }

    fn parse(self, text: &str) -> Option<Value> {
        match self {
            Self::String => Some(Value::String(text.to_owned())),
            Self::Int => text.trim().parse().ok().map(Value::Int),
            Self::Float => text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map(Value::Float),
            Self::Bool => match text.trim() {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => None,
            },
            Self::Absent => Some(Value::Null),
            Self::Imported => None,
        }
    }
}

#[derive(Clone)]
struct MapCellDraft {
    original: Value,
    kind: MapCellKind,
    text: String,
}
impl MapCellDraft {
    fn new(value: &Value) -> Self {
        Self {
            original: value.clone(),
            kind: MapCellKind::from_value(value),
            text: display_string(value),
        }
    }

    fn follows(&self, value: &Value) -> bool {
        match (&self.original, value) {
            // Imported nonfinite values and signed zero are retained by their bits.
            (Value::Float(before), Value::Float(after)) => before.to_bits() == after.to_bits(),
            (before, after) => before == after,
        }
    }
}

fn edit_map_value(ui: &mut Ui, value: &mut Value, label: &str, stacked: bool) {
    let draft_id = ui.id().with("value_map_cell_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<MapCellDraft>(draft_id))
        .filter(|draft| draft.follows(value))
        .unwrap_or_else(|| MapCellDraft::new(value));
    let mut apply = false;
    let mut enter = false;
    let mut text_changed = false;
    let mut type_and_apply = |ui: &mut Ui, draft: &mut MapCellDraft| {
        let response = egui::ComboBox::from_id_salt("cell_type")
            .width(52.0)
            .selected_text(draft.kind.label())
            .show_ui(ui, |ui| {
                for candidate in [
                    MapCellKind::String,
                    MapCellKind::Int,
                    MapCellKind::Float,
                    MapCellKind::Bool,
                    MapCellKind::Absent,
                ] {
                    ui.selectable_value(&mut draft.kind, candidate, candidate.label());
                }
            })
            .response;
        response.widget_info(|| egui::WidgetInfo {
            current_text_value: Some(draft.kind.label().to_owned()),
            ..egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                ui.is_enabled(),
                format!("{label} type"),
            )
        });
        response.on_hover_text("Choose a type, then apply the complete value.");
        let valid = draft.kind.parse(&draft.text).is_some();
        let response = ui.add_enabled(
            valid,
            egui::Button::new(crate::icons::text(lucide_icons::Icon::Check, 12.0)),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled() && valid,
                format!("Apply {label}"),
            )
        });
        apply = response
            .on_hover_text(if valid {
                "Apply this cell, or press Enter in its value field."
            } else {
                "Enter a complete signed integer, finite number, or true/false value. Saved null markers require an explicit replacement type."
            })
            .clicked();
    };
    let mut value_field = |ui: &mut Ui, draft: &mut MapCellDraft| {
        let enabled = !matches!(draft.kind, MapCellKind::Absent | MapCellKind::Imported);
        let previous = draft.text.clone();
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    [VALUE_MAP_CELL_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut draft.text),
                )
            })
            .inner;
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::text_edit(ui.is_enabled() && enabled, &previous, &draft.text, "");
            info.label = Some(format!("{label} value"));
            info
        });
        text_changed = response.changed();
        enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        response.on_hover_text(if draft.text.is_empty() {
            "<empty>"
        } else {
            &draft.text
        });
    };
    if stacked {
        ui.vertical(|ui| {
            ui.horizontal(|ui| type_and_apply(ui, &mut draft));
            value_field(ui, &mut draft);
        });
    } else {
        ui.horizontal(|ui| {
            type_and_apply(ui, &mut draft);
            value_field(ui, &mut draft);
        });
    }
    // Ordinary text cells keep their existing immediate Cut/Paste/typing behavior.
    // Replacing an imported typed cell with text still requires an explicit commit.
    if text_changed
        && ui.is_enabled()
        && draft.kind == MapCellKind::String
        && matches!(value, Value::String(_))
    {
        *value = Value::String(draft.text.clone());
        draft.original = value.clone();
    }
    if (apply || enter)
        && ui.is_enabled()
        && let Some(next) = draft.kind.parse(&draft.text)
    {
        *value = next;
        draft = MapCellDraft::new(value);
    }
    ui.ctx().data_mut(|data| data.insert_temp(draft_id, draft));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_editor_rejects_nonfinite_text_and_retains_normalized_finite_input() {
        for text in ["NaN", "inf", "-inf", "Infinity", "1e999"] {
            assert_eq!(parse_finite_float(text), None, "{text}");
        }
        assert_eq!(parse_finite_float(" \u{2009}−1 234.5 \t"), Some(-1234.5));
        assert_eq!(
            parse_finite_float(" -0 ").map(f64::to_bits),
            Some((-0.0_f64).to_bits())
        );
        let mut current = 3.0;
        assert_eq!(accept_finite_float(&mut current, Some(f64::INFINITY)), 3.0);
        assert_eq!(accept_finite_float(&mut current, Some(f64::NAN)), 3.0);
        assert_eq!(
            accept_finite_float(&mut current, Some(-0.0)).to_bits(),
            (-0.0_f64).to_bits()
        );
    }

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
