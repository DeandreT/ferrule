//! Authored native egui interaction fixtures; no mapping execution or private data.
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
    /// The widget `first_widget_id` names in each header row.
    header_first_widgets: Vec<Option<(egui::Sense, egui::Vec2)>>,
}

#[allow(refining_impl_trait)]
impl SnarlViewer<usize> for Probe {
    fn title(&mut self, node: &usize) -> String {
        format!("Node {node}")
    }
    fn inputs(&mut self, node: &usize) -> usize {
        usize::from(*node != 0)
    }
    fn outputs(&mut self, node: &usize) -> usize {
        usize::from(*node == 0)
    }
    fn show_input(
        &mut self,
        _pin: &InPin,
        ui: &mut egui::Ui,
        _snarl: &mut Snarl<usize>,
    ) -> PinInfo {
        ui.label("in");
        PinInfo::circle()
    }
    fn show_output(
        &mut self,
        _pin: &OutPin,
        ui: &mut egui::Ui,
        _snarl: &mut Snarl<usize>,
    ) -> PinInfo {
        ui.label("out");
        PinInfo::circle()
    }
    fn current_transform(
        &mut self,
        transform: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<usize>,
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
        snarl: &mut Snarl<usize>,
    ) {
        assert!(header.is_finite());
        if snarl[node] == 0 {
            self.from = outputs.first().map(|(_, pos)| *pos);
        }
        if snarl[node] == 1 {
            self.to = inputs.first().map(|(_, pos)| *pos);
        }
    }
    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<usize>,
    ) {
        let first = crate::canvas_keyboard::focus::first_widget_id(ui);
        self.header_first_widgets.push(
            ui.ctx()
                .read_response(first)
                .map(|response| (response.sense, response.rect.size())),
        );
        ui.label(self.title(&snarl[node]));
    }
    fn wire_clicked(
        &mut self,
        hits: &[(OutPinId, InPinId)],
        _pos: egui::Pos2,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<usize>,
    ) {
        self.clicks.push(hits.to_vec());
    }
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<usize>) {
        self.disconnected += 1;
        snarl.disconnect(from.id, to.id);
    }
}

fn frame(
    context: &egui::Context,
    snarl: &mut Snarl<usize>,
    probe: &mut Probe,
    events: Vec<egui::Event>,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let _ = context.run_test_ui(input, |ui| {
        let mut style = SnarlStyle::new();
        style.wire_style = Some(WireStyle::Line);
        SnarlWidget::new()
            .id(egui::Id::unique("hook-fixture"))
            .style(style)
            .show(snarl, probe, ui);
    });
}

#[test]
fn primary_wire_click_reports_all_coincident_edges_without_disconnect_or_drag_click() {
    let context = egui::Context::default();
    let mut snarl = Snarl::new();
    let a = snarl.insert_node(egui::pos2(60.0, 90.0), 0);
    let b = snarl.insert_node(egui::pos2(420.0, 90.0), 1);
    let c = snarl.insert_node(egui::pos2(420.0, 90.0), 2);
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
fn the_first_header_widget_is_the_collapse_button() {
    let context = egui::Context::default();
    let mut snarl = Snarl::new();
    snarl.insert_node(egui::pos2(60.0, 90.0), 0);
    snarl.insert_node(egui::pos2(420.0, 90.0), 1);
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
