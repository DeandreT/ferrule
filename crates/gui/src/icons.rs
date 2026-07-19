use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Button, FontData, FontFamily, FontId, Response, RichText, Ui, WidgetInfo, WidgetType};
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
    let response = ui.add_enabled(
        enabled,
        Button::new(text(icon, crate::theme::METRICS.icon_size)).min_size(egui::vec2(
            crate::theme::METRICS.icon_button_size,
            crate::theme::METRICS.icon_button_size,
        )),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, &label));
    response.on_hover_text(label)
}

fn family() -> FontFamily {
    FontFamily::Name(FONT_NAME.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_text_uses_the_bundled_font_family() {
        let text = text(Icon::Play, 16.0);
        assert_eq!(text.text(), char::from(Icon::Play).to_string());
    }
}
