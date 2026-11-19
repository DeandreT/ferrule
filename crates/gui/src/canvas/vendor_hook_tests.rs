//! Authored native egui interaction fixtures; no mapping execution or private data.
use crate::canvas::CanvasNode;
use crate::canvas::CanvasNode::Graph;
use crate::test_ui::ContextExt as _;

use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer, SnarlWidget, WireStyle};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};

#[derive(Default)]
struct Probe {
    transform: egui::emath::TSTransform,
    from: Option<egui::Pos2>,
    to: Option<egui::Pos2>,
    clicks: Vec<Vec<(OutPinId, InPinId)>>,
    disconnected: usize,
    blocker_rect: Option<egui::Rect>,
    blocker_width: f32,
    /// The widget `first_widget_id` names in each header row.
    header_first_widgets: Vec<Option<(egui::Sense, egui::Vec2)>>,
}

#[allow(refining_impl_trait)]
impl SnarlViewer<CanvasNode> for Probe {
    fn title(&mut self, node: &CanvasNode) -> String {
        format!("Node {node:?}")
    }
    fn inputs(&mut self, node: &CanvasNode) -> usize {
        usize::from(*node != Graph(0))
    }
    fn outputs(&mut self, node: &CanvasNode) -> usize {
        usize::from(*node == Graph(0))
    }
    fn show_input(
        &mut self,
        _pin: &InPin,
        ui: &mut egui::Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) -> PinInfo {
        ui.label("in");
        PinInfo::circle()
    }
    fn show_output(
        &mut self,
        _pin: &OutPin,
        ui: &mut egui::Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) -> PinInfo {
        ui.label("out");
        PinInfo::circle()
    }
    fn current_transform(
        &mut self,
        transform: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        self.transform = *transform;
    }
    fn node_geometry(
        &mut self,
        node: NodeId,
        header: egui::Rect,
        inputs: &[(usize, egui::Pos2)],
        outputs: &[(usize, egui::Pos2)],
        _ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        assert!(header.is_finite());
        if snarl[node] == Graph(0) {
            self.from = outputs.first().map(|(_, pos)| *pos);
        }
        if snarl[node] == Graph(1) {
            self.to = inputs.first().map(|(_, pos)| *pos);
        }
    }
    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let first = crate::canvas_keyboard::focus::first_widget_id(ui);
        self.header_first_widgets.push(
            ui.ctx()
                .read_response(first)
                .map(|response| (response.sense, response.rect.size())),
        );
        ui.label(self.title(&snarl[node]));
        if snarl[node] == Graph(2) && self.blocker_width > 0.0 {
            ui.set_min_width(self.blocker_width);
        }
    }
    fn final_node_rect(
        &mut self,
        node: NodeId,
        rect: egui::Rect,
        _: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        if snarl[node] == Graph(2) {
            self.blocker_rect = Some(rect);
        }
    }
    fn wire_clicked(
        &mut self,
        hits: &[(OutPinId, InPinId)],
        _pos: egui::Pos2,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        self.clicks.push(hits.to_vec());
    }
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        self.disconnected += 1;
        snarl.disconnect(from.id, to.id);
    }
}

fn frame(
    context: &egui::Context,
    snarl: &mut Snarl<CanvasNode>,
    probe: &mut Probe,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    context.run_test_ui(input, |ui| {
        let mut style = SnarlStyle::new();
        style.wire_style = Some(WireStyle::Line);
        SnarlWidget::new()
            .id(egui::Id::unique("hook-fixture"))
            .style(style)
            .show(snarl, probe, ui);
    })
}

#[test]
fn primary_wire_click_reports_all_coincident_edges_without_disconnect_or_drag_click() {
    let context = egui::Context::default();
    let mut snarl = Snarl::new();
    let a = snarl.insert_node(egui::pos2(60.0, 90.0), Graph(0));
    let b = snarl.insert_node(egui::pos2(420.0, 90.0), Graph(1));
    let c = snarl.insert_node(egui::pos2(420.0, 90.0), Graph(2));
    for node in [b, c] {
        snarl.connect(OutPinId { node: a, output: 0 }, InPinId { node, input: 0 });
    }
    let mut probe = Probe::default();
    for _ in 0..5 {
        frame(&context, &mut snarl, &mut probe, vec![]);
    }
    let pos = probe.transform * probe.from.unwrap().lerp(probe.to.unwrap(), 0.5);
    let button = |button, pressed, pos| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(egui::PointerButton::Primary, true, pos)],
    );
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(egui::PointerButton::Primary, false, pos)],
    );
    assert_eq!(probe.clicks.len(), 1);
    assert_eq!(probe.clicks[0].len(), 2);
    assert!(probe.clicks[0].windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(snarl.wires().count(), 2);
    let moved = pos + egui::vec2(0.0, 40.0);
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(egui::PointerButton::Primary, true, pos)],
    );
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![egui::Event::PointerMoved(moved)],
    );
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(egui::PointerButton::Primary, false, moved)],
    );
    assert_eq!(probe.clicks.len(), 1);
    let pos = probe.transform * probe.from.unwrap().lerp(probe.to.unwrap(), 0.5);
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(egui::PointerButton::Secondary, true, pos)],
    );
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![button(egui::PointerButton::Secondary, false, pos)],
    );
    assert_eq!(probe.disconnected, 1);
    assert_eq!(snarl.wires().count(), 1);
}

#[test]
fn moved_and_resized_cards_reroute_drawn_and_clickable_wire_after_arrange() {
    let context = egui::Context::default();
    let mut snarl = Snarl::new();
    let a = snarl.insert_node(egui::pos2(45.0, 100.0), Graph(0));
    let b = snarl.insert_node(egui::pos2(530.0, 100.0), Graph(1));
    let c = snarl.insert_node(egui::pos2(270.0, 220.0), Graph(2));
    let edge = (
        OutPinId { node: a, output: 0 },
        InPinId { node: b, input: 0 },
    );
    snarl.connect(edge.0, edge.1);
    let mut probe = Probe::default();
    let painted = |snarl: &mut Snarl<CanvasNode>, probe: &mut Probe| {
        for _ in 0..4 {
            frame(&context, snarl, probe, vec![]);
        }
        let output = frame(&context, snarl, probe, vec![]);
        let from = probe.transform * probe.from.unwrap();
        let to = probe.transform * probe.to.unwrap();
        let mut shapes = output.shapes.iter().map(|s| &s.shape).collect::<Vec<_>>();
        while let Some(shape) = shapes.pop() {
            use egui::epaint::Shape;
            let points: &[egui::Pos2] = match shape {
                Shape::LineSegment { points, .. } => points,
                Shape::Path(path) => &path.points,
                Shape::Vec(children) => {
                    shapes.extend(children);
                    continue;
                }
                _ => continue,
            };
            if points.first().is_some_and(|p| p.distance(from) < 1.0)
                && points.last().is_some_and(|p| p.distance(to) < 1.0)
            {
                return points.to_vec();
            }
        }
        panic!("no painted wire between measured pins {from:?} and {to:?}");
    };
    assert_eq!(painted(&mut snarl, &mut probe).len(), 2);
    snarl.get_node_info_mut(c).unwrap().pos.y = 100.0;
    let detour = painted(&mut snarl, &mut probe);
    assert!(detour.len() > 2);
    let raw_pins = (probe.from, probe.to);
    let click = detour
        .windows(2)
        .filter(|s| (s[0].y - s[1].y).abs() < 0.1 && (s[0].y - detour[0].y).abs() > 4.0)
        .max_by(|l, r| l[0].distance(l[1]).total_cmp(&r[0].distance(r[1])))
        .map(|s| s[0].lerp(s[1], 0.5))
        .unwrap();
    frame(
        &context,
        &mut snarl,
        &mut probe,
        vec![egui::Event::PointerMoved(click)],
    );
    for pressed in [true, false] {
        frame(
            &context,
            &mut snarl,
            &mut probe,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    assert_eq!(probe.clicks, [vec![edge]]);
    let width = probe.blocker_rect.unwrap().width();
    probe.blocker_width = width + 60.0;
    assert_ne!(painted(&mut snarl, &mut probe), detour);
    assert!(probe.blocker_rect.unwrap().width() > width);
    assert_eq!((probe.from, probe.to), raw_pins);
    snarl.get_node_info_mut(c).unwrap().pos.y = 220.0;
    assert_eq!(painted(&mut snarl, &mut probe).len(), 2);
    crate::canvas_layout::arrange_snarl(&mut snarl, &Default::default(), Default::default());
    let positions = [a, b, c].map(|id| snarl.get_node_info(id).unwrap().pos);
    let _ = painted(&mut snarl, &mut probe);
    assert_ne!((probe.from, probe.to), raw_pins);
    assert_eq!(
        positions,
        [a, b, c].map(|id| snarl.get_node_info(id).unwrap().pos)
    );
    assert_eq!(snarl.wires().count(), 1);
}

#[test]
fn the_first_header_widget_is_the_collapse_button() {
    let context = egui::Context::default();
    let mut snarl = Snarl::new();
    snarl.insert_node(egui::pos2(60.0, 90.0), Graph(0));
    snarl.insert_node(egui::pos2(420.0, 90.0), Graph(1));
    let mut probe = Probe::default();
    for _ in 0..3 {
        frame(&context, &mut snarl, &mut probe, vec![]);
    }
    let icon = context.global_style().spacing.icon_width;
    assert!(!probe.header_first_widgets.is_empty());
    assert!(
        probe
            .header_first_widgets
            .iter()
            .all(|widget| widget
                .is_some_and(|(sense, size)| sense == egui::Sense::click()
                    && size == egui::Vec2::splat(icon))),
        "{:?}",
        probe.header_first_widgets
    );
}
