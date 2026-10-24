//! Editor-wide appearance controls with a live canvas preview.

use egui::{RichText, Sense, Stroke, Vec2, pos2};

use crate::appearance::{
    AppearancePreset, CanvasPattern, EditorAppearance, MAX_CORNER_RADIUS, MAX_GRID_ANGLE_DEGREES,
    MAX_GRID_SPACING, MAX_GRID_STROKE_WIDTH, MAX_WIRE_FRAME_SIZE, MAX_WIRE_WIDTH,
    MIN_CORNER_RADIUS, MIN_GRID_ANGLE_DEGREES, MIN_GRID_SPACING, MIN_GRID_STROKE_WIDTH,
    MIN_WIRE_FRAME_SIZE, MIN_WIRE_WIDTH, RgbaColor, SemanticThemeColors, WireColorMode,
    WireFrameAdjustment, WireGeometry,
};
use crate::theme::{CustomTheme, ResolvedTheme, ThemeColor, ThemePreference, ThemeState, palette};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppearanceTab {
    #[default]
    Theme,
    Wires,
    Canvas,
}

pub fn show(
    context: &egui::Context,
    open: &mut bool,
    tab: &mut AppearanceTab,
    theme: &mut ThemeState,
    appearance: &mut EditorAppearance,
    palette: crate::theme::Palette,
) {
    if !*open {
        return;
    }
    let response = crate::ui_kit::dialog(context, crate::ui_kit::DialogKind::Docked, "Appearance")
        .id_salt("appearance_editor")
        .width(crate::ui_kit::DialogWidth::Medium)
        .show_body(|ui| {
             ui.horizontal_wrapped(|ui| {
                ui.selectable_value(tab, AppearanceTab::Theme, "Theme");
                ui.selectable_value(tab, AppearanceTab::Wires, "Wires");
                ui.selectable_value(tab, AppearanceTab::Canvas, "Canvas");
            });
            ui.separator();
            match tab {
                    AppearanceTab::Theme => show_theme(ui, theme),
                    AppearanceTab::Wires => show_wires(ui, appearance, palette),
                    AppearanceTab::Canvas => {
                        ui.horizontal(|ui| {
                            ui.label("Canvas density");
                            let mut density = appearance.density();
                            for option in crate::appearance::CanvasDensity::ALL {
                                ui.selectable_value(&mut density, option, option.label());
                            }
                            appearance.set_density(density);
                        });
                         ui.label("Compact is recommended. Canvas only; menus, dialogs and the inspector keep their normal size.");
                        show_canvas(ui, appearance);
                    }
                }
        });
    *open = !response.close_requested;
}

fn show_theme(ui: &mut egui::Ui, theme: &mut ThemeState) {
    ui.label(RichText::new("Workbench").strong());
    ui.horizontal_wrapped(|ui| {
        for preference in ThemePreference::ALL {
            ui.selectable_value(&mut theme.preference, preference, preference.label());
        }
    });

    if theme.preference != ThemePreference::Custom {
        return;
    }

    ui.horizontal(|ui| {
        ui.checkbox(&mut theme.custom.dark_mode, "Dark controls");
        ui.menu_button("Start from…", |ui| {
            if ui.button("Dark palette").clicked() {
                theme.custom = CustomTheme::from_palette(palette(ResolvedTheme::Dark), true);
                ui.close();
            }
            if ui.button("Light palette").clicked() {
                theme.custom = CustomTheme::from_palette(palette(ResolvedTheme::Light), false);
                ui.close();
            }
            if ui.button("High-contrast palette").clicked() {
                theme.custom =
                    CustomTheme::from_palette(palette(ResolvedTheme::HighContrast), true);
                ui.close();
            }
        })
        .response
        .on_hover_text(
            "Replace custom colors with a starting palette; then edit individual colors below",
        );
    });

    color_section(ui, "Surfaces", |ui| {
        theme_color(ui, "Background", &mut theme.custom.background);
        theme_color(ui, "Panel", &mut theme.custom.panel);
        theme_color(ui, "Surface", &mut theme.custom.surface);
        theme_color(ui, "Elevated", &mut theme.custom.elevated_surface);
        theme_color(ui, "Canvas", &mut theme.custom.canvas);
        theme_color(ui, "Border", &mut theme.custom.border);
        theme_color(ui, "Strong border", &mut theme.custom.strong_border);
        theme_color(ui, "Canvas grid", &mut theme.custom.canvas_grid);
    });
    color_section(ui, "Text and actions", |ui| {
        theme_color(ui, "Text", &mut theme.custom.text);
        theme_color(ui, "Muted text", &mut theme.custom.muted_text);
        theme_color(ui, "Action", &mut theme.custom.action);
        theme_color(ui, "Action hover", &mut theme.custom.action_hover);
        theme_color(ui, "On action", &mut theme.custom.on_action);
        theme_color(ui, "Selection", &mut theme.custom.selection);
        theme_color(ui, "Wire", &mut theme.custom.wire);
    });
    color_section(ui, "Data and status", |ui| {
        theme_color(ui, "Source", &mut theme.custom.source);
        theme_color(ui, "Target", &mut theme.custom.target);
        theme_color(ui, "Success", &mut theme.custom.success);
        theme_color(ui, "Warning", &mut theme.custom.warning);
        theme_color(ui, "Error", &mut theme.custom.error);
    });
}

fn show_wires(
    ui: &mut egui::Ui,
    appearance: &mut EditorAppearance,
    palette: crate::theme::Palette,
) {
    let mut wire = *appearance.wire();
    wire_preview(ui, wire.geometry(), wire.width(), appearance, palette);

    ui.label(RichText::new("Color").strong());
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(wire.color_mode() == WireColorMode::Theme, "Theme color")
            .clicked()
        {
            wire.set_color_mode(WireColorMode::Theme);
        }
        if ui
            .selectable_label(
                wire.color_mode() == WireColorMode::UniquePerWire,
                "Unique per wire",
            )
            .clicked()
        {
            wire.set_color_mode(WireColorMode::UniquePerWire);
        }
    });

    ui.add_space(crate::ui_kit::space::MD);
    ui.label(RichText::new("Geometry").strong());
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(
                matches!(wire.geometry(), WireGeometry::Straight),
                "Straight",
            )
            .clicked()
        {
            let _ = wire.set_geometry(WireGeometry::Straight);
        }
        if ui
            .selectable_label(
                matches!(wire.geometry(), WireGeometry::Orthogonal { .. }),
                "Orthogonal",
            )
            .clicked()
        {
            let radius = match wire.geometry() {
                WireGeometry::Orthogonal { corner_radius } => corner_radius,
                _ => 12.0,
            };
            let _ = wire.set_geometry(WireGeometry::Orthogonal {
                corner_radius: radius,
            });
        }
        if ui
            .selectable_label(matches!(wire.geometry(), WireGeometry::Bezier3), "Cubic")
            .clicked()
        {
            let _ = wire.set_geometry(WireGeometry::Bezier3);
        }
        if ui
            .selectable_label(matches!(wire.geometry(), WireGeometry::Bezier5), "Smooth")
            .clicked()
        {
            let _ = wire.set_geometry(WireGeometry::Bezier5);
        }
    });

    ui.add_space(crate::ui_kit::space::MD);
    let mut width = wire.width();
    if setting_slider(
        ui,
        "Width",
        &mut width,
        MIN_WIRE_WIDTH..=MAX_WIRE_WIDTH,
        " px",
    ) {
        let _ = wire.set_width(width);
    }
    let mut frame_size = wire.frame_size();
    if setting_slider(
        ui,
        "Curvature",
        &mut frame_size,
        MIN_WIRE_FRAME_SIZE..=MAX_WIRE_FRAME_SIZE,
        " px",
    ) {
        let _ = wire.set_frame_size(frame_size);
    }
    if let WireGeometry::Orthogonal { corner_radius } = wire.geometry() {
        let mut radius = corner_radius;
        if setting_slider(
            ui,
            "Corner radius",
            &mut radius,
            MIN_CORNER_RADIUS..=MAX_CORNER_RADIUS,
            " px",
        ) {
            let _ = wire.set_geometry(WireGeometry::Orthogonal {
                corner_radius: radius,
            });
        }
    }

    ui.horizontal(|ui| {
        setting_label(ui, "Distance behavior");
        egui::ComboBox::from_id_salt("wire_frame_adjustment")
            .selected_text(frame_adjustment_label(wire.frame_adjustment()))
            .show_ui(ui, |ui| {
                for adjustment in [
                    WireFrameAdjustment::Fixed,
                    WireFrameAdjustment::DownscaleNearby,
                    WireFrameAdjustment::UpscaleDistant,
                    WireFrameAdjustment::Adaptive,
                ] {
                    if ui
                        .selectable_label(
                            wire.frame_adjustment() == adjustment,
                            frame_adjustment_label(adjustment),
                        )
                        .clicked()
                    {
                        wire.set_frame_adjustment(adjustment);
                    }
                }
            });
    });
    appearance.set_wire(wire);
}

fn show_canvas(ui: &mut egui::Ui, appearance: &mut EditorAppearance) {
    let mut follow = appearance.follows_theme_colors();
    if ui
        .checkbox(&mut follow, "Follow workbench colors")
        .changed()
    {
        appearance.set_follow_theme_colors(follow);
    }

    ui.add_space(crate::ui_kit::space::MD);
    let mut canvas = *appearance.canvas();
    ui.label(RichText::new("Background").strong());
    let pattern = canvas.pattern();
    ui.horizontal(|ui| {
        if ui
            .selectable_label(pattern == CanvasPattern::None, "Plain")
            .clicked()
        {
            canvas.set_pattern(CanvasPattern::None);
        }
        if ui
            .selectable_label(pattern == CanvasPattern::Grid, "Grid")
            .clicked()
        {
            canvas.set_pattern(CanvasPattern::Grid);
        }
    });
    if canvas.pattern() == CanvasPattern::Grid {
        let mut spacing = canvas.grid_spacing();
        if setting_slider(
            ui,
            "Spacing",
            &mut spacing,
            MIN_GRID_SPACING..=MAX_GRID_SPACING,
            " px",
        ) {
            let _ = canvas.set_grid_spacing(spacing);
        }
        let mut angle = canvas.grid_angle_degrees();
        if setting_slider(
            ui,
            "Angle",
            &mut angle,
            MIN_GRID_ANGLE_DEGREES..=MAX_GRID_ANGLE_DEGREES,
            " degrees",
        ) {
            let _ = canvas.set_grid_angle_degrees(angle);
        }
        let mut stroke = canvas.grid_stroke_width();
        if setting_slider(
            ui,
            "Grid width",
            &mut stroke,
            MIN_GRID_STROKE_WIDTH..=MAX_GRID_STROKE_WIDTH,
            " px",
        ) {
            let _ = canvas.set_grid_stroke_width(stroke);
        }
    }
    appearance.set_canvas(canvas);

    if !appearance.follows_theme_colors() {
        ui.add_space(crate::ui_kit::space::MD);
        let mut colors = *appearance.colors();
        color_section(ui, "Canvas colors", |ui| {
            rgba_color(ui, "Canvas", &mut colors.canvas);
            rgba_color(ui, "Grid", &mut colors.grid);
            rgba_color(ui, "Node", &mut colors.node_surface);
            rgba_color(ui, "Node header", &mut colors.node_header);
            rgba_color(ui, "Node border", &mut colors.node_border);
            rgba_color(ui, "Wire", &mut colors.wire);
            rgba_color(ui, "Selection", &mut colors.selection);
        });
        color_section(ui, "Semantic colors", |ui| {
            rgba_color(ui, "Source", &mut colors.source);
            rgba_color(ui, "Target", &mut colors.target);
            rgba_color(ui, "Transform", &mut colors.transform);
            rgba_color(ui, "Warning", &mut colors.warning);
            rgba_color(ui, "Error", &mut colors.error);
        });
        ui.menu_button("Start from…", |ui| {
            if ui.button("Dark palette").clicked() {
                colors = SemanticThemeColors::preset(AppearancePreset::Dark);
                ui.close();
            }
            if ui.button("Light palette").clicked() {
                colors = SemanticThemeColors::preset(AppearancePreset::Light);
                ui.close();
            }
            if ui.button("High-contrast palette").clicked() {
                colors = SemanticThemeColors::preset(AppearancePreset::HighContrast);
                ui.close();
            }
        });
        appearance.set_colors(colors);
    }
}

fn color_section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    crate::ui_kit::collapsible_section(
        ui,
        ("appearance_colors", title),
        title,
        matches!(title, "Surfaces" | "Canvas colors"),
        add_contents,
    );
}

fn theme_color(ui: &mut egui::Ui, label: &str, color: &mut ThemeColor) {
    ui.horizontal(|ui| {
        setting_label(ui, label);
        let mut value = color.color32();
        if color_control(ui, label, &mut value) {
            color.set_color32(value);
        }
    });
}

fn rgba_color(ui: &mut egui::Ui, label: &str, color: &mut RgbaColor) {
    ui.horizontal(|ui| {
        setting_label(ui, label);
        let mut value = color.to_egui();
        if color_control(ui, label, &mut value) {
            color.set_egui(value);
        }
    });
}

fn setting_label(ui: &mut egui::Ui, label: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(
            crate::ui_kit::FieldWidth::Medium.min(),
            ui.spacing().interact_size.y,
        ),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(label);
        },
    );
}

fn color_control(ui: &mut egui::Ui, label: &str, color: &mut egui::Color32) -> bool {
    ui.push_id(label, |ui| {
        let swatch = ui.color_edit_button_srgba(color);
        ui.painter().rect_stroke(swatch.rect, crate::theme::METRICS.radius, Stroke::new(1.0, ui.visuals().strong_text_color()), egui::StrokeKind::Outside);
        let id = ui.make_persistent_id("hex_draft");
        let (mut hex, previous) = ui.ctx().data_mut(|data| data.get_temp::<(String, egui::Color32)>(id)).unwrap_or_else(|| (color.to_hex(), *color));
        if previous != *color { hex = color.to_hex(); }
        let response = ui.add(crate::ui_kit::text_field(&mut hex, crate::ui_kit::FieldWidth::Small).char_limit(9));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), format!("{label} hex")));
        let mut changed = swatch.changed();
        if response.changed() && let Ok(parsed) = egui::Color32::from_hex(&hex) {
            changed |= parsed != *color;
            *color = parsed;
        }
        if egui::Color32::from_hex(&hex).is_err() {
            response.clone().on_hover_text("Enter #RRGGBB or #RRGGBBAA. The current color stays unchanged until the value is valid.");
            if response.lost_focus() { hex = color.to_hex(); }
        }
        ui.ctx().data_mut(|data| data.insert_temp(id, (hex, *color)));
        changed
    }).inner
}

fn setting_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        setting_label(ui, label);
        changed = ui
            .add(egui::Slider::new(value, range).suffix(suffix))
            .changed();
    });
    changed
}

fn frame_adjustment_label(adjustment: WireFrameAdjustment) -> &'static str {
    match adjustment {
        WireFrameAdjustment::Fixed => "Fixed",
        WireFrameAdjustment::DownscaleNearby => "Compact nearby",
        WireFrameAdjustment::UpscaleDistant => "Expand distant",
        WireFrameAdjustment::Adaptive => "Adaptive",
    }
}

fn wire_preview(
    ui: &mut egui::Ui,
    geometry: WireGeometry,
    width: f32,
    appearance: &EditorAppearance,
    palette: crate::theme::Palette,
) {
    let desired = Vec2::new(ui.available_width(), 84.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    let painter = ui.painter_at(rect);
    let colors = appearance.resolved_colors(palette);
    let canvas = colors.canvas.to_egui();
    let border = colors.node_border.to_egui();
    painter.rect_filled(rect, 4.0, canvas);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );

    let mode = appearance.wire().color_mode();
    let wire_count = if mode == WireColorMode::UniquePerWire {
        3
    } else {
        1
    };
    for wire_index in 0..wire_count {
        let offset = (wire_index as f32 - (wire_count - 1) as f32 / 2.0) * 18.0;
        let start = pos2(rect.left() + 34.0, rect.center().y + offset);
        let end = pos2(rect.right() - 34.0, rect.center().y - offset);
        let color = crate::wire_colors::connected_color(
            mode,
            colors,
            crate::canvas::CanvasNode::TargetBlock(0),
            wire_index,
        );
        let points = preview_points(start, end, geometry);
        painter.add(egui::Shape::line(points, Stroke::new(width, color)));
        painter.circle_filled(start, 5.0, color);
        painter.circle_filled(end, 5.0, color);
    }
}

fn preview_points(start: egui::Pos2, end: egui::Pos2, geometry: WireGeometry) -> Vec<egui::Pos2> {
    match geometry {
        WireGeometry::Straight => vec![start, end],
        WireGeometry::Orthogonal { .. } => {
            let middle = (start.x + end.x) * 0.5;
            vec![start, pos2(middle, start.y), pos2(middle, end.y), end]
        }
        WireGeometry::Bezier3 => sample_curve(start, end, 0.42),
        WireGeometry::Bezier5 => sample_curve(start, end, 0.68),
    }
}

fn sample_curve(start: egui::Pos2, end: egui::Pos2, strength: f32) -> Vec<egui::Pos2> {
    let distance = end.x - start.x;
    let control_a = pos2(start.x + distance * strength, start.y - 48.0);
    let control_b = pos2(end.x - distance * strength, end.y + 48.0);
    (0..=32)
        .map(|step| {
            let t = step as f32 / 32.0;
            let one_minus = 1.0 - t;
            let a = one_minus.powi(3);
            let b = 3.0 * one_minus.powi(2) * t;
            let c = 3.0 * one_minus * t.powi(2);
            let d = t.powi(3);
            pos2(
                start.x * a + control_a.x * b + control_b.x * c + end.x * d,
                start.y * a + control_a.y * b + control_b.y * c + end.y * d,
            )
        })
        .collect()
}
