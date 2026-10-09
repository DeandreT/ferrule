//! Fit the camera to measured node frames without moving graph nodes.

use std::collections::BTreeMap;

use egui::{Context, Id, Pos2, Rect, Ui};
use egui_snarl::{NodeId, Snarl};

use crate::canvas::CanvasNode;

const SCREEN_MARGIN: f32 = 12.0;

#[derive(Clone, Copy)]
struct MeasuredNode {
    value: CanvasNode,
    position: Pos2,
    rect: Rect,
}

#[derive(Clone, Default)]
struct Measurements(BTreeMap<NodeId, MeasuredNode>);

#[derive(Clone, Copy)]
struct FitState {
    viewport: Rect,
    last_frame: u64,
    pending: bool,
    applied_bounds: Option<Rect>,
    minimum_zoom: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FittedView {
    pub graph_center: Pos2,
    pub screen_center: Pos2,
    pub zoom: f32,
}

pub(crate) fn record_node_rect(ui: &Ui, node: NodeId, rect: Rect, snarl: &Snarl<CanvasNode>) {
    let Some(info) = snarl.get_node_info(node) else {
        return;
    };
    let measured = MeasuredNode {
        value: snarl[node],
        position: info.pos,
        rect,
    };
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Measurements>(ui.layer_id().id.with("fit_node_rects"))
            .0
            .insert(node, measured);
    });
}

fn rectangles(
    context: &Context,
    canvas: Id,
    snarl: &Snarl<CanvasNode>,
) -> Option<Vec<(NodeId, Rect)>> {
    let measured = context.data(|data| {
        data.get_temp::<Measurements>(canvas.with("fit_node_rects"))
            .unwrap_or_default()
    });
    snarl
        .nodes_pos_ids()
        .map(|(id, position, value)| {
            let node = measured.0.get(&id)?;
            if node.value != *value || !position.is_finite() || !node.position.is_finite() {
                return None;
            }
            let rect = node.rect.translate(position - node.position);
            (rect.is_finite() && rect.is_positive()).then_some((id, rect))
        })
        .collect()
}

fn bounds(context: &Context, canvas: Id, snarl: &Snarl<CanvasNode>) -> Option<Rect> {
    let bounds = rectangles(context, canvas, snarl)?
        .into_iter()
        .fold(Rect::NOTHING, |bounds, (_, rect)| bounds.union(rect));
    (bounds.is_finite() && bounds.is_positive()).then_some(bounds)
}

fn fitted_view(bounds: Rect, viewport: Rect, maximum_zoom: f32) -> Option<FittedView> {
    if !bounds.is_finite()
        || !bounds.is_positive()
        || !viewport.is_finite()
        || !viewport.is_positive()
        || !maximum_zoom.is_finite()
        || maximum_zoom <= 0.0
    {
        return None;
    }
    let available = viewport.shrink(SCREEN_MARGIN);
    if !available.is_positive() {
        return None;
    }
    let zoom = (available.width() / bounds.width())
        .min(available.height() / bounds.height())
        .min(maximum_zoom);
    let graph_center = bounds.center();
    let screen_center = available.center();
    let translation = screen_center.to_vec2() - graph_center.to_vec2() * zoom;
    if !zoom.is_finite() || zoom <= 0.0 || !translation.is_finite() {
        return None;
    }
    let inverse = egui::emath::TSTransform {
        scaling: zoom,
        translation,
    }
    .inverse();
    if !inverse.scaling.is_finite()
        || !inverse.translation.is_finite()
        || !(inverse * viewport).is_finite()
    {
        return None;
    }
    Some(FittedView {
        graph_center,
        screen_center,
        zoom,
    })
}

pub(super) fn begin_frame(
    context: &Context,
    canvas: Id,
    viewport: Rect,
    snarl: &Snarl<CanvasNode>,
    maximum_zoom: f32,
    navigation: bool,
) -> (Option<FittedView>, Option<f32>) {
    let frame = context.cumulative_frame_nr();
    let mut state = context
        .data(|data| data.get_temp::<FitState>(canvas.with("measured_fit")))
        .unwrap_or(FitState {
            viewport,
            last_frame: frame,
            pending: true,
            applied_bounds: None,
            minimum_zoom: None,
        });
    state.pending |= viewport != state.viewport || frame > state.last_frame.saturating_add(1);
    state.viewport = viewport;
    state.last_frame = frame;
    if navigation {
        state.pending = false;
    }
    let view = if state.pending {
        state.applied_bounds = bounds(context, canvas, snarl);
        state
            .applied_bounds
            .and_then(|bounds| fitted_view(bounds, viewport, maximum_zoom))
    } else {
        None
    };
    if let Some(view) = view {
        // Keep a large graph fitted on the next frame too: Snarl otherwise
        // clamps its stored transform back to its default minimum zoom.
        state.minimum_zoom = Some(view.zoom);
    }
    context.data_mut(|data| {
        data.insert_temp(canvas.with("measured_fit"), state);
        // Only this frame's live node measurements are retained.
        data.insert_temp(canvas.with("fit_node_rects"), Measurements::default());
    });
    (view, state.minimum_zoom)
}

pub(super) fn end_frame(context: &Context, canvas: Id, snarl: &Snarl<CanvasNode>) {
    let current = bounds(context, canvas, snarl);
    let frame = context.cumulative_frame_nr();
    let repaint = context.data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(canvas.with("measured_fit"), || FitState {
            viewport: Rect::NOTHING,
            last_frame: frame,
            pending: false,
            applied_bounds: None,
            minimum_zoom: None,
        });
        if state.pending {
            // First measure, then fit; another measurement pass is needed only
            // when a node's actual frame changed while the fit was settling.
            state.pending = current.is_some() && !same_bounds(state.applied_bounds, current);
        }
        state.pending
    });
    if repaint {
        context.request_repaint();
    }
}

fn same_bounds(left: Option<Rect>, right: Option<Rect>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            (left.min - right.min).abs().max_elem() <= 0.5
                && (left.max - right.max).abs().max_elem() <= 0.5
        }
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
pub(super) fn measured_rects(
    context: &Context,
    canvas: Id,
    snarl: &Snarl<CanvasNode>,
) -> Option<(Rect, Vec<(NodeId, Rect)>)> {
    let viewport = context
        .data(|data| data.get_temp::<FitState>(canvas.with("measured_fit")))?
        .viewport;
    Some((viewport, rectangles(context, canvas, snarl)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(view: FittedView) -> egui::emath::TSTransform {
        egui::emath::TSTransform {
            scaling: view.zoom,
            translation: view.screen_center.to_vec2() - view.graph_center.to_vec2() * view.zoom,
        }
    }

    #[test]
    fn fit_complete_wide_and_tall_frames_with_screen_margin() {
        let viewport = Rect::from_min_max(egui::pos2(20.0, 40.0), egui::pos2(880.0, 660.0));
        let left = Rect::from_min_max(egui::pos2(-6.0, -6.0), egui::pos2(306.0, 126.0));
        let right = Rect::from_min_max(egui::pos2(794.0, 294.0), egui::pos2(1206.0, 946.0));
        let view = fitted_view(left.union(right), viewport, 2.0).expect("finite frame fit");
        eprintln!("fit original frames={left:?}/{right:?}, viewport={viewport:?}, view={view:?}");
        assert_eq!(view.graph_center, egui::pos2(600.0, 470.0));
        assert_eq!(view.screen_center, egui::pos2(450.0, 350.0));
        assert!((view.zoom - 596.0 / 952.0).abs() < 0.000_001);
        let available = viewport.shrink(12.0).expand(0.001);
        assert!(available.contains_rect(transform(view) * left));
        assert!(available.contains_rect(transform(view) * right));
    }

    #[test]
    fn fit_single_and_large_frames_without_a_clipping_zoom_floor() {
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
        let single = Rect::from_min_max(egui::pos2(194.0, 94.0), egui::pos2(506.0, 226.0));
        let view = fitted_view(single, viewport, 2.0).expect("single node");
        assert_eq!(view.zoom, 2.0);
        assert_eq!(view.graph_center, egui::pos2(350.0, 160.0));
        let large = Rect::from_min_size(Pos2::ZERO, egui::vec2(10_000.0, 1_000.0));
        let view = fitted_view(large, viewport, 2.0).expect("large finite graph");
        assert!((view.zoom - 0.0876).abs() < 0.000_001);
        assert!(
            viewport
                .shrink(12.0)
                .expand(0.001)
                .contains_rect(transform(view) * large)
        );
    }

    #[test]
    fn fit_refuses_empty_nonfinite_or_unusable_geometry() {
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
        let node = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 100.0));
        for invalid in [
            Rect::NOTHING,
            Rect::NAN,
            Rect::from_min_size(Pos2::ZERO, egui::Vec2::ZERO),
        ] {
            assert!(fitted_view(invalid, viewport, 2.0).is_none());
            assert!(fitted_view(node, invalid, 2.0).is_none());
        }
        assert!(
            fitted_view(
                node,
                Rect::from_min_size(Pos2::ZERO, egui::vec2(20.0, 20.0)),
                2.0
            )
            .is_none()
        );
        for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(fitted_view(node, viewport, zoom).is_none());
        }
    }

    #[test]
    fn fit_first_resize_and_return_requests_preserve_explicit_navigation() {
        let context = Context::default();
        let canvas = Id::new("fit-lifecycle");
        let mut snarl = Snarl::new();
        let node = snarl.insert_node(Pos2::ZERO, CanvasNode::Graph(7));
        let measured = MeasuredNode {
            value: CanvasNode::Graph(7),
            position: Pos2::ZERO,
            rect: Rect::from_min_size(egui::pos2(-6.0, -6.0), egui::vec2(312.0, 132.0)),
        };
        let record = || {
            context.data_mut(|data| {
                data.insert_temp(
                    canvas.with("fit_node_rects"),
                    Measurements(BTreeMap::from([(node, measured)])),
                )
            })
        };
        let wide = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let narrow = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
        assert!(
            begin_frame(&context, canvas, wide, &snarl, 2.0, false)
                .0
                .is_none()
        );
        record();
        end_frame(&context, canvas, &snarl);
        assert!(
            begin_frame(&context, canvas, wide, &snarl, 2.0, false)
                .0
                .is_some()
        );
        record();
        end_frame(&context, canvas, &snarl);
        assert!(
            begin_frame(&context, canvas, wide, &snarl, 2.0, false)
                .0
                .is_none()
        );
        record();
        assert!(
            begin_frame(&context, canvas, narrow, &snarl, 2.0, true)
                .0
                .is_none()
        );
        record();
        end_frame(&context, canvas, &snarl);
        assert!(
            begin_frame(&context, canvas, narrow, &snarl, 2.0, false)
                .0
                .is_none()
        );
        record();
        assert!(
            begin_frame(&context, canvas, wide, &snarl, 2.0, false)
                .0
                .is_some()
        );
        record();
        end_frame(&context, canvas, &snarl);
        for _ in 0..3 {
            let _ = context.run_ui(Default::default(), |_| {});
        }
        record();
        assert!(
            begin_frame(&context, canvas, wide, &snarl, 2.0, false)
                .0
                .is_some()
        );
    }

    #[test]
    fn fit_measurements_require_each_live_identity_and_follow_its_position() {
        let context = Context::default();
        let canvas = Id::new("fit-measurements");
        let mut snarl = Snarl::new();
        let node = snarl.insert_node(egui::pos2(200.0, 100.0), CanvasNode::Graph(7));
        let measured = MeasuredNode {
            value: CanvasNode::Graph(7),
            position: egui::pos2(200.0, 100.0),
            rect: Rect::from_min_max(egui::pos2(194.0, 94.0), egui::pos2(506.0, 226.0)),
        };
        assert!(bounds(&context, canvas, &snarl).is_none());
        context.data_mut(|data| {
            data.insert_temp(
                canvas.with("fit_node_rects"),
                Measurements(BTreeMap::from([(node, measured)])),
            )
        });
        let before = format!("{snarl:#?}");
        assert_eq!(bounds(&context, canvas, &snarl), Some(measured.rect));
        assert_eq!(format!("{snarl:#?}"), before);
        snarl.get_node_info_mut(node).expect("live node").pos += egui::vec2(50.0, 75.0);
        assert_eq!(
            bounds(&context, canvas, &snarl),
            Some(measured.rect.translate(egui::vec2(50.0, 75.0)))
        );
        snarl[node] = CanvasNode::Graph(8);
        assert!(bounds(&context, canvas, &snarl).is_none());
        snarl.remove_node(node);
        assert!(bounds(&context, canvas, &snarl).is_none());
    }

    #[test]
    fn fit_refuses_finite_coefficients_with_an_overflowing_inverse_viewport() {
        let viewport = Rect::from_min_max(Pos2::ZERO, egui::pos2(900.0, 700.0));
        let bounds = Rect::from_min_max(Pos2::ZERO, egui::pos2(f32::MAX, 1_000.0));
        let unguarded = FittedView {
            graph_center: egui::pos2(f32::MAX / 2.0, 500.0),
            screen_center: egui::pos2(450.0, 350.0),
            zoom: 876.0 / f32::MAX,
        };
        let forward = transform(unguarded);
        let inverse = forward.inverse();
        let inverse_viewport = inverse * viewport;
        let actual = fitted_view(bounds, viewport, 2.0);
        eprintln!(
            "fit inverse-viewport original bounds={bounds:?}, viewport={viewport:?}, view={unguarded:?}, transform={forward:?}, inverse={inverse:?}, inverse_viewport={inverse_viewport:?}, actual={actual:?}"
        );
        assert!(forward.scaling.is_finite() && forward.scaling > 0.0);
        assert!(forward.translation.is_finite());
        assert!(inverse.scaling.is_finite() && inverse.translation.is_finite());
        assert!(!inverse_viewport.is_finite());
        assert!(actual.is_none());
    }
}
