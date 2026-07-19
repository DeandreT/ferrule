//! Serializable graph-canvas appearance independent of editor document state.

use std::fmt;

use egui::{Color32, CornerRadius, Frame, Margin, Stroke, vec2};
use egui_snarl::ui::{BackgroundPattern, Grid, SnarlStyle, WireStyle};
use serde::{Deserialize, Deserializer, Serialize};

pub const MIN_WIRE_WIDTH: f32 = 0.5;
pub const MAX_WIRE_WIDTH: f32 = 8.0;
pub const MIN_WIRE_FRAME_SIZE: f32 = 8.0;
pub const MAX_WIRE_FRAME_SIZE: f32 = 240.0;
pub const MIN_CORNER_RADIUS: f32 = 0.0;
pub const MAX_CORNER_RADIUS: f32 = 64.0;
pub const MIN_GRID_SPACING: f32 = 8.0;
pub const MAX_GRID_SPACING: f32 = 256.0;
pub const MIN_GRID_STROKE_WIDTH: f32 = 0.25;
pub const MAX_GRID_STROKE_WIDTH: f32 = 4.0;
pub const MIN_GRID_ANGLE_DEGREES: f32 = -180.0;
pub const MAX_GRID_ANGLE_DEGREES: f32 = 180.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearancePreset {
    #[default]
    Dark,
    Light,
    HighContrast,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireGeometry {
    Straight,
    Orthogonal {
        corner_radius: f32,
    },
    Bezier3,
    #[default]
    Bezier5,
}

impl WireGeometry {
    fn validate(self) -> Result<(), AppearanceError> {
        if let Self::Orthogonal { corner_radius } = self {
            validate_number(
                "wire corner radius",
                corner_radius,
                MIN_CORNER_RADIUS,
                MAX_CORNER_RADIUS,
            )?;
        }
        Ok(())
    }

    fn to_snarl(self) -> WireStyle {
        match self {
            Self::Straight => WireStyle::Line,
            Self::Orthogonal { corner_radius } => WireStyle::AxisAligned { corner_radius },
            Self::Bezier3 => WireStyle::Bezier3,
            Self::Bezier5 => WireStyle::Bezier5,
        }
    }
}

/// How the requested wire frame responds to endpoint distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireFrameAdjustment {
    /// Use the configured frame size at every distance.
    Fixed,
    /// Reduce the frame when endpoints are too close for the configured size.
    #[default]
    DownscaleNearby,
    /// Increase the frame as endpoints move farther apart.
    UpscaleDistant,
    /// Apply both distance adjustments.
    Adaptive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireColorMode {
    #[default]
    Theme,
    UniquePerWire,
}

impl WireFrameAdjustment {
    const fn flags(self) -> (bool, bool) {
        match self {
            Self::Fixed => (false, false),
            Self::DownscaleNearby => (false, true),
            Self::UpscaleDistant => (true, false),
            Self::Adaptive => (true, true),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WireAppearance {
    geometry: WireGeometry,
    width: f32,
    frame_size: f32,
    frame_adjustment: WireFrameAdjustment,
    color_mode: WireColorMode,
}

impl WireAppearance {
    pub fn new(
        geometry: WireGeometry,
        width: f32,
        frame_size: f32,
        frame_adjustment: WireFrameAdjustment,
    ) -> Result<Self, AppearanceError> {
        geometry.validate()?;
        validate_number("wire width", width, MIN_WIRE_WIDTH, MAX_WIRE_WIDTH)?;
        validate_number(
            "wire frame size",
            frame_size,
            MIN_WIRE_FRAME_SIZE,
            MAX_WIRE_FRAME_SIZE,
        )?;
        Ok(Self {
            geometry,
            width,
            frame_size,
            frame_adjustment,
            color_mode: WireColorMode::Theme,
        })
    }

    pub const fn geometry(self) -> WireGeometry {
        self.geometry
    }

    pub const fn width(self) -> f32 {
        self.width
    }

    pub const fn frame_size(self) -> f32 {
        self.frame_size
    }

    pub const fn frame_adjustment(self) -> WireFrameAdjustment {
        self.frame_adjustment
    }

    pub const fn color_mode(self) -> WireColorMode {
        self.color_mode
    }

    pub fn set_geometry(&mut self, geometry: WireGeometry) -> Result<(), AppearanceError> {
        geometry.validate()?;
        self.geometry = geometry;
        Ok(())
    }

    pub fn set_width(&mut self, width: f32) -> Result<(), AppearanceError> {
        validate_number("wire width", width, MIN_WIRE_WIDTH, MAX_WIRE_WIDTH)?;
        self.width = width;
        Ok(())
    }

    pub fn set_frame_size(&mut self, frame_size: f32) -> Result<(), AppearanceError> {
        validate_number(
            "wire frame size",
            frame_size,
            MIN_WIRE_FRAME_SIZE,
            MAX_WIRE_FRAME_SIZE,
        )?;
        self.frame_size = frame_size;
        Ok(())
    }

    pub fn set_frame_adjustment(&mut self, adjustment: WireFrameAdjustment) {
        self.frame_adjustment = adjustment;
    }

    pub fn set_color_mode(&mut self, color_mode: WireColorMode) {
        self.color_mode = color_mode;
    }
}

impl Default for WireAppearance {
    fn default() -> Self {
        Self {
            geometry: WireGeometry::Bezier5,
            width: 2.0,
            frame_size: 80.0,
            frame_adjustment: WireFrameAdjustment::DownscaleNearby,
            color_mode: WireColorMode::Theme,
        }
    }
}

impl<'de> Deserialize<'de> for WireAppearance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(default)]
        struct WireDocument {
            geometry: WireGeometry,
            width: f32,
            frame_size: f32,
            frame_adjustment: WireFrameAdjustment,
            color_mode: WireColorMode,
        }

        impl Default for WireDocument {
            fn default() -> Self {
                let wire = WireAppearance::default();
                Self {
                    geometry: wire.geometry,
                    width: wire.width,
                    frame_size: wire.frame_size,
                    frame_adjustment: wire.frame_adjustment,
                    color_mode: wire.color_mode,
                }
            }
        }

        let document = WireDocument::deserialize(deserializer)?;
        let mut wire = Self::new(
            document.geometry,
            document.width,
            document.frame_size,
            document.frame_adjustment,
        )
        .map_err(serde::de::Error::custom)?;
        wire.set_color_mode(document.color_mode);
        Ok(wire)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanvasPattern {
    None,
    #[default]
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CanvasAppearance {
    pattern: CanvasPattern,
    grid_spacing: f32,
    grid_angle_degrees: f32,
    grid_stroke_width: f32,
}

impl CanvasAppearance {
    pub fn new(
        pattern: CanvasPattern,
        grid_spacing: f32,
        grid_angle_degrees: f32,
        grid_stroke_width: f32,
    ) -> Result<Self, AppearanceError> {
        validate_number(
            "grid spacing",
            grid_spacing,
            MIN_GRID_SPACING,
            MAX_GRID_SPACING,
        )?;
        validate_number(
            "grid angle",
            grid_angle_degrees,
            MIN_GRID_ANGLE_DEGREES,
            MAX_GRID_ANGLE_DEGREES,
        )?;
        validate_number(
            "grid stroke width",
            grid_stroke_width,
            MIN_GRID_STROKE_WIDTH,
            MAX_GRID_STROKE_WIDTH,
        )?;
        Ok(Self {
            pattern,
            grid_spacing,
            grid_angle_degrees,
            grid_stroke_width,
        })
    }

    pub const fn pattern(self) -> CanvasPattern {
        self.pattern
    }

    pub const fn grid_spacing(self) -> f32 {
        self.grid_spacing
    }

    pub const fn grid_angle_degrees(self) -> f32 {
        self.grid_angle_degrees
    }

    pub const fn grid_stroke_width(self) -> f32 {
        self.grid_stroke_width
    }

    pub fn set_pattern(&mut self, pattern: CanvasPattern) {
        self.pattern = pattern;
    }

    pub fn set_grid_spacing(&mut self, spacing: f32) -> Result<(), AppearanceError> {
        validate_number("grid spacing", spacing, MIN_GRID_SPACING, MAX_GRID_SPACING)?;
        self.grid_spacing = spacing;
        Ok(())
    }

    pub fn set_grid_angle_degrees(&mut self, angle: f32) -> Result<(), AppearanceError> {
        validate_number(
            "grid angle",
            angle,
            MIN_GRID_ANGLE_DEGREES,
            MAX_GRID_ANGLE_DEGREES,
        )?;
        self.grid_angle_degrees = angle;
        Ok(())
    }

    pub fn set_grid_stroke_width(&mut self, width: f32) -> Result<(), AppearanceError> {
        validate_number(
            "grid stroke width",
            width,
            MIN_GRID_STROKE_WIDTH,
            MAX_GRID_STROKE_WIDTH,
        )?;
        self.grid_stroke_width = width;
        Ok(())
    }
}

impl Default for CanvasAppearance {
    fn default() -> Self {
        Self {
            pattern: CanvasPattern::Grid,
            grid_spacing: 32.0,
            grid_angle_degrees: 0.0,
            grid_stroke_width: 1.0,
        }
    }
}

impl<'de> Deserialize<'de> for CanvasAppearance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(default)]
        struct CanvasDocument {
            pattern: CanvasPattern,
            grid_spacing: f32,
            grid_angle_degrees: f32,
            grid_stroke_width: f32,
        }

        impl Default for CanvasDocument {
            fn default() -> Self {
                let canvas = CanvasAppearance::default();
                Self {
                    pattern: canvas.pattern,
                    grid_spacing: canvas.grid_spacing,
                    grid_angle_degrees: canvas.grid_angle_degrees,
                    grid_stroke_width: canvas.grid_stroke_width,
                }
            }
        }

        let canvas = CanvasDocument::deserialize(deserializer)?;
        Self::new(
            canvas.pattern,
            canvas.grid_spacing,
            canvas.grid_angle_degrees,
            canvas.grid_stroke_width,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// RGBA bytes serialize as a compact, renderer-independent four-item array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RgbaColor([u8; 4]);

impl RgbaColor {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self([red, green, blue, 255])
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self([red, green, blue, alpha])
    }

    pub const fn to_egui(self) -> Color32 {
        Color32::from_rgba_unmultiplied_const(self.0[0], self.0[1], self.0[2], self.0[3])
    }

    pub fn set_egui(&mut self, color: Color32) {
        self.0 = color.to_srgba_unmultiplied();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticThemeColors {
    pub canvas: RgbaColor,
    pub grid: RgbaColor,
    pub node_surface: RgbaColor,
    pub node_header: RgbaColor,
    pub node_border: RgbaColor,
    pub wire: RgbaColor,
    pub source: RgbaColor,
    pub target: RgbaColor,
    pub transform: RgbaColor,
    pub selection: RgbaColor,
    pub warning: RgbaColor,
    pub error: RgbaColor,
}

impl SemanticThemeColors {
    pub fn from_palette(colors: crate::theme::Palette) -> Self {
        Self {
            canvas: RgbaColor(colors.canvas.to_srgba_unmultiplied()),
            grid: RgbaColor(colors.canvas_grid.to_srgba_unmultiplied()),
            node_surface: RgbaColor(colors.surface.to_srgba_unmultiplied()),
            node_header: RgbaColor(colors.elevated_surface.to_srgba_unmultiplied()),
            node_border: RgbaColor(colors.border.to_srgba_unmultiplied()),
            wire: RgbaColor(colors.wire.to_srgba_unmultiplied()),
            source: RgbaColor(colors.source.to_srgba_unmultiplied()),
            target: RgbaColor(colors.target.to_srgba_unmultiplied()),
            transform: RgbaColor(colors.action.to_srgba_unmultiplied()),
            selection: RgbaColor(colors.selection.to_srgba_unmultiplied()),
            warning: RgbaColor(colors.warning.to_srgba_unmultiplied()),
            error: RgbaColor(colors.error.to_srgba_unmultiplied()),
        }
    }

    pub const fn preset(preset: AppearancePreset) -> Self {
        match preset {
            AppearancePreset::Dark => Self {
                canvas: RgbaColor::rgb(12, 14, 16),
                grid: RgbaColor::rgb(38, 43, 48),
                node_surface: RgbaColor::rgb(33, 37, 42),
                node_header: RgbaColor::rgb(43, 48, 54),
                node_border: RgbaColor::rgb(67, 75, 84),
                wire: RgbaColor::rgb(135, 146, 157),
                source: RgbaColor::rgb(68, 199, 199),
                target: RgbaColor::rgb(105, 207, 145),
                transform: RgbaColor::rgb(75, 151, 232),
                selection: RgbaColor::rgba(75, 151, 232, 72),
                warning: RgbaColor::rgb(244, 184, 96),
                error: RgbaColor::rgb(244, 123, 130),
            },
            AppearancePreset::Light => Self {
                canvas: RgbaColor::rgb(250, 251, 252),
                grid: RgbaColor::rgb(220, 225, 229),
                node_surface: RgbaColor::rgb(255, 255, 255),
                node_header: RgbaColor::rgb(238, 241, 243),
                node_border: RgbaColor::rgb(193, 201, 208),
                wire: RgbaColor::rgb(91, 103, 115),
                source: RgbaColor::rgb(8, 127, 131),
                target: RgbaColor::rgb(40, 124, 69),
                transform: RgbaColor::rgb(29, 99, 181),
                selection: RgbaColor::rgba(29, 99, 181, 56),
                warning: RgbaColor::rgb(143, 87, 0),
                error: RgbaColor::rgb(180, 35, 47),
            },
            AppearancePreset::HighContrast => Self {
                canvas: RgbaColor::rgb(0, 0, 0),
                grid: RgbaColor::rgb(74, 74, 74),
                node_surface: RgbaColor::rgb(20, 20, 20),
                node_header: RgbaColor::rgb(30, 30, 30),
                node_border: RgbaColor::rgb(255, 255, 255),
                wire: RgbaColor::rgb(255, 255, 255),
                source: RgbaColor::rgb(0, 229, 255),
                target: RgbaColor::rgb(0, 255, 133),
                transform: RgbaColor::rgb(0, 183, 255),
                selection: RgbaColor::rgba(0, 183, 255, 96),
                warning: RgbaColor::rgb(255, 215, 0),
                error: RgbaColor::rgb(255, 106, 130),
            },
        }
    }
}

impl Default for SemanticThemeColors {
    fn default() -> Self {
        Self::preset(AppearancePreset::Dark)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorAppearance {
    wire: WireAppearance,
    canvas: CanvasAppearance,
    colors: SemanticThemeColors,
    follow_theme_colors: bool,
}

impl EditorAppearance {
    pub const fn preset(preset: AppearancePreset) -> Self {
        Self {
            wire: WireAppearance {
                geometry: WireGeometry::Bezier5,
                width: 2.0,
                frame_size: 80.0,
                frame_adjustment: WireFrameAdjustment::DownscaleNearby,
                color_mode: WireColorMode::Theme,
            },
            canvas: CanvasAppearance {
                pattern: CanvasPattern::Grid,
                grid_spacing: 32.0,
                grid_angle_degrees: 0.0,
                grid_stroke_width: 1.0,
            },
            colors: SemanticThemeColors::preset(preset),
            follow_theme_colors: false,
        }
    }

    pub const fn wire(&self) -> &WireAppearance {
        &self.wire
    }

    pub const fn canvas(&self) -> &CanvasAppearance {
        &self.canvas
    }

    pub const fn colors(&self) -> &SemanticThemeColors {
        &self.colors
    }

    pub const fn follows_theme_colors(&self) -> bool {
        self.follow_theme_colors
    }

    pub fn set_wire(&mut self, wire: WireAppearance) {
        self.wire = wire;
    }

    pub fn set_canvas(&mut self, canvas: CanvasAppearance) {
        self.canvas = canvas;
    }

    pub fn set_colors(&mut self, colors: SemanticThemeColors) {
        self.colors = colors;
    }

    pub fn set_follow_theme_colors(&mut self, follow: bool) {
        self.follow_theme_colors = follow;
    }

    pub fn to_snarl_style(self) -> SnarlStyle {
        let mut style = SnarlStyle::new();
        let (upscale, downscale) = self.wire.frame_adjustment.flags();
        style.wire_width = Some(self.wire.width);
        style.wire_frame_size = Some(self.wire.frame_size);
        style.upscale_wire_frame = Some(upscale);
        style.downscale_wire_frame = Some(downscale);
        style.wire_style = Some(self.wire.geometry.to_snarl());
        style.pin_fill = Some(self.colors.wire.to_egui());
        style.pin_stroke = Some(Stroke::new(1.0, self.colors.node_border.to_egui()));

        style.bg_frame = Some(Frame::new().fill(self.colors.canvas.to_egui()));
        style.bg_pattern = Some(match self.canvas.pattern {
            CanvasPattern::None => BackgroundPattern::NoPattern,
            CanvasPattern::Grid => BackgroundPattern::Grid(Grid::new(
                vec2(self.canvas.grid_spacing, self.canvas.grid_spacing),
                self.canvas.grid_angle_degrees.to_radians(),
            )),
        });
        style.bg_pattern_stroke = Some(Stroke::new(
            self.canvas.grid_stroke_width,
            self.colors.grid.to_egui(),
        ));

        style.node_frame = Some(
            Frame::new()
                .inner_margin(Margin::same(8))
                .fill(self.colors.node_surface.to_egui())
                .stroke(Stroke::new(1.0, self.colors.node_border.to_egui()))
                .corner_radius(CornerRadius::same(4)),
        );
        style.header_frame = Some(
            Frame::new()
                .inner_margin(Margin::symmetric(8, 4))
                .fill(self.colors.node_header.to_egui())
                .stroke(Stroke::new(1.0, self.colors.node_border.to_egui()))
                .corner_radius(CornerRadius::same(4)),
        );
        style.select_stoke = Some(Stroke::new(1.5, self.colors.selection.to_egui()));
        style.select_fill = Some(self.colors.selection.to_egui());
        style
    }

    pub fn to_snarl_style_with_palette(self, palette: crate::theme::Palette) -> SnarlStyle {
        let mut resolved = self;
        resolved.colors = self.resolved_colors(palette);
        resolved.to_snarl_style()
    }

    pub fn resolved_colors(self, palette: crate::theme::Palette) -> SemanticThemeColors {
        if self.follow_theme_colors {
            SemanticThemeColors::from_palette(palette)
        } else {
            self.colors
        }
    }
}

impl Default for EditorAppearance {
    fn default() -> Self {
        let mut appearance = Self::preset(AppearancePreset::Dark);
        appearance.follow_theme_colors = true;
        appearance
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppearanceError {
    OutOfRange {
        field: &'static str,
        value: f32,
        minimum: f32,
        maximum: f32,
    },
}

impl fmt::Display for AppearanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange {
                field,
                value,
                minimum,
                maximum,
            } => write!(
                formatter,
                "{field} must be a finite value from {minimum} through {maximum}, got {value}"
            ),
        }
    }
}

impl std::error::Error for AppearanceError {}

fn validate_number(
    field: &'static str,
    value: f32,
    minimum: f32,
    maximum: f32,
) -> Result<(), AppearanceError> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(AppearanceError::OutOfRange {
            field,
            value,
            minimum,
            maximum,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_convert_every_supported_snarl_style_field() {
        let appearance = EditorAppearance::default();
        let style = appearance.to_snarl_style();

        assert_eq!(style.wire_width, Some(2.0));
        assert_eq!(style.wire_frame_size, Some(80.0));
        assert_eq!(style.upscale_wire_frame, Some(false));
        assert_eq!(style.downscale_wire_frame, Some(true));
        assert_eq!(style.wire_style, Some(WireStyle::Bezier5));
        assert_eq!(style.pin_fill, Some(appearance.colors().wire.to_egui()));
        assert_eq!(
            style.bg_frame.map(|frame| frame.fill),
            Some(appearance.colors().canvas.to_egui())
        );
        let Some(BackgroundPattern::Grid(grid)) = style.bg_pattern else {
            panic!("default appearance should use a grid");
        };
        assert_eq!(grid.spacing, vec2(32.0, 32.0));
        assert_eq!(grid.angle, 0.0);
    }

    #[test]
    fn every_wire_geometry_and_frame_adjustment_maps_exactly() {
        let geometries = [
            (WireGeometry::Straight, WireStyle::Line),
            (
                WireGeometry::Orthogonal {
                    corner_radius: 12.0,
                },
                WireStyle::AxisAligned {
                    corner_radius: 12.0,
                },
            ),
            (WireGeometry::Bezier3, WireStyle::Bezier3),
            (WireGeometry::Bezier5, WireStyle::Bezier5),
        ];
        for (geometry, expected) in geometries {
            let wire = WireAppearance::new(geometry, 2.0, 80.0, WireFrameAdjustment::Fixed)
                .expect("wire is valid");
            let mut appearance = EditorAppearance::default();
            appearance.set_wire(wire);
            assert_eq!(appearance.to_snarl_style().wire_style, Some(expected));
        }

        for (adjustment, expected) in [
            (WireFrameAdjustment::Fixed, (false, false)),
            (WireFrameAdjustment::DownscaleNearby, (false, true)),
            (WireFrameAdjustment::UpscaleDistant, (true, false)),
            (WireFrameAdjustment::Adaptive, (true, true)),
        ] {
            let mut wire = WireAppearance::default();
            wire.set_frame_adjustment(adjustment);
            let mut appearance = EditorAppearance::default();
            appearance.set_wire(wire);
            let style = appearance.to_snarl_style();
            assert_eq!(
                (style.upscale_wire_frame, style.downscale_wire_frame),
                (Some(expected.0), Some(expected.1))
            );
        }
    }

    #[test]
    fn setters_reject_non_finite_and_out_of_range_values_atomically() {
        let mut wire = WireAppearance::default();
        assert!(wire.set_width(f32::NAN).is_err());
        assert!(wire.set_width(MAX_WIRE_WIDTH + 1.0).is_err());
        assert_eq!(wire.width(), 2.0);

        let mut canvas = CanvasAppearance::default();
        assert!(canvas.set_grid_spacing(MIN_GRID_SPACING - 1.0).is_err());
        assert!(canvas.set_grid_angle_degrees(f32::INFINITY).is_err());
        assert_eq!(canvas.grid_spacing(), 32.0);
        assert_eq!(canvas.grid_angle_degrees(), 0.0);

        assert!(
            WireAppearance::new(
                WireGeometry::Orthogonal {
                    corner_radius: MAX_CORNER_RADIUS + 1.0,
                },
                2.0,
                80.0,
                WireFrameAdjustment::Fixed,
            )
            .is_err()
        );
    }

    #[test]
    fn deserialization_applies_defaults_and_rejects_invalid_numbers() {
        let partial: EditorAppearance = serde_json::from_str(r#"{"wire":{"width":3.5}}"#)
            .expect("missing appearance fields receive defaults");
        assert_eq!(partial.wire().width(), 3.5);
        assert_eq!(partial.wire().frame_size(), 80.0);
        assert_eq!(partial.wire().color_mode(), WireColorMode::Theme);

        let error =
            serde_json::from_str::<EditorAppearance>(r#"{"canvas":{"grid_spacing":500.0}}"#)
                .expect_err("invalid persisted spacing must fail");
        assert!(error.to_string().contains("grid spacing"));
    }

    #[test]
    fn rgba_colors_and_custom_themes_roundtrip_without_renderer_types() {
        let mut appearance = EditorAppearance::preset(AppearancePreset::Light);
        let mut colors = *appearance.colors();
        colors.source = RgbaColor::rgba(64, 128, 192, 128);
        appearance.set_colors(colors);

        let json = serde_json::to_string(&appearance).expect("appearance serializes");
        assert!(json.contains("[64,128,192,128]"));
        let decoded: EditorAppearance =
            serde_json::from_str(&json).expect("appearance deserializes");
        assert_eq!(decoded, appearance);
        assert_eq!(decoded.colors().source, RgbaColor::rgba(64, 128, 192, 128));
        assert_eq!(
            decoded.colors().source.to_egui(),
            Color32::from_rgba_unmultiplied(64, 128, 192, 128)
        );
    }

    #[test]
    fn unique_wire_colors_roundtrip_and_old_documents_keep_theme_color() {
        let mut appearance = EditorAppearance::default();
        let mut wire = *appearance.wire();
        wire.set_color_mode(WireColorMode::UniquePerWire);
        appearance.set_wire(wire);

        let json = serde_json::to_string(&appearance).expect("appearance serializes");
        let decoded: EditorAppearance =
            serde_json::from_str(&json).expect("appearance deserializes");
        let legacy: EditorAppearance = serde_json::from_str(r#"{"wire":{"width":2.5}}"#)
            .expect("legacy appearance deserializes");

        assert_eq!(decoded.wire().color_mode(), WireColorMode::UniquePerWire);
        assert_eq!(legacy.wire().color_mode(), WireColorMode::Theme);
    }

    #[test]
    fn no_pattern_and_rotated_grid_map_to_snarl_types() {
        let mut appearance = EditorAppearance::default();
        appearance.set_canvas(
            CanvasAppearance::new(CanvasPattern::None, 40.0, 90.0, 2.0).expect("canvas is valid"),
        );
        assert_eq!(
            appearance.to_snarl_style().bg_pattern,
            Some(BackgroundPattern::NoPattern)
        );

        let mut canvas = *appearance.canvas();
        canvas.set_pattern(CanvasPattern::Grid);
        appearance.set_canvas(canvas);
        let Some(BackgroundPattern::Grid(grid)) = appearance.to_snarl_style().bg_pattern else {
            panic!("grid pattern should map to a grid");
        };
        assert_eq!(grid.spacing, vec2(40.0, 40.0));
        assert_eq!(grid.angle, std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn built_in_presets_have_distinct_semantic_contrast_sets() {
        let dark = EditorAppearance::preset(AppearancePreset::Dark);
        let light = EditorAppearance::preset(AppearancePreset::Light);
        let high_contrast = EditorAppearance::preset(AppearancePreset::HighContrast);

        assert_ne!(dark.colors(), light.colors());
        assert_ne!(dark.colors(), high_contrast.colors());
        assert_eq!(high_contrast.colors().node_border.to_egui(), Color32::WHITE);
    }
}
