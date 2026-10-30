//! Keyboard operation of the mapping canvas.
//!
//! egui-snarl draws node frames, pins and collapse buttons as focusable but
//! unnamed widgets that react to the pointer only. This module names them,
//! maps each focus stop to the node that owns it, and gives Enter/Space the
//! pointer's meaning: select a node, collapse it, or draw a wire.
//!
//! A keyboard wire starts with Enter on one pin and completes with Enter on a
//! pin of the opposite direction; it connects through
//! [`SnarlViewer::connect`], exactly like a dropped pointer drag. Escape
//! cancels it and returns focus to the pin it started from.
//!
//! Arrow keys follow the graph layout instead of egui's spatial focus search,
//! which compares the canvas's graph-space widget rectangles with screen-space
//! controls. On a node frame, collapse or Edit button they move to the nearest
//! node frame in that direction; on a pin, Up and Down step through the pins
//! of its side and Left and Right cross to the nearest pin of the other side.
//! Stepping from a visible endpoint field onto a hidden-fields pin scrolls the
//! list by one row instead, so every field stays reachable.

use egui_snarl::ui::SnarlViewer as _;
use egui_snarl::{InPinId, NodeId as SnarlNodeId, OutPinId, Snarl};

use crate::canvas::CanvasNode;
use crate::graph_viewer::GraphViewer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanvasPin {
    Input(InPinId),
    Output(OutPinId),
}

impl CanvasPin {
    pub(crate) const fn node(self) -> SnarlNodeId {
        match self {
            Self::Input(pin) => pin.node,
            Self::Output(pin) => pin.node,
        }
    }

    /// The pin's position in its side of the node, top to bottom.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Input(pin) => pin.input,
            Self::Output(pin) => pin.output,
        }
    }

    /// The pin at `index` on the same side of the same node.
    pub(crate) const fn with_index(self, index: usize) -> Self {
        match self {
            Self::Input(pin) => Self::Input(InPinId {
                node: pin.node,
                input: index,
            }),
            Self::Output(pin) => Self::Output(OutPinId {
                node: pin.node,
                output: index,
            }),
        }
    }

    const fn is_input(self) -> bool {
        matches!(self, Self::Input(_))
    }
}

/// Pin widgets of the pass being drawn.
#[derive(Clone, Debug, Default)]
struct PinWidgets(Vec<PinWidget>);

/// A pin widget recorded while egui-snarl draws the graph.
#[derive(Clone, Debug)]
pub(crate) struct PinWidget {
    /// egui-snarl's interaction ID for the pin.
    pub id: egui::Id,
    pub pin: CanvasPin,
    /// Accessible name, such as "Input Name · Target: Order".
    pub name: String,
}

/// Focusable node controls of one complete pass and the node owning each.
#[derive(Clone, Default)]
struct NodeControls(Vec<(egui::Id, SnarlNodeId)>);

/// The start of a keyboard wire.
#[derive(Clone, Copy)]
struct PendingWire {
    pin: CanvasPin,
    widget: egui::Id,
}

fn controls_id(canvas_id: egui::Id) -> egui::Id {
    canvas_id.with("keyboard_node_controls")
}

fn next_controls_id(canvas_id: egui::Id) -> egui::Id {
    canvas_id.with("keyboard_node_controls_next")
}

fn surface_id(canvas_id: egui::Id) -> egui::Id {
    canvas_id.with("keyboard_surface")
}

fn pins_id(canvas_id: egui::Id) -> egui::Id {
    canvas_id.with("keyboard_pins")
}

fn wire_id(canvas_id: egui::Id) -> egui::Id {
    canvas_id.with("keyboard_wire")
}

fn pin_focus_id(canvas_id: egui::Id) -> egui::Id {
    canvas_id.with("keyboard_pin_focus")
}

/// The frame egui-snarl draws for `node` on the snarl layer `layer`.
pub(crate) fn frame_id(layer: egui::Id, node: SnarlNodeId) -> egui::Id {
    layer.with(("snarl-node", node)).with("frame")
}

const ARROWS: [egui::Key; 4] = [
    egui::Key::ArrowUp,
    egui::Key::ArrowDown,
    egui::Key::ArrowLeft,
    egui::Key::ArrowRight,
];

/// Takes the first of `keys` pressed without modifiers for the focused canvas
/// control `id`, and keeps egui's spatial arrow focus away from it whether or
/// not an arrow is among `keys`.
pub(crate) fn take_key(ctx: &egui::Context, id: egui::Id, keys: &[egui::Key]) -> Option<egui::Key> {
    // The lock applies from the next pass, as a text field's does. Until then
    // an arrow pressed this pass cancels the search egui started for it.
    ctx.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        );
    });
    let (arrow, tab) = ctx.input(|input| {
        (
            ARROWS.iter().any(|key| input.key_pressed(*key)),
            input.key_pressed(egui::Key::Tab),
        )
    });
    if arrow && !tab {
        ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    }
    ctx.input_mut(|input| {
        keys.iter()
            .copied()
            .find(|key| input.consume_key(egui::Modifiers::NONE, *key))
    })
}

/// Activation by Enter, Space or an assistive-technology click, which
/// egui-snarl ignores because it reacts to primary pointer clicks only.
pub(crate) fn keyboard_activated(response: &egui::Response) -> bool {
    response.clicked() && !response.clicked_by(egui::PointerButton::Primary)
}

/// Names a focus stop drawn by egui-snarl.
pub(crate) fn name(response: &egui::Response, label: &str) {
    name_as(response, egui::Role::Button, label);
}

/// Names a canvas focus stop with the given role.
pub(crate) fn name_as(response: &egui::Response, role: egui::Role, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(role, response.enabled(), label));
}

/// Records a focusable control drawn on the snarl layer of `ui` for `node`.
pub(crate) fn record_control(ui: &egui::Ui, id: egui::Id, node: SnarlNodeId) {
    record_control_at(ui.ctx(), ui.layer_id().id, id, node);
}

/// Records a pin drawn on the snarl layer of `ui`.
pub(crate) fn record_pin(ui: &egui::Ui, pin: PinWidget) {
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<PinWidgets>(pins_id(ui.layer_id().id))
            .0
            .push(pin);
    });
}

/// The ID egui gives the first widget allocated in `ui`. egui-snarl allocates
/// a node's collapse button first in the header row it passes to
/// `show_header`.
pub(crate) fn first_widget_id(ui: &egui::Ui) -> egui::Id {
    egui::Id::unique(ui.unique_id().value().wrapping_add(1))
}

/// Paints keyboard focus around a canvas control, in graph coordinates.
pub(crate) fn paint_focus(ui: &egui::Ui, rect: egui::Rect, rounding: f32) {
    ui.painter().rect_stroke(
        rect,
        rounding,
        ui.visuals().selection.stroke,
        egui::StrokeKind::Outside,
    );
}

fn canvas_layers(canvas_id: egui::Id) -> [egui::Id; 4] {
    [
        canvas_id,
        canvas_id.with("navigation_layer"),
        canvas_id.with("minimap_layer"),
        canvas_id.with("field_search"),
    ]
}

/// What canvas shortcuts (F2, Delete, Ctrl+F) may act on this pass.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Shortcuts {
    /// No control or a canvas control has keyboard focus; never a text field.
    pub enabled: bool,
    /// The node owning the focused frame, pin, collapse or Edit button.
    pub node: Option<SnarlNodeId>,
}

pub(crate) fn shortcuts(ctx: &egui::Context, canvas_id: egui::Id) -> Shortcuts {
    let Some(focused) = ctx.memory(|memory| memory.focused()) else {
        return Shortcuts {
            enabled: true,
            node: None,
        };
    };
    if ctx.text_edit_focused() {
        return Shortcuts::default();
    }
    let node = ctx.data(|data| {
        data.get_temp::<NodeControls>(controls_id(canvas_id))
            .and_then(|controls| {
                controls
                    .0
                    .iter()
                    .find(|(id, _)| *id == focused)
                    .map(|(_, node)| *node)
            })
    });
    let on_canvas = node.is_some() || has_keyboard_focus(ctx, canvas_id);
    Shortcuts {
        enabled: on_canvas,
        node,
    }
}

/// Whether a control of this canvas holds keyboard focus.
pub(crate) fn has_keyboard_focus(ctx: &egui::Context, canvas_id: egui::Id) -> bool {
    ctx.memory(|memory| memory.focused())
        .and_then(|id| ctx.read_response(id))
        .is_some_and(|response| canvas_layers(canvas_id).contains(&response.layer_id.id))
}

/// Starts a pass: node controls are recorded afresh while the graph is drawn.
///
/// While Shift is held, egui-snarl registers a focusable rectangle-selection
/// target in front of the first node. Shift+Tab from that node lands on it,
/// and it is gone once Shift is released, so focus moves on to the canvas
/// surface instead of being dropped.
pub(crate) fn begin(ctx: &egui::Context, canvas_id: egui::Id) {
    if ctx.memory(|memory| memory.focused()) == Some(canvas_id.with("select"))
        && let Some(surface) = ctx.data(|data| data.get_temp::<egui::Id>(surface_id(canvas_id)))
    {
        ctx.memory_mut(|memory| memory.request_focus(surface));
    }
    ctx.data_mut(|data| {
        data.remove::<NodeControls>(next_controls_id(canvas_id));
        data.remove::<PinWidgets>(pins_id(canvas_id));
    });
}

/// Cancels a keyboard wire on Escape. Runs before the canvas tools, which
/// would otherwise take Escape to clear the connection focus.
pub(crate) fn cancel_wire_on_escape(ctx: &egui::Context, canvas_id: egui::Id) {
    let Some(pending) = ctx.data(|data| data.get_temp::<PendingWire>(wire_id(canvas_id))) else {
        return;
    };
    if egui::Popup::is_any_open(ctx)
        || crate::ui_kit::modal_open(ctx)
        || crate::canvas_tools::picker_open(ctx)
        || GraphViewer::properties_open(ctx)
    {
        return;
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ctx.data_mut(|data| data.remove::<PendingWire>(wire_id(canvas_id)));
        // egui drops focus on Escape before any widget runs.
        ctx.memory_mut(|memory| memory.request_focus(pending.widget));
        ctx.request_repaint();
    }
}

/// Names the canvas surface and pins, handles Enter on pins, and paints
/// keyboard focus. Runs after egui-snarl has drawn every node. Returns the
/// graph position of a newly focused control outside `viewport`, which the
/// canvas pans to.
pub(crate) fn finish(
    ui: &egui::Ui,
    canvas_id: egui::Id,
    surface: &egui::Response,
    viewport: egui::Rect,
    viewer: &mut GraphViewer<'_>,
    snarl: &mut Snarl<CanvasNode>,
) -> Option<egui::Pos2> {
    let ctx = ui.ctx();
    surface.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, "Mapping canvas"));
    ctx.data_mut(|data| data.insert_temp(surface_id(canvas_id), surface.id));
    let layer = egui::LayerId::new(ui.layer_id().order, canvas_id);
    let painter = ctx.layer_painter(layer).with_clip_rect(
        viewer
            .canvas_transform
            .map_or(egui::Rect::NOTHING, |transform| {
                transform.inverse() * viewport
            }),
    );
    let pins = ctx
        .data_mut(|data| data.remove_temp::<PinWidgets>(pins_id(canvas_id)))
        .unwrap_or_default()
        .0;
    let mut pending = ctx.data(|data| data.get_temp::<PendingWire>(wire_id(canvas_id)));
    if pending.is_some_and(|wire| !pins.iter().any(|widget| widget.id == wire.widget)) {
        pending = None;
    }
    let mut activated = None;
    for widget in &pins {
        let Some(response) = ctx.read_response(widget.id) else {
            continue;
        };
        name(&response, &widget.name);
        record_control_at(ctx, canvas_id, widget.id, widget.pin.node());
        let radius = response.rect.width().min(response.rect.height()) * 0.5;
        if response.has_focus() {
            painter.circle_stroke(
                response.rect.center(),
                radius + 2.0,
                ui.visuals().selection.stroke,
            );
        }
        if pending.is_some_and(|wire| wire.widget == widget.id) {
            painter.circle_stroke(
                response.rect.center(),
                radius + 4.0,
                egui::Stroke::new(2.0, ui.visuals().warn_fg_color),
            );
        }
        if keyboard_activated(&response) {
            activated = Some(widget);
        }
    }
    if let Some(widget) = activated {
        pending = match (pending.map(|wire| wire.pin), widget.pin) {
            (Some(CanvasPin::Output(from)), CanvasPin::Input(to))
            | (Some(CanvasPin::Input(to)), CanvasPin::Output(from)) => {
                viewer.connect(&snarl.out_pin(from), &snarl.in_pin(to), snarl);
                None
            }
            (Some(start), pin) if start == pin => None,
            (_, pin) => Some(PendingWire {
                pin,
                widget: widget.id,
            }),
        };
        ctx.request_repaint();
    }
    ctx.data_mut(|data| match pending {
        Some(wire) => {
            data.insert_temp(wire_id(canvas_id), wire);
        }
        None => data.remove::<PendingWire>(wire_id(canvas_id)),
    });
    let controls = ctx.data_mut(|data| {
        data.remove_temp::<NodeControls>(next_controls_id(canvas_id))
            .unwrap_or_default()
    });
    let moved = take_pending_pin(ctx, canvas_id, &pins)
        .or_else(|| navigate(ctx, canvas_id, surface.id, &pins, &controls, viewer, snarl));
    if let Some(id) = moved {
        ctx.memory_mut(|memory| memory.request_focus(id));
        ctx.request_repaint();
    }
    let reveal = ctx
        .memory(|memory| memory.focused())
        .filter(|focused| controls.0.iter().any(|(id, _)| id == focused))
        .and_then(|focused| ctx.read_response(focused))
        // A control focused during this pass reports no gained focus.
        .filter(|response| response.gained_focus() || moved == Some(response.id))
        .and_then(|response| {
            let transform = viewer.canvas_transform?;
            (!viewport.contains_rect(transform * response.rect)).then(|| response.rect.center())
        });
    ctx.data_mut(|data| data.insert_temp(controls_id(canvas_id), controls));
    reveal
}

fn record_control_at(ctx: &egui::Context, canvas_id: egui::Id, id: egui::Id, node: SnarlNodeId) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<NodeControls>(next_controls_id(canvas_id))
            .0
            .push((id, node));
    });
}

/// A pin an arrow key scrolled into view, focused once it has been drawn.
#[derive(Clone, Copy)]
struct PendingPinFocus(CanvasPin);

fn take_pending_pin(
    ctx: &egui::Context,
    canvas_id: egui::Id,
    pins: &[PinWidget],
) -> Option<egui::Id> {
    let PendingPinFocus(pin) = ctx.data_mut(|data| {
        let pending = data.get_temp::<PendingPinFocus>(pin_focus_id(canvas_id));
        data.remove::<PendingPinFocus>(pin_focus_id(canvas_id));
        pending
    })?;
    pins.iter()
        .find(|widget| widget.pin == pin)
        .map(|widget| widget.id)
}

/// Handles an arrow key on the canvas surface, a node control or a pin, and
/// returns the control to focus. The surface takes arrows without moving.
fn navigate(
    ctx: &egui::Context,
    canvas_id: egui::Id,
    surface: egui::Id,
    pins: &[PinWidget],
    controls: &NodeControls,
    viewer: &mut GraphViewer<'_>,
    snarl: &mut Snarl<CanvasNode>,
) -> Option<egui::Id> {
    let focused = ctx.memory(|memory| memory.focused())?;
    if ctx.text_edit_focused()
        || egui::Popup::is_any_open(ctx)
        || crate::ui_kit::modal_open(ctx)
        || crate::canvas_tools::picker_open(ctx)
        || GraphViewer::properties_open(ctx)
    {
        return None;
    }
    let pin = pins.iter().find(|widget| widget.id == focused);
    let node = controls
        .0
        .iter()
        .find(|(id, _)| *id == focused)
        .map(|(_, node)| *node);
    if pin.is_none() && node.is_none() && focused != surface {
        return None;
    }
    let key = take_key(ctx, focused, &ARROWS)?;
    if let Some(widget) = pin {
        return pin_target(ctx, canvas_id, widget, key, pins, viewer, snarl);
    }
    let from = node?;
    let rect = |node| {
        ctx.read_response(frame_id(canvas_id, node))
            .map(|response| response.rect)
    };
    let origin = rect(from)?;
    let candidates = snarl
        .node_ids()
        .filter(|(node, _)| *node != from)
        .filter_map(|(node, _)| Some((node, rect(node)?)));
    nearest_in_direction(origin, candidates, arrow_direction(key)?)
        .map(|node| frame_id(canvas_id, node))
}

fn arrow_direction(key: egui::Key) -> Option<egui::Vec2> {
    match key {
        egui::Key::ArrowUp => Some(egui::Vec2::UP),
        egui::Key::ArrowDown => Some(egui::Vec2::DOWN),
        egui::Key::ArrowLeft => Some(egui::Vec2::LEFT),
        egui::Key::ArrowRight => Some(egui::Vec2::RIGHT),
        _ => None,
    }
}

/// The pin an arrow key moves to from `widget`. Up and Down keep to the
/// pin's side of its node; inputs are drawn on the left and outputs on the
/// right, so Right crosses from an input and Left from an output.
fn pin_target(
    ctx: &egui::Context,
    canvas_id: egui::Id,
    widget: &PinWidget,
    key: egui::Key,
    pins: &[PinWidget],
    viewer: &mut GraphViewer<'_>,
    snarl: &mut Snarl<CanvasNode>,
) -> Option<egui::Id> {
    let node = widget.pin.node();
    let input = widget.pin.is_input();
    let index = widget.pin.index();
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let step = if key == egui::Key::ArrowUp { -1 } else { 1 };
            if let Some(revealed) = viewer.scroll_endpoint_field(widget.pin, step, snarl) {
                ctx.data_mut(|data| {
                    data.insert_temp(pin_focus_id(canvas_id), PendingPinFocus(revealed));
                });
                ctx.request_repaint();
                return None;
            }
            let side = pins
                .iter()
                .filter(|other| other.pin.node() == node && other.pin.is_input() == input);
            if step < 0 {
                side.filter(|other| other.pin.index() < index)
                    .max_by_key(|other| other.pin.index())
            } else {
                side.filter(|other| other.pin.index() > index)
                    .min_by_key(|other| other.pin.index())
            }
            .map(|other| other.id)
        }
        egui::Key::ArrowLeft | egui::Key::ArrowRight => {
            if input != (key == egui::Key::ArrowRight) {
                return None;
            }
            let height = |id| {
                ctx.read_response(id)
                    .map(|response| response.rect.center().y)
            };
            let from = height(widget.id)?;
            pins.iter()
                .filter(|other| other.pin.node() == node && other.pin.is_input() != input)
                .filter_map(|other| Some((other, (height(other.id)? - from).abs())))
                .min_by(|(a, a_distance), (b, b_distance)| {
                    a_distance
                        .total_cmp(b_distance)
                        .then(a.pin.index().cmp(&b.pin.index()))
                })
                .map(|(other, _)| other.id)
        }
        _ => None,
    }
}

/// The candidate nearest ahead of `origin` in `direction`, within 45° of it.
/// Offsets are measured between rectangle centres, except that ranges
/// overlapping by at least half the shorter one count as aligned, so a tall
/// endpoint node is level with every node beside it.
fn nearest_in_direction<T>(
    origin: egui::Rect,
    candidates: impl IntoIterator<Item = (T, egui::Rect)>,
    direction: egui::Vec2,
) -> Option<T> {
    fn gap(candidate: egui::Rangef, origin: egui::Rangef) -> f32 {
        let overlap = candidate.intersection(origin).span();
        if overlap >= 0.5 * candidate.span().min(origin.span()) {
            0.0
        } else {
            candidate.center() - origin.center()
        }
    }
    candidates
        .into_iter()
        .filter_map(|(item, rect)| {
            let offset = egui::vec2(
                gap(rect.x_range(), origin.x_range()),
                gap(rect.y_range(), origin.y_range()),
            );
            let ahead = offset.dot(direction);
            let aside = (offset - direction * ahead).length();
            (ahead > 0.0 && aside <= ahead).then_some((item, ahead + 2.0 * aside))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(item, _)| item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
    }

    #[test]
    fn arrows_pick_the_nearest_node_in_their_direction() {
        let origin = rect(300.0, 100.0, 120.0, 60.0);
        let nodes = [
            ("tall source", rect(0.0, 0.0, 160.0, 600.0)),
            ("right", rect(600.0, 110.0, 120.0, 60.0)),
            ("far right", rect(900.0, 100.0, 120.0, 60.0)),
            ("below", rect(310.0, 300.0, 120.0, 60.0)),
            ("diagonal", rect(600.0, 400.0, 120.0, 60.0)),
        ];
        let nearest = |direction| nearest_in_direction(origin, nodes, direction);
        assert_eq!(nearest(egui::Vec2::RIGHT), Some("right"));
        assert_eq!(nearest(egui::Vec2::LEFT), Some("tall source"));
        assert_eq!(nearest(egui::Vec2::DOWN), Some("below"));
        assert_eq!(nearest(egui::Vec2::UP), None);
    }

    #[test]
    fn arrows_skip_nodes_outside_a_45_degree_cone() {
        let origin = rect(0.0, 0.0, 100.0, 50.0);
        let steep = [("steep", rect(150.0, 400.0, 100.0, 50.0))];
        assert_eq!(nearest_in_direction(origin, steep, egui::Vec2::RIGHT), None);
        assert_eq!(
            nearest_in_direction(origin, steep, egui::Vec2::DOWN),
            Some("steep")
        );
    }

    #[test]
    fn pins_report_their_side_and_position() {
        let node = SnarlNodeId(3);
        let input = CanvasPin::Input(InPinId { node, input: 2 });
        let output = CanvasPin::Output(OutPinId { node, output: 1 });
        assert!(input.is_input() && !output.is_input());
        assert_eq!((input.index(), output.index()), (2, 1));
        assert_eq!(
            input.with_index(5),
            CanvasPin::Input(InPinId { node, input: 5 })
        );
        assert_eq!(
            output.with_index(0),
            CanvasPin::Output(OutPinId { node, output: 0 })
        );
    }
}
