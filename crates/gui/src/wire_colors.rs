//! Deterministic, contrast-aware canvas wire colors.

use egui::{Color32, ecolor::Hsva};

use crate::appearance::{SemanticThemeColors, WireColorMode};
use crate::canvas::CanvasNode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireEmphasis {
    Normal,
    Incident,
    Unrelated,
}

pub fn output_color(mode: WireColorMode, colors: SemanticThemeColors) -> Color32 {
    match mode {
        WireColorMode::Theme => colors.wire.to_egui(),
        WireColorMode::UniquePerWire => contrast_anchor(colors.canvas.to_egui()),
    }
}

pub fn input_color(
    mode: WireColorMode,
    colors: SemanticThemeColors,
    node: CanvasNode,
    input: usize,
) -> Color32 {
    match mode {
        WireColorMode::Theme => colors.wire.to_egui(),
        WireColorMode::UniquePerWire => {
            unique_destination_color(colors.canvas.to_egui(), node, input)
        }
    }
}

pub fn connected_color(
    mode: WireColorMode,
    colors: SemanticThemeColors,
    node: CanvasNode,
    input: usize,
) -> Color32 {
    mix_colors(
        output_color(mode, colors),
        input_color(mode, colors, node, input),
    )
}

pub fn with_emphasis(color: Color32, canvas: Color32, emphasis: WireEmphasis) -> Color32 {
    const INCIDENT_BLEND: u16 = 64;
    const UNRELATED_ALPHA_PERCENT: u16 = 24;

    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    match emphasis {
        WireEmphasis::Normal => color,
        WireEmphasis::Incident => {
            let [anchor_red, anchor_green, anchor_blue, _] =
                contrast_anchor(canvas).to_srgba_unmultiplied();
            Color32::from_rgba_unmultiplied(
                blend_channel(red, anchor_red, INCIDENT_BLEND),
                blend_channel(green, anchor_green, INCIDENT_BLEND),
                blend_channel(blue, anchor_blue, INCIDENT_BLEND),
                alpha.max(224),
            )
        }
        WireEmphasis::Unrelated => Color32::from_rgba_unmultiplied(
            red,
            green,
            blue,
            u8::try_from(u16::from(alpha) * UNRELATED_ALPHA_PERCENT / 100).unwrap_or_default(),
        ),
    }
}

fn blend_channel(color: u8, anchor: u8, anchor_weight: u16) -> u8 {
    let color_weight = 256 - anchor_weight;
    u8::try_from((u16::from(color) * color_weight + u16::from(anchor) * anchor_weight) / 256)
        .unwrap_or_default()
}

fn unique_destination_color(canvas: Color32, node: CanvasNode, input: usize) -> Color32 {
    let node_key = match node {
        CanvasNode::SourceBlock(block) => 0x11_u64 << 56 | usize_key(block),
        CanvasNode::TargetBlock(block) => 0x22_u64 << 56 | usize_key(block),
        CanvasNode::Graph(id) => 0x33_u64 << 56 | u64::from(id),
        CanvasNode::Placeholder(id) => 0x44_u64 << 56 | u64::from(id),
    };
    let hue_bits = u32::try_from(splitmix64(node_key) >> 40).unwrap_or_default();
    let base_hue = hue_bits as f32 / 16_777_216.0;
    let input_step = u32::try_from(input % 4096).unwrap_or_default();
    let hue = (base_hue + input_step as f32 * 0.618_034).fract();
    let value = if is_dark(canvas) { 0.95 } else { 0.72 };
    Hsva {
        h: hue,
        s: 0.82,
        v: value,
        a: 1.0,
    }
    .into()
}

fn contrast_anchor(canvas: Color32) -> Color32 {
    if is_dark(canvas) {
        Color32::WHITE
    } else {
        Color32::from_gray(12)
    }
}

const fn is_dark(color: Color32) -> bool {
    let weighted = color.r() as u32 * 54 + color.g() as u32 * 183 + color.b() as u32 * 19;
    weighted < 128 * 256
}

const fn mix_colors(left: Color32, right: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(
        u8::midpoint(left.r(), right.r()),
        u8::midpoint(left.g(), right.g()),
        u8::midpoint(left.b(), right.b()),
        u8::midpoint(left.a(), right.a()),
    )
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn usize_key(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::appearance::AppearancePreset;

    #[test]
    fn theme_mode_keeps_the_configured_wire_color() {
        let colors = SemanticThemeColors::preset(AppearancePreset::Dark);

        assert_eq!(
            output_color(WireColorMode::Theme, colors),
            colors.wire.to_egui()
        );
        assert_eq!(
            input_color(WireColorMode::Theme, colors, CanvasNode::Graph(4), 2),
            colors.wire.to_egui()
        );
    }

    #[test]
    fn unique_mode_is_stable_and_distinguishes_destination_pins() {
        let colors = SemanticThemeColors::preset(AppearancePreset::Dark);
        let node = CanvasNode::TargetBlock(3);
        let generated = (0..32)
            .map(|input| {
                connected_color(WireColorMode::UniquePerWire, colors, node, input).to_array()
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(generated.len(), 32);
        assert_eq!(
            connected_color(WireColorMode::UniquePerWire, colors, node, 7),
            connected_color(WireColorMode::UniquePerWire, colors, node, 7)
        );
    }

    #[test]
    fn unique_mode_uses_opposite_anchors_for_light_and_dark_canvases() {
        let dark = SemanticThemeColors::preset(AppearancePreset::Dark);
        let light = SemanticThemeColors::preset(AppearancePreset::Light);

        assert_eq!(
            output_color(WireColorMode::UniquePerWire, dark),
            Color32::WHITE
        );
        assert_eq!(
            output_color(WireColorMode::UniquePerWire, light),
            Color32::from_gray(12)
        );
    }

    #[test]
    fn hover_emphasis_is_transient_contrasting_and_alpha_bounded() {
        let canvas = Color32::from_rgb(24, 28, 32);
        let base = Color32::from_rgba_unmultiplied(80, 140, 200, 180);

        assert_eq!(with_emphasis(base, canvas, WireEmphasis::Normal), base);

        let incident = with_emphasis(base, canvas, WireEmphasis::Incident);
        assert!(incident.r() > base.r());
        assert!(incident.g() > base.g());
        assert!(incident.b() > base.b());
        assert_eq!(incident.a(), 224);

        let unrelated = with_emphasis(base, canvas, WireEmphasis::Unrelated);
        for (actual, expected) in unrelated.to_srgba_unmultiplied()[..3]
            .iter()
            .zip([80_u8, 140, 200])
        {
            assert!(actual.abs_diff(expected) <= 3);
        }
        assert_eq!(unrelated.a(), 43);
    }
}
