use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Button, FontData, FontFamily, FontId, Response, RichText, Role, Ui, WidgetInfo};
use lucide_icons::{Icon, LUCIDE_FONT_BYTES};

const FONT_NAME: &str = "ferrule-lucide";

pub fn install(ctx: &egui::Context) {
    ctx.add_font(FontInsert::new(
        FONT_NAME,
        FontData::from_static(LUCIDE_FONT_BYTES),
        vec![InsertFontFamily {
            family: family(),
            priority: FontPriority::Highest,
        }],
    ));
}

pub fn text(icon: Icon, size: f32) -> RichText {
    RichText::new(char::from(icon).to_string()).font(FontId::new(size, family()))
}

pub fn button(ui: &mut Ui, enabled: bool, icon: Icon, label: impl Into<String>) -> Response {
    let label = label.into();
    // Disabled buttons still name their action.
    let disabled_text = label.clone();
    button_with_disabled_text(ui, enabled, icon, label, disabled_text)
}

/// An icon button whose tooltip reads `disabled_text` while disabled.
pub fn button_with_disabled_text(
    ui: &mut Ui,
    enabled: bool,
    icon: Icon,
    label: impl Into<String>,
    disabled_text: impl Into<String>,
) -> Response {
    let label = label.into();
    let response = ui.add_enabled(
        enabled,
        Button::new(text(icon, crate::theme::METRICS.icon_size)).min_size(egui::vec2(
            crate::theme::METRICS.icon_button_size,
            crate::theme::METRICS.icon_button_size,
        )),
    );
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, &label));
    button_tooltips(ui, response, label, disabled_text)
}

/// Keep click-triggered tooltips quiet even when that click disables the button.
pub(crate) fn button_tooltips(
    ui: &mut Ui,
    response: Response,
    label: impl Into<String>,
    disabled_text: impl Into<String>,
) -> Response {
    // A click usually opens a dialog or menu under the pointer; the tooltip
    // stays hidden until the pointer leaves the button.
    let quiet = response.id.with("quiet_tooltip");
    let suppressed = if response.clicked() {
        ui.data_mut(|data| data.insert_temp(quiet, true));
        true
    } else if response.contains_pointer() {
        ui.data(|data| data.get_temp::<bool>(quiet).unwrap_or(false))
    } else {
        ui.data_mut(|data| data.remove::<bool>(quiet));
        false
    };
    if suppressed {
        return response;
    }
    response
        .on_hover_text(label.into())
        .on_disabled_hover_text(disabled_text.into())
}

fn family() -> FontFamily {
    FontFamily::Name(FONT_NAME.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt as _;

    #[test]
    fn icon_text_uses_the_bundled_font_family() {
        let text = text(Icon::Play, 16.0);
        assert_eq!(text.text(), char::from(Icon::Play).to_string());
    }

    #[test]
    fn clicked_tooltip_stays_quiet_when_the_button_becomes_disabled() {
        let context = egui::Context::default();
        let mut response = None;
        let mut frame = |enabled, events| {
            let _ = context.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let button = ui.add_enabled(enabled, Button::new("Undo"));
                    response = Some(button_tooltips(ui, button, "Undo", "Nothing to undo"));
                },
            );
            response.clone().unwrap()
        };
        let first = frame(true, Vec::new());
        let pos = first.rect.center();
        frame(
            true,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let clicked = frame(
            true,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(clicked.clicked());
        let quiet = clicked.id.with("quiet_tooltip");
        frame(false, Vec::new());
        assert_eq!(
            context.data(|data| data.get_temp::<bool>(quiet)),
            Some(true)
        );
        frame(
            false,
            vec![egui::Event::PointerMoved(egui::pos2(500.0, 300.0))],
        );
        assert_eq!(context.data(|data| data.get_temp::<bool>(quiet)), None);
    }
}
