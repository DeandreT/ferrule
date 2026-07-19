//! Shared visual foundation for the desktop editor.
//!
//! The palette keeps structural surfaces neutral and reserves color for
//! actions, data-flow roles, and diagnostics. Semantic colors must always be
//! paired with an icon, shape, or text label at the call site.

use egui::style::WidgetVisuals;
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, Style, TextStyle, Theme,
    Visuals, vec2,
};
use serde::{Deserialize, Serialize};

/// Stable dimensions shared by the workspace shell and graph canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InteractionMetrics {
    pub control_height: f32,
    pub icon_button_size: f32,
    pub icon_size: f32,
    pub toolbar_height: f32,
    pub panel_margin: f32,
    pub section_spacing: f32,
    pub node_pin_size: f32,
    pub node_row_height: f32,
}

pub const METRICS: InteractionMetrics = InteractionMetrics {
    control_height: 32.0,
    icon_button_size: 32.0,
    icon_size: 16.0,
    toolbar_height: 38.0,
    panel_margin: 10.0,
    section_spacing: 12.0,
    node_pin_size: 12.0,
    node_row_height: 28.0,
};

/// User-facing theme choice. High contrast intentionally uses a dark base so
/// it remains deterministic instead of changing meaning with the system theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Dark,
    Light,
    HighContrast,
}

impl ThemePreference {
    pub const ALL: [Self; 4] = [Self::System, Self::Dark, Self::Light, Self::HighContrast];

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::HighContrast => "High contrast",
        }
    }
}

/// Serializable desktop appearance state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeState {
    pub preference: ThemePreference,
}

impl ThemeState {
    /// Installs both ordinary palettes, selects the requested egui theme, and
    /// returns the palette that is active for this frame.
    pub fn apply(self, context: &egui::Context) -> Palette {
        context.set_style_of(Theme::Dark, style(ResolvedTheme::Dark));
        context.set_style_of(Theme::Light, style(ResolvedTheme::Light));

        let resolved = match self.preference {
            ThemePreference::System => {
                context.set_theme(egui::ThemePreference::System);
                ResolvedTheme::from_system(context.system_theme())
            }
            ThemePreference::Dark => {
                context.set_theme(Theme::Dark);
                ResolvedTheme::Dark
            }
            ThemePreference::Light => {
                context.set_theme(Theme::Light);
                ResolvedTheme::Light
            }
            ThemePreference::HighContrast => {
                context.set_style_of(Theme::Dark, style(ResolvedTheme::HighContrast));
                context.set_theme(Theme::Dark);
                ResolvedTheme::HighContrast
            }
        };
        palette(resolved)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedTheme {
    Dark,
    Light,
    HighContrast,
}

impl ResolvedTheme {
    fn from_system(system: Option<Theme>) -> Self {
        match system {
            Some(Theme::Light) => Self::Light,
            Some(Theme::Dark) | None => Self::Dark,
        }
    }
}

/// Theme colors used outside standard egui widgets, especially the graph and
/// diagnostics. Surfaces are deliberately neutral rather than tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub background: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub elevated_surface: Color32,
    pub canvas: Color32,
    pub text: Color32,
    pub muted_text: Color32,
    pub border: Color32,
    pub strong_border: Color32,
    pub action: Color32,
    pub action_hover: Color32,
    pub on_action: Color32,
    pub selection: Color32,
    pub source: Color32,
    pub target: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub canvas_grid: Color32,
    pub wire: Color32,
}

pub const fn palette(theme: ResolvedTheme) -> Palette {
    match theme {
        ResolvedTheme::Dark => Palette {
            background: Color32::from_rgb(17, 19, 21),
            panel: Color32::from_rgb(24, 27, 31),
            surface: Color32::from_rgb(33, 37, 42),
            elevated_surface: Color32::from_rgb(43, 48, 54),
            canvas: Color32::from_rgb(12, 14, 16),
            text: Color32::from_rgb(240, 243, 246),
            muted_text: Color32::from_rgb(174, 182, 190),
            border: Color32::from_rgb(67, 75, 84),
            strong_border: Color32::from_rgb(102, 113, 124),
            action: Color32::from_rgb(75, 151, 232),
            action_hover: Color32::from_rgb(100, 170, 244),
            on_action: Color32::from_rgb(7, 18, 29),
            selection: Color32::from_rgb(43, 91, 139),
            source: Color32::from_rgb(68, 199, 199),
            target: Color32::from_rgb(105, 207, 145),
            success: Color32::from_rgb(105, 207, 145),
            warning: Color32::from_rgb(244, 184, 96),
            error: Color32::from_rgb(244, 123, 130),
            canvas_grid: Color32::from_rgb(38, 43, 48),
            wire: Color32::from_rgb(135, 146, 157),
        },
        ResolvedTheme::Light => Palette {
            background: Color32::from_rgb(245, 247, 248),
            panel: Color32::from_rgb(255, 255, 255),
            surface: Color32::from_rgb(238, 241, 243),
            elevated_surface: Color32::from_rgb(255, 255, 255),
            canvas: Color32::from_rgb(250, 251, 252),
            text: Color32::from_rgb(23, 32, 42),
            muted_text: Color32::from_rgb(82, 94, 106),
            border: Color32::from_rgb(193, 201, 208),
            strong_border: Color32::from_rgb(128, 140, 151),
            action: Color32::from_rgb(29, 99, 181),
            action_hover: Color32::from_rgb(22, 82, 153),
            on_action: Color32::WHITE,
            selection: Color32::from_rgb(194, 221, 249),
            source: Color32::from_rgb(8, 127, 131),
            target: Color32::from_rgb(40, 124, 69),
            success: Color32::from_rgb(29, 113, 55),
            warning: Color32::from_rgb(143, 87, 0),
            error: Color32::from_rgb(180, 35, 47),
            canvas_grid: Color32::from_rgb(220, 225, 229),
            wire: Color32::from_rgb(91, 103, 115),
        },
        ResolvedTheme::HighContrast => Palette {
            background: Color32::BLACK,
            panel: Color32::from_rgb(8, 8, 8),
            surface: Color32::from_rgb(20, 20, 20),
            elevated_surface: Color32::from_rgb(30, 30, 30),
            canvas: Color32::BLACK,
            text: Color32::WHITE,
            muted_text: Color32::from_rgb(222, 222, 222),
            border: Color32::from_rgb(170, 170, 170),
            strong_border: Color32::WHITE,
            action: Color32::from_rgb(0, 183, 255),
            action_hover: Color32::from_rgb(80, 207, 255),
            on_action: Color32::BLACK,
            selection: Color32::from_rgb(0, 91, 128),
            source: Color32::from_rgb(0, 229, 255),
            target: Color32::from_rgb(0, 255, 133),
            success: Color32::from_rgb(0, 255, 133),
            warning: Color32::from_rgb(255, 215, 0),
            error: Color32::from_rgb(255, 106, 130),
            canvas_grid: Color32::from_rgb(74, 74, 74),
            wire: Color32::WHITE,
        },
    }
}

pub fn style(theme: ResolvedTheme) -> Style {
    let colors = palette(theme);
    let mut style = match theme {
        ResolvedTheme::Light => Theme::Light.default_style(),
        ResolvedTheme::Dark | ResolvedTheme::HighContrast => Theme::Dark.default_style(),
    };

    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();
    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.window_margin = Margin::same(10);
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.button_padding = vec2(8.0, 4.0);
    style.spacing.interact_size = vec2(METRICS.control_height, METRICS.control_height);
    style.spacing.icon_width = METRICS.icon_size;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.icon_spacing = 6.0;
    style.spacing.indent = 22.0;
    style.spacing.scroll = egui::style::ScrollStyle::thin();
    style.animation_time = 0.14;
    style.explanation_tooltips = true;
    style.url_in_tooltip = true;

    style.visuals = visuals(colors, theme != ResolvedTheme::Light);
    style
}

fn visuals(colors: Palette, dark_mode: bool) -> Visuals {
    let radius = CornerRadius::same(4);
    let noninteractive = widget(
        colors.surface,
        colors.surface,
        colors.border,
        colors.text,
        radius,
    );
    let inactive = widget(
        colors.elevated_surface,
        colors.surface,
        colors.border,
        colors.text,
        radius,
    );
    let hovered = widget(
        colors.elevated_surface,
        colors.elevated_surface,
        colors.action_hover,
        colors.text,
        radius,
    );
    let active = widget(
        colors.action,
        colors.action,
        colors.action_hover,
        colors.text,
        radius,
    );
    let open = widget(
        colors.elevated_surface,
        colors.elevated_surface,
        colors.action,
        colors.text,
        radius,
    );
    let mut visuals = if dark_mode {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    visuals.override_text_color = Some(colors.text);
    visuals.weak_text_color = Some(colors.muted_text);
    visuals.widgets = egui::style::Widgets {
        noninteractive,
        inactive,
        hovered,
        active,
        open,
    };
    visuals.selection.bg_fill = colors.selection;
    visuals.selection.stroke = Stroke::new(1.5, colors.text);
    visuals.hyperlink_color = colors.action_hover;
    visuals.faint_bg_color = colors.surface;
    visuals.extreme_bg_color = colors.background;
    visuals.text_edit_bg_color = Some(colors.background);
    visuals.code_bg_color = colors.surface;
    visuals.warn_fg_color = colors.warning;
    visuals.error_fg_color = colors.error;
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.window_shadow = Shadow::NONE;
    visuals.window_fill = colors.elevated_surface;
    visuals.window_stroke = Stroke::new(1.0, colors.strong_border);
    visuals.menu_corner_radius = radius;
    visuals.panel_fill = colors.panel;
    visuals.popup_shadow = Shadow::NONE;
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = true;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.disabled_alpha = if dark_mode { 0.62 } else { 0.55 };
    visuals
}

fn widget(
    bg_fill: Color32,
    weak_bg_fill: Color32,
    border: Color32,
    foreground: Color32,
    corner_radius: CornerRadius,
) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill,
        weak_bg_fill,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius,
        fg_stroke: Stroke::new(1.0, foreground),
        expansion: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_resolution_is_deterministic_when_the_platform_has_no_preference() {
        assert_eq!(ResolvedTheme::from_system(None), ResolvedTheme::Dark);
        assert_eq!(
            ResolvedTheme::from_system(Some(Theme::Light)),
            ResolvedTheme::Light
        );
        assert_eq!(
            ResolvedTheme::from_system(Some(Theme::Dark)),
            ResolvedTheme::Dark
        );
    }

    #[test]
    fn theme_application_selects_the_matching_egui_theme_and_metrics() {
        let context = egui::Context::default();
        let active = ThemeState {
            preference: ThemePreference::Light,
        }
        .apply(&context);
        assert_eq!(active, palette(ResolvedTheme::Light));
        assert_eq!(context.theme(), Theme::Light);
        assert!(!context.global_style().visuals.dark_mode);
        assert_eq!(
            context.global_style().spacing.interact_size.y,
            METRICS.control_height
        );

        ThemeState {
            preference: ThemePreference::HighContrast,
        }
        .apply(&context);
        assert_eq!(context.theme(), Theme::Dark);
        assert_eq!(
            context.global_style().visuals.panel_fill,
            Color32::from_rgb(8, 8, 8)
        );
    }

    #[test]
    fn body_text_and_diagnostic_text_meet_normal_text_contrast() {
        for theme in [
            ResolvedTheme::Dark,
            ResolvedTheme::Light,
            ResolvedTheme::HighContrast,
        ] {
            let colors = palette(theme);
            for foreground in [colors.text, colors.muted_text, colors.warning, colors.error] {
                assert!(contrast_ratio(foreground, colors.panel) >= 4.5);
            }
            assert!(contrast_ratio(colors.on_action, colors.action) >= 4.5);
        }
    }

    #[test]
    fn graph_semantics_and_boundaries_remain_visible_without_text() {
        for theme in [
            ResolvedTheme::Dark,
            ResolvedTheme::Light,
            ResolvedTheme::HighContrast,
        ] {
            let colors = palette(theme);
            for foreground in [
                colors.source,
                colors.target,
                colors.strong_border,
                colors.wire,
            ] {
                assert!(contrast_ratio(foreground, colors.canvas) >= 3.0);
            }
        }
    }

    #[test]
    fn high_contrast_primary_text_exceeds_enhanced_contrast() {
        let colors = palette(ResolvedTheme::HighContrast);
        assert!(contrast_ratio(colors.text, colors.panel) >= 7.0);
        assert!(contrast_ratio(colors.muted_text, colors.panel) >= 7.0);
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f32 {
        let lighter = relative_luminance(a).max(relative_luminance(b));
        let darker = relative_luminance(a).min(relative_luminance(b));
        (lighter + 0.05) / (darker + 0.05)
    }

    fn relative_luminance(color: Color32) -> f32 {
        fn channel(value: u8) -> f32 {
            let value = f32::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }

        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }
}
