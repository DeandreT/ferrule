use egui_snarl::ui::{SnarlStyle, SnarlWidget};
use egui_snarl::{NodeId as SnarlNodeId, Snarl};

use crate::canvas::CanvasNode;
use crate::canvas_search::CanvasSearchState;
use crate::graph_viewer::GraphViewer;

const CANVAS_ID: &str = "mapping_canvas";
#[path = "fit.rs"]
mod fit;
pub(crate) use fit::record_node_rect;
const EDGE_PAN_ZONE: f32 = 72.0;
const EDGE_PAN_SPEED: f32 = 900.0;

#[derive(Clone, Copy, Debug)]
pub struct CanvasInteraction {
    pub viewport_width: f32,
    pub focused: bool,
}

pub struct CanvasOptions {
    pub tools_id: egui::Id,
    pub density: crate::appearance::CanvasDensity,
    pub wire: crate::appearance::WireAppearance,
    pub request_compact: bool,
    pub id_salt: egui::Id,
    pub show_minimap: bool,
    pub view_generation: u64,
    pub style: SnarlStyle,
    /// One navigation request in graph coordinates, consumed when shown.
    pub focus: Option<egui::Pos2>,
}

#[derive(Clone, Default)]
struct PinInteractionIds(Vec<egui::Id>);

#[derive(Clone, Copy, Default)]
struct CanvasFocus {
    graph_position: egui::Pos2,
    zoom: Option<f32>,
}

#[derive(Clone, Copy)]
// The last painted transform lets endpoint wheel input be consumed before
// egui::Scene decides whether to pan the canvas behind an interactive node.
struct CanvasTransform(egui::emath::TSTransform);

pub fn show(
    snarl: &mut Snarl<CanvasNode>,
    viewer: &mut GraphViewer<'_>,
    search: &mut CanvasSearchState,
    options: CanvasOptions,
    ui: &mut egui::Ui,
) -> CanvasInteraction {
    let canvas_id = egui::Id::unique((CANVAS_ID, options.id_salt, options.view_generation));
    let generation_id = options.tools_id.with("camera_generation");
    let previous_generation = ui.ctx().data(|data| data.get_temp::<u64>(generation_id));
    let owner_id = canvas_id.with("camera_owner");
    let previous_owner = ui.ctx().data(|data| data.get_temp::<egui::Id>(owner_id));
    if previous_owner != Some(options.tools_id) {
        fit::reset(ui.ctx(), canvas_id);
    }
    if previous_generation.is_some_and(|generation| generation != options.view_generation) {
        fit::request_all(ui.ctx(), canvas_id);
    }
    ui.ctx().data_mut(|data| {
        data.insert_temp(generation_id, options.view_generation);
        data.insert_temp(owner_id, options.tools_id);
    });
    let mut style = options.style;
    style.min_scale = Some(0.05);
    let viewport = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    let focused = ui.ctx().input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|position| viewport.contains(position))
    });
    let dragged_id = ui.ctx().dragged_id();
    let (pointer, primary_down, frame_seconds) = ui.ctx().input(|input| {
        (
            input.pointer.latest_pos(),
            input.pointer.primary_down(),
            input.stable_dt.min(0.05),
        )
    });
    let previous_pin_ids = ui.ctx().data(|data| {
        data.get_temp::<PinInteractionIds>(canvas_id.with("pin_interaction_ids"))
            .unwrap_or_default()
    });
    let wire_dragging = pin_drag_active(primary_down, dragged_id, &previous_pin_ids.0);
    viewer.camera_pan = edge_pan_delta(viewport, pointer, wire_dragging, frame_seconds);
    if viewer.camera_pan != egui::Vec2::ZERO {
        ui.ctx().request_repaint();
    }
    viewer.pin_interaction_ids.clear();
    if viewer.sync_endpoint_view(snarl) {
        ui.ctx().request_repaint();
    }
    let hover_marker = canvas_id.with("hovered_node");
    let focus_marker = canvas_id.with("minimap_focus");
    let transform_marker = canvas_id.with("transform");
    let minimap_focus = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<CanvasFocus>(focus_marker));
    viewer.camera_focus = options
        .focus
        .map(|position| (position, viewport.center(), None))
        .or_else(|| {
            minimap_focus.map(|focus| (focus.graph_position, viewport.center(), focus.zoom))
        });
    viewer.canvas_transform = None;
    let hovered_node = ui
        .ctx()
        .data(|data| data.get_temp::<Option<SnarlNodeId>>(hover_marker).flatten());
    viewer.begin_node_hover_frame(hovered_node);
    let selected = SnarlWidget::new()
        .id(canvas_id)
        .style(style)
        .get_selected_nodes(ui);
    crate::canvas_tools::begin(ui.ctx(), options.tools_id, viewer, snarl);
    crate::canvas_tools::toolbar(
        ui,
        viewer,
        snarl,
        &selected,
        crate::canvas_tools::ToolbarOptions {
            wire: options.wire,
            requested: options.request_compact,
            search_open: search.is_open(),
        },
    );
    let viewport = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    let find = !crate::canvas_tools::picker_open(ui.ctx())
        && !GraphViewer::properties_open(ui.ctx())
        && !ui.ctx().egui_wants_keyboard_input()
        && ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
    if find {
        search.open_selected(&selected, snarl);
    }
    viewer.endpoint_search_match = search.active_match(viewer.source_blocks, viewer.target_blocks);
    let previous_transform = ui
        .ctx()
        .data(|data| data.get_temp::<CanvasTransform>(transform_marker));
    // egui's Ctrl+wheel factor is intentionally strong for generic scenes.
    // Override only this canvas camera, retaining the point under the pointer.
    let zoom_delta = ui.ctx().input(|input| input.zoom_delta());
    if zoom_delta != 1.0
        && let (Some(pointer), Some(transform)) = (pointer, previous_transform)
        && viewport.contains(pointer)
        && !egui::Popup::is_any_open(ui.ctx())
        && ui
            .ctx()
            .layer_id_at(pointer)
            .is_none_or(|layer| layer.order <= ui.layer_id().order)
    {
        viewer.camera_focus = Some((
            transform.0.inverse() * pointer,
            pointer,
            Some(zoom_step(transform.0.scaling, zoom_delta.powf(0.25))),
        ));
    }
    let node_wheel = (!wire_dragging)
        .then(|| {
            let delta_y = ui.ctx().input(|input| input.smooth_scroll_delta().y);
            pointer
                .zip(previous_transform)
                .map(|(pointer, transform)| (transform.0.inverse() * pointer, delta_y))
        })
        .flatten()
        .filter(|(_, delta_y)| *delta_y != 0.0);
    if let Some((graph_position, delta_y)) = node_wheel
        && viewer.scroll_endpoint_at(graph_position, delta_y, snarl)
    {
        // Consume the vertical component before egui::Scene can interpret it
        // as canvas panning. Horizontal trackpad motion remains available.
        ui.ctx()
            .input_mut(|input| input.smooth_scroll_delta.y = 0.0);
        ui.ctx().request_repaint();
    }
    let delete = !search.is_open()
        && !crate::canvas_tools::picker_open(ui.ctx())
        && !egui::Popup::is_any_open(ui.ctx())
        && !GraphViewer::properties_open(ui.ctx())
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.ctx().input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        });
    if delete {
        viewer.remove_snarl_nodes_recorded(&selected, snarl);
    }
    let frame = style
        .bg_frame
        .unwrap_or_else(|| egui::Frame::canvas(ui.style()));
    let fit_viewport =
        (ui.available_rect_before_wrap() - frame.total_margin()).intersect(ui.clip_rect());
    let magnification = if style.crisp_magnified_text.unwrap_or(false) {
        style.max_scale.unwrap_or(2.0)
    } else {
        1.0
    };
    let maximum_zoom = style.max_scale.unwrap_or(2.0) / magnification;
    if crate::canvas_tools::take_fit_request(ui.ctx()) {
        fit::request_all(ui.ctx(), canvas_id);
    }
    let pointer_navigation = pointer.is_some_and(|pointer| viewport.contains(pointer))
        && (primary_down
            || ui
                .ctx()
                .input(|input| input.smooth_scroll_delta() != egui::Vec2::ZERO));
    let (fitted, minimum_zoom) = fit::begin_frame(
        ui.ctx(),
        canvas_id,
        fit_viewport,
        snarl,
        maximum_zoom,
        viewer.camera_focus.is_some() || wire_dragging || pointer_navigation,
    );
    if let Some(fitted) = fitted {
        viewer.camera_focus = Some((fitted.graph_center, fitted.screen_center, Some(fitted.zoom)));
    }
    if let Some(minimum_zoom) = minimum_zoom {
        style.min_scale = Some(
            style
                .min_scale
                .unwrap_or(0.2)
                .min(minimum_zoom * magnification),
        );
    }
    viewer.endpoint_scroll.begin_geometry_frame();
    viewer.sync_endpoint_view(snarl);
    crate::canvas_tools::begin(ui.ctx(), options.tools_id, viewer, snarl);
    ui.scope(|ui| {
        options.density.apply(ui);
        SnarlWidget::new()
            .id(canvas_id)
            .style(style)
            .show(snarl, viewer, ui);
    });
    if let Some(transform) = viewer.canvas_transform {
        ui.ctx()
            .data_mut(|data| data.insert_temp(transform_marker, CanvasTransform(transform)));
        if let Some(focus) = show_navigation(ui, canvas_id, viewport, transform) {
            match focus {
                Navigation::Zoom(zoom) => {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            focus_marker,
                            CanvasFocus {
                                graph_position: transform.inverse() * viewport.center(),
                                zoom: Some(zoom),
                            },
                        )
                    });
                }
                Navigation::Fit => fit::request_all(ui.ctx(), canvas_id),
            }
            ui.ctx().request_repaint();
        }
    }
    crate::canvas_search::show(
        ui.ctx(),
        canvas_id,
        viewport,
        search,
        viewer.source_blocks,
        viewer.target_blocks,
        viewer.endpoint_scroll,
    );
    // Header filters, scrollbars, search and graph edits can change the visible
    // semantic-to-visual pin projection during the pass. Reconcile after native
    // node interaction has finished; never mutate the mapping to update a view.
    if viewer.sync_endpoint_view(snarl) {
        ui.ctx().request_repaint();
    }
    if options.show_minimap
        && let (Some(transform), Some(node_sizes)) =
            (viewer.canvas_transform, viewer.node_sizes.as_deref())
        && let Some(navigation) = crate::canvas_minimap::show(
            ui,
            canvas_id,
            viewport,
            snarl,
            node_sizes,
            transform,
            viewer.colors,
        )
    {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                focus_marker,
                CanvasFocus {
                    graph_position: navigation.graph_position,
                    zoom: navigation.zoom,
                },
            );
        });
        ui.ctx().request_repaint();
    }
    let pin_interaction_ids = std::mem::take(&mut viewer.pin_interaction_ids);
    crate::canvas_tools::preview_actions(ui, viewport, snarl);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            canvas_id.with("pin_interaction_ids"),
            PinInteractionIds(pin_interaction_ids),
        );
    });
    let hovered_node_this_frame = viewer.end_node_hover_frame();
    ui.ctx()
        .data_mut(|data| data.insert_temp(hover_marker, hovered_node_this_frame));
    if hovered_node_this_frame != hovered_node {
        // egui-snarl resolves wire colors from pins before it reports final
        // node rectangles, so a changed hover needs one immediate repaint.
        ui.ctx().request_repaint();
    }
    fit::end_frame(ui.ctx(), canvas_id, snarl);
    CanvasInteraction {
        viewport_width: viewport.width(),
        focused,
    }
}

enum Navigation {
    Zoom(f32),
    Fit,
}

fn zoom_step(current: f32, factor: f32) -> f32 {
    // An explicit fit of an exceptionally large graph may be below 5%.
    // A subsequent zoom-out must never jump in to the ordinary zoom floor.
    (current * factor).clamp(current.min(0.05), 2.0)
}

fn show_navigation(
    ui: &mut egui::Ui,
    id: egui::Id,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
) -> Option<Navigation> {
    let rect = egui::Rect::from_min_size(
        viewport.left_bottom()
            + egui::vec2(
                crate::ui_kit::space::MD,
                -crate::ui_kit::METRICS.control_height - crate::ui_kit::space::MD * 2.0,
            ),
        egui::vec2(240.0, crate::ui_kit::METRICS.control_height),
    );
    let layer = egui::LayerId::new(ui.layer_id().order, id.with("navigation_layer"));
    ui.ctx().set_sublayer(ui.layer_id(), layer);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id.with("navigation"))
            .layer_id(layer)
            .max_rect(rect),
    );
    child.set_clip_rect(viewport);
    let mut result = None;
    egui::Frame::popup(ui.style()).show(&mut child, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::ui_kit::space::XS;
            if crate::ui_kit::icon_button(ui, lucide_icons::Icon::Minus, "Zoom out", None).clicked()
            {
                result = Some(Navigation::Zoom(zoom_step(transform.scaling, 1.0 / 1.1)));
            }
            if ui
                .button(format!("{:.0}%", transform.scaling * 100.0))
                .on_hover_text("Reset zoom to 100%")
                .clicked()
            {
                result = Some(Navigation::Zoom(1.0));
            }
            if crate::ui_kit::icon_button(ui, lucide_icons::Icon::Plus, "Zoom in", None).clicked() {
                result = Some(Navigation::Zoom(zoom_step(transform.scaling, 1.1)));
            }
            if crate::ui_kit::icon_button(ui, lucide_icons::Icon::Maximize, "Fit all nodes", None)
                .clicked()
            {
                result = Some(Navigation::Fit);
            }
        });
    });
    result
}

#[cfg(test)]
pub(crate) fn current_fit_rects(
    context: &egui::Context,
    id_salt: egui::Id,
    view_generation: u64,
    snarl: &Snarl<CanvasNode>,
) -> Option<(egui::Rect, Vec<(SnarlNodeId, egui::Rect)>)> {
    let canvas_id = egui::Id::unique((CANVAS_ID, id_salt, view_generation));
    let transform =
        context.data(|data| data.get_temp::<CanvasTransform>(canvas_id.with("transform")))?;
    let (viewport, rects) = fit::measured_rects(context, canvas_id, snarl)?;
    Some((
        viewport,
        rects
            .into_iter()
            .map(|(id, rect)| (id, transform.0 * rect))
            .collect(),
    ))
}

#[cfg(test)]
pub fn current_pin_interaction_ids(
    context: &egui::Context,
    id_salt: egui::Id,
    view_generation: u64,
) -> Vec<egui::Id> {
    let canvas_id = egui::Id::unique((CANVAS_ID, id_salt, view_generation));
    context.data(|data| {
        data.get_temp::<PinInteractionIds>(canvas_id.with("pin_interaction_ids"))
            .unwrap_or_default()
            .0
    })
}

fn pin_drag_active(
    primary_down: bool,
    dragged_id: Option<egui::Id>,
    pin_interaction_ids: &[egui::Id],
) -> bool {
    primary_down && dragged_id.is_some_and(|dragged| pin_interaction_ids.contains(&dragged))
}

fn edge_pan_delta(
    viewport: egui::Rect,
    pointer: Option<egui::Pos2>,
    dragging: bool,
    frame_seconds: f32,
) -> egui::Vec2 {
    if !dragging || !viewport.is_positive() || frame_seconds <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let Some(pointer) = pointer else {
        return egui::Vec2::ZERO;
    };
    let pressure = |position: f32, minimum: f32, maximum: f32| {
        let toward_minimum = ((minimum + EDGE_PAN_ZONE - position) / EDGE_PAN_ZONE).clamp(0.0, 1.0);
        let toward_maximum =
            ((position - (maximum - EDGE_PAN_ZONE)) / EDGE_PAN_ZONE).clamp(0.0, 1.0);
        toward_minimum - toward_maximum
    };
    egui::vec2(
        pressure(pointer.x, viewport.left(), viewport.right()),
        pressure(pointer.y, viewport.top(), viewport.bottom()),
    ) * (EDGE_PAN_SPEED * frame_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precise_zoom_steps_do_not_jump_after_a_very_large_explicit_fit() {
        assert!((zoom_step(1.0, 1.1) - 1.1).abs() < f32::EPSILON);
        assert_eq!(zoom_step(0.01, 1.0 / 1.1), 0.01);
        assert!((zoom_step(0.01, 1.1) - 0.011).abs() < 0.000_001);
        assert_eq!(zoom_step(2.0, 1.1), 2.0);
    }

    #[test]
    fn edge_pan_uses_distance_pressure_only_during_active_drags() {
        let viewport = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(900.0, 650.0));

        assert_eq!(
            edge_pan_delta(viewport, Some(viewport.center()), true, 1.0 / 60.0),
            egui::Vec2::ZERO
        );
        assert_eq!(
            edge_pan_delta(viewport, Some(egui::pos2(100.0, 350.0)), false, 1.0 / 60.0),
            egui::Vec2::ZERO
        );
        let lower_left = edge_pan_delta(viewport, Some(egui::pos2(100.0, 650.0)), true, 1.0 / 60.0);
        assert!((lower_left - egui::vec2(15.0, -15.0)).length() < 0.001);
        let upper_right = edge_pan_delta(viewport, Some(egui::pos2(950.0, 20.0)), true, 1.0 / 60.0);
        assert!((upper_right - egui::vec2(-15.0, 15.0)).length() < 0.001);
    }

    #[test]
    fn edge_pan_activation_accepts_only_recorded_pin_drags() {
        let pin = egui::Id::unique("pin");
        let node = egui::Id::unique("node");

        assert!(pin_drag_active(true, Some(pin), &[pin]));
        assert!(!pin_drag_active(true, Some(node), &[pin]));
        assert!(!pin_drag_active(false, Some(pin), &[pin]));
        assert!(!pin_drag_active(true, None, &[pin]));
    }
}
