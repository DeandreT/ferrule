use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{FontData, FontFamily};

const FONT_NAME: &str = "ferrule-cjk-fallback";
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansCJKsc-Regular.otf");

pub fn install(ctx: &egui::Context) {
    ctx.add_font(FontInsert::new(
        FONT_NAME,
        FontData::from_static(FONT_BYTES),
        vec![
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Lowest,
            },
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt as _;
    use egui::epaint::text::FontsView;
    use egui::{Color32, FontDefinitions, FontId};

    fn with_fonts<T>(ctx: &egui::Context, mut inspect: impl FnMut(&mut FontsView<'_>) -> T) -> T {
        let mut result = None;
        let _ = ctx.run_test_ui(egui::RawInput::default(), |ui| {
            result = Some(ui.ctx().fonts_mut(|fonts| inspect(fonts)));
        });
        result.expect("the headless pass inspected fonts")
    }

    fn assert_characters(fonts: &mut FontsView<'_>, family: &FontFamily, text: &str) {
        let characters = fonts.characters(family);
        for character in text.chars() {
            assert!(
                characters.contains_key(&character),
                "{family:?} is missing {character:?}"
            );
        }
    }

    #[test]
    fn cjk_fallback_renders_han_kana_hangul_and_supplementary_labels() {
        let ctx = egui::Context::default();
        install(&ctx);
        with_fonts(&ctx, |fonts| {
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                let text = "中文数据 漢字 日本語 かなカナ 한국어 㐀 𠮷";
                assert_characters(fonts, &family, text);
                let font_id = FontId::new(14.0, family);
                let galley = fonts.layout_no_wrap(text.to_owned(), font_id, Color32::BLACK);
                assert!(!galley.is_empty());
                assert!(galley.size().x > 0.0);
            }
        });
    }

    #[test]
    fn cjk_fallback_preserves_existing_font_order_and_latin_greek_metrics() {
        let baseline = egui::Context::default();
        let ctx = egui::Context::default();
        install(&ctx);
        let default_families = FontDefinitions::default().families;

        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let font_id = FontId::new(14.0, family.clone());
            let baseline_size = with_fonts(&baseline, |fonts| {
                fonts
                    .layout_no_wrap("Alpha 42 α Ω".to_owned(), font_id.clone(), Color32::BLACK)
                    .size()
            });
            let fallback_size = with_fonts(&ctx, |fonts| {
                let mut expected = default_families[&family].clone();
                expected.push(FONT_NAME.to_owned());
                assert_eq!(fonts.definitions().families[&family], expected);
                assert_characters(fonts, &family, "Alpha 42 α Ω");
                fonts
                    .layout_no_wrap("Alpha 42 α Ω".to_owned(), font_id.clone(), Color32::BLACK)
                    .size()
            });
            assert_eq!(fallback_size, baseline_size);
        }
    }

    #[test]
    fn cjk_fallback_keeps_lucide_in_its_separate_family() {
        let ctx = egui::Context::default();
        install(&ctx);
        crate::icons::install(&ctx);
        with_fonts(&ctx, |fonts| {
            let family = FontFamily::Name("ferrule-lucide".into());
            assert_eq!(
                fonts.definitions().families[&family],
                vec!["ferrule-lucide".to_owned()]
            );
            assert_characters(
                fonts,
                &family,
                &char::from(lucide_icons::Icon::Play).to_string(),
            );
            assert!(!fonts.characters(&family).contains_key(&'中'));
        });
    }
}
