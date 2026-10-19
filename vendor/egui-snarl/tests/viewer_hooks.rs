//! Self-authored headless egui fixtures for the local viewer hooks.

use std::collections::BTreeMap;

use egui::{Color32, Context, Event, Modifiers, PointerButton, Pos2, Rect, Ui, Vec2, pos2, vec2};
use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer, SnarlWidget, WireStyle};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};

type Edge = (OutPinId, InPinId);

const SOURCE_COLOR: Color32 = Color32::from_rgb(200, 40, 80);
const TARGET_COLOR: Color32 = Color32::from_rgb(40, 120, 160);
const MIXED_COLOR: Color32 = Color32::from_rgb(120, 80, 120);
const FOCUSED_COLOR: Color32 = Color32::from_rgb(20, 220, 90);
const OTHER_COLOR: Color32 = Color32::from_rgb(100, 110, 130);

#[derive(Clone, Copy)]
enum Kind {
    Source,
    Target,
}

struct Geometry {
    header: Rect,
    inputs: Vec<(usize, Pos2)>,
    outputs: Vec<(usize, Pos2)>,
}

#[derive(Default)]
struct Probe {
    focus: Option<InPinId>,
    colors: BTreeMap<Edge, Color32>,
    geometry: BTreeMap<NodeId, Geometry>,
    controls: BTreeMap<NodeId, Rect>,
    control_clicks: usize,
    node_clicks: Vec<NodeId>,
    wire_clicks: Vec<Vec<Edge>>,
    disconnected: usize,
}

#[allow(refining_impl_trait)]
impl SnarlViewer<Kind> for Probe {
    fn title(&mut self, _node: &Kind) -> String {
        "fixture".into()
    }

    fn inputs(&mut self, node: &Kind) -> usize {
        usize::from(matches!(node, Kind::Target))
    }

    fn outputs(&mut self, node: &Kind) -> usize {
        usize::from(matches!(node, Kind::Source))
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _snarl: &mut Snarl<Kind>,
    ) {
        let response = ui.button("control");
        self.controls.insert(node, response.rect);
        if response.clicked() {
            self.control_clicks += 1;
        }
    }

    fn show_input(&mut self, _pin: &InPin, ui: &mut Ui, _snarl: &mut Snarl<Kind>) -> PinInfo {
        ui.label("input");
        PinInfo::circle().with_wire_color(TARGET_COLOR)
    }

    fn show_output(&mut self, _pin: &OutPin, ui: &mut Ui, _snarl: &mut Snarl<Kind>) -> PinInfo {
        ui.label("output");
        PinInfo::circle().with_wire_color(SOURCE_COLOR)
    }

    fn current_transform(
        &mut self,
        transform: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<Kind>,
    ) {
        // Keep graph and screen coordinates identical in these fixtures.
        *transform = egui::emath::TSTransform {
            scaling: 1.0,
            translation: Vec2::ZERO,
        };
    }

    fn wire_color(
        &mut self,
        from: OutPinId,
        to: InPinId,
        color: Color32,
        _ui: &Ui,
        _snarl: &Snarl<Kind>,
    ) -> Color32 {
        self.colors.insert((from, to), color);
        if Some(to) == self.focus {
            FOCUSED_COLOR
        } else {
            OTHER_COLOR
        }
    }

    fn wire_clicked(&mut self, edges: &[Edge], _pos: Pos2, _ui: &mut Ui, _snarl: &mut Snarl<Kind>) {
        self.wire_clicks.push(edges.to_vec());
    }

    fn node_clicked(&mut self, node: NodeId, _ui: &mut Ui, _snarl: &mut Snarl<Kind>) {
        self.node_clicks.push(node);
    }

    fn node_geometry(
        &mut self,
        node: NodeId,
        header: Rect,
        inputs: &[(usize, Pos2)],
        outputs: &[(usize, Pos2)],
        _ui: &mut Ui,
        _snarl: &mut Snarl<Kind>,
    ) {
        self.geometry.insert(
            node,
            Geometry {
                header,
                inputs: inputs.to_vec(),
                outputs: outputs.to_vec(),
            },
        );
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Kind>) {
        self.disconnected += 1;
        snarl.disconnect(from.id, to.id);
    }
}

fn frame(
    context: &Context,
    snarl: &mut Snarl<Kind>,
    probe: &mut Probe,
    events: Vec<Event>,
) -> egui::FullOutput {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
            events,
            ..Default::default()
        },
        |ui| {
            let style = SnarlStyle {
                wire_style: Some(WireStyle::Line),
                wire_width: Some(2.0),
                collapsible: Some(false),
                min_scale: Some(1.0),
                max_scale: Some(1.0),
                ..SnarlStyle::new()
            };
            SnarlWidget::new()
                .id(egui::Id::unique("vendored-hook-fixture"))
                .style(style)
                .show(snarl, probe, ui);
        },
    );
    // Interaction/shape checks do not upload textures to a renderer.
    output.textures_delta.clear();
    output
}

fn settle(context: &Context, snarl: &mut Snarl<Kind>, probe: &mut Probe) {
    for _ in 0..5 {
        let _ = frame(context, snarl, probe, Vec::new());
    }
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn click(
    context: &Context,
    snarl: &mut Snarl<Kind>,
    probe: &mut Probe,
    pos: Pos2,
    which: PointerButton,
) {
    let _ = frame(context, snarl, probe, vec![Event::PointerMoved(pos)]);
    let _ = frame(context, snarl, probe, vec![button(pos, which, true)]);
    let _ = frame(context, snarl, probe, vec![button(pos, which, false)]);
}

fn fixture(coincident: bool) -> (Context, Snarl<Kind>, Probe, [NodeId; 3]) {
    let context = Context::default();
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(pos2(100.0, 100.0), Kind::Source);
    let first = snarl.insert_node(pos2(500.0, 100.0), Kind::Target);
    let second = snarl.insert_node(
        pos2(500.0, if coincident { 100.0 } else { 260.0 }),
        Kind::Target,
    );
    let from = OutPinId {
        node: source,
        output: 0,
    };
    for node in [first, second] {
        snarl.connect(from, InPinId { node, input: 0 });
    }
    let mut probe = Probe {
        focus: Some(InPinId {
            node: first,
            input: 0,
        }),
        ..Probe::default()
    };
    settle(&context, &mut snarl, &mut probe);
    (context, snarl, probe, [source, first, second])
}

fn has_line_color(shape: &egui::Shape, color: Color32) -> bool {
    match shape {
        egui::Shape::LineSegment { stroke, .. } => stroke.color == color,
        egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_line_color(shape, color)),
        _ => false,
    }
}

#[test]
fn fanout_colors_are_overridden_per_edge_after_endpoint_mixing() {
    let (context, mut snarl, mut probe, _) = fixture(false);
    let output = frame(&context, &mut snarl, &mut probe, Vec::new());
    assert_eq!(probe.colors.len(), 2);
    assert!(probe.colors.values().all(|color| *color == MIXED_COLOR));
    for color in [FOCUSED_COLOR, OTHER_COLOR] {
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| has_line_color(&shape.shape, color))
        );
    }
    assert_eq!(snarl.wires().count(), 2);
}

#[test]
fn coincident_primary_hits_are_stable_and_secondary_disconnect_stays_native() {
    let (context, mut snarl, mut probe, [source, target, _]) = fixture(true);
    let from = probe.geometry[&source].outputs[0].1;
    let to = probe.geometry[&target].inputs[0].1;
    let pos = from.lerp(to, 0.5);
    click(
        &context,
        &mut snarl,
        &mut probe,
        pos,
        PointerButton::Primary,
    );
    assert_eq!(probe.wire_clicks.len(), 1);
    let mut expected = snarl.wires().collect::<Vec<_>>();
    expected.sort_unstable();
    assert_eq!(probe.wire_clicks[0], expected);
    assert_eq!(probe.disconnected, 0);
    assert_eq!(snarl.wires().count(), 2);

    click(
        &context,
        &mut snarl,
        &mut probe,
        pos,
        PointerButton::Secondary,
    );
    assert_eq!(probe.wire_clicks.len(), 1);
    assert_eq!(probe.disconnected, 1);
    assert_eq!(snarl.wires().count(), 1);
}

#[test]
fn header_control_click_is_not_reported_as_a_node_or_wire_click() {
    let (context, mut snarl, mut probe, [source, _, _]) = fixture(false);
    let control = probe.controls[&source];
    assert!(probe.geometry[&source].header.contains_rect(control));
    click(
        &context,
        &mut snarl,
        &mut probe,
        control.center(),
        PointerButton::Primary,
    );
    assert_eq!(probe.control_clicks, 1);
    assert!(probe.node_clicks.is_empty());
    assert!(probe.wire_clicks.is_empty());
    assert_eq!(snarl.wires().count(), 2);
}

#[test]
fn native_pin_drag_connects_without_forging_a_primary_focus_click() {
    let (context, mut snarl, mut probe, [source, target, _]) = fixture(false);
    let out_pin = OutPinId {
        node: source,
        output: 0,
    };
    snarl.drop_outputs(out_pin);
    settle(&context, &mut snarl, &mut probe);
    let from = probe.geometry[&source].outputs[0].1;
    let to = probe.geometry[&target].inputs[0].1;
    let _ = frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![Event::PointerMoved(from)],
    );
    let _ = frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(from, PointerButton::Primary, true)],
    );
    let _ = frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![Event::PointerMoved(from + vec2(40.0, 0.0))],
    );
    let _ = frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![Event::PointerMoved(to)],
    );
    let _ = frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(to, PointerButton::Primary, false)],
    );
    assert_eq!(
        snarl.wires().collect::<Vec<_>>(),
        vec![(
            out_pin,
            InPinId {
                node: target,
                input: 0
            }
        )]
    );
    assert!(probe.node_clicks.is_empty());
    assert!(probe.wire_clicks.is_empty());
    assert_eq!(probe.disconnected, 0);
}

// Deliberately implements none of the new hooks, as existing downstream viewers do.
struct LegacyViewer;

#[allow(refining_impl_trait)]
impl SnarlViewer<Kind> for LegacyViewer {
    fn title(&mut self, _node: &Kind) -> String {
        "legacy".into()
    }

    fn inputs(&mut self, node: &Kind) -> usize {
        usize::from(matches!(node, Kind::Target))
    }

    fn outputs(&mut self, node: &Kind) -> usize {
        usize::from(matches!(node, Kind::Source))
    }

    fn show_input(&mut self, _pin: &InPin, _ui: &mut Ui, _snarl: &mut Snarl<Kind>) -> PinInfo {
        PinInfo::circle()
    }

    fn show_output(&mut self, _pin: &OutPin, _ui: &mut Ui, _snarl: &mut Snarl<Kind>) -> PinInfo {
        PinInfo::circle()
    }
}

#[test]
fn legacy_viewer_defaults_preserve_color_and_connections() {
    let context = Context::default();
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(pos2(100.0, 100.0), Kind::Source);
    let target = snarl.insert_node(pos2(500.0, 100.0), Kind::Target);
    let edge = (
        OutPinId {
            node: source,
            output: 0,
        },
        InPinId {
            node: target,
            input: 0,
        },
    );
    snarl.connect(edge.0, edge.1);
    let mut viewer = LegacyViewer;
    let mut output = context.run_ui(Default::default(), |ui| {
        assert_eq!(
            viewer.wire_color(edge.0, edge.1, MIXED_COLOR, ui, &snarl),
            MIXED_COLOR
        );
        viewer.wire_clicked(&[edge], Pos2::ZERO, ui, &mut snarl);
        viewer.node_clicked(source, ui, &mut snarl);
        SnarlWidget::new().show(&mut snarl, &mut viewer, ui);
    });
    output.textures_delta.clear();
    assert_eq!(snarl.wires().collect::<Vec<_>>(), vec![edge]);
}
