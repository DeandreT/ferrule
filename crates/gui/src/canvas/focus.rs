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
    const fn node(self) -> SnarlNodeId {
        match self {
            Self::Input(pin) => pin.node,
            Self::Output(pin) => pin.node,
        }
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

/// Activation by Enter, Space or an assistive-technology click, which
/// egui-snarl ignores because it reacts to primary pointer clicks only.
pub(crate) fn keyboard_activated(response: &egui::Response) -> bool {
    response.clicked() && !response.clicked_by(egui::PointerButton::Primary)
}

/// Names a focus stop drawn by egui-snarl.
pub(crate) fn name(response: &egui::Response, label: &str) {
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, response.enabled(), label));
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
    let reveal = ctx
        .memory(|memory| memory.focused())
        .filter(|focused| controls.0.iter().any(|(id, _)| id == focused))
        .and_then(|focused| ctx.read_response(focused))
        .filter(egui::Response::gained_focus)
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
