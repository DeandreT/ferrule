//! Presentation-only connection focus, static lineage and measured layout previews.

use crate::canvas::CanvasNode;
use crate::graph_viewer::GraphViewer;
use egui_snarl::ui::SnarlViewer;
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Edge {
    pub from: CanvasNode,
    pub output: usize,
    pub to: CanvasNode,
    pub input: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Node(CanvasNode),
    Edge(Edge),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Trace {
    #[default]
    Direct,
    Inputs,
    Outputs,
    Both,
}

#[derive(Clone, Default)]
pub(crate) struct Geometry {
    pub size: egui::Vec2,
    pub offset: egui::Vec2,
    pub inputs: Vec<(usize, egui::Vec2)>,
    pub outputs: Vec<(usize, egui::Vec2)>,
}

#[derive(Clone, Default)]
struct Change {
    before: BTreeMap<CanvasNode, egui::Pos2>,
    after: BTreeMap<CanvasNode, egui::Pos2>,
}

#[derive(Clone, Default)]
struct State {
    nodes: BTreeSet<CanvasNode>,
    selected: BTreeSet<CanvasNode>,
    toolbar_top: Option<f32>,
    fit_changed: bool,
    visual_edges: BTreeMap<(OutPinId, InPinId), Vec<Edge>>,
    edge_visual: BTreeMap<Edge, (OutPinId, InPinId)>,
    widths: BTreeMap<CanvasNode, f32>,
    edit_requested: Option<CanvasNode>,
    focus: Option<Focus>,
    trace: Trace,
    edges: Vec<Edge>,
    bright: BTreeSet<Edge>,
    picker: Vec<Edge>,
    cursor: usize,
    reveal_picker_cursor: bool,
    pinned: BTreeSet<CanvasNode>,
    measured: BTreeMap<CanvasNode, Geometry>,
    preview: Option<Change>,
    undo: Vec<Change>,
    redo: Vec<Change>,
    selection_only: bool,
    message: String,
}

fn active(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::unique("active_canvas_tools")))
}

fn read<R>(ctx: &egui::Context, f: impl FnOnce(&State) -> R) -> Option<R> {
    let id = active(ctx)?;
    ctx.data_mut(|data| Some(f(data.get_temp_mut_or_default::<State>(id))))
}

fn edit<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let id = active(ctx)?;
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(id);
        Some(f(state))
    })
}

pub(crate) fn begin(
    ctx: &egui::Context,
    salt: egui::Id,
    viewer: &GraphViewer<'_>,
    snarl: &Snarl<CanvasNode>,
) {
    let id = salt.with("canvas_tools");
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique("active_canvas_tools"), id));
    let owned = if viewer.function_output.is_none() {
        crate::graph_viewer::sequence_item_ids(
            viewer.graph,
            viewer.root_scope,
            viewer.extra_targets,
            viewer.inactive_target_scopes,
            viewer.project_references,
        )
    } else {
        Default::default()
    };
    let unavailable = viewer.project_references.source_conflicts();
    let edges = crate::app::semantic_canvas_wires(
        viewer.graph,
        viewer.root_scope,
        viewer.source_blocks,
        viewer.target_blocks,
        snarl,
        crate::app::SourcePinPolicy {
            owned_items: &owned,
            unavailable_blocks: &unavailable,
        },
    )
    .into_iter()
    .filter_map(|(from, to)| {
        Some(Edge {
            from: *snarl.get_node(from.node)?,
            output: from.output,
            to: *snarl.get_node(to.node)?,
            input: to.input,
        })
    })
    .collect::<Vec<_>>();
    edit(ctx, |state| {
        let current_nodes = snarl
            .node_ids()
            .map(|(_, node)| *node)
            .collect::<BTreeSet<_>>();
        if state.edges != edges || state.nodes != current_nodes {
            state.preview = None;
            state.undo.clear();
            state.redo.clear();
        }
        state.nodes = current_nodes;
        state.pinned.retain(|node| state.nodes.contains(node));
        state.widths.retain(|node, _| state.nodes.contains(node));
        state.measured.retain(|node, _| state.nodes.contains(node));
        invalidate_moved_preview(state, snarl);
        state.edges = edges;
        let nodes = snarl
            .node_ids()
            .map(|(id, node)| (*node, id))
            .collect::<BTreeMap<_, _>>();
        let previous_visual_edges = std::mem::take(&mut state.visual_edges);
        state.edge_visual.clear();
        for edge in &state.edges {
            if let (Some(&from), Some(&to), Some(output), Some(input)) = (
                nodes.get(&edge.from),
                nodes.get(&edge.to),
                visual_pin(viewer, edge.from, edge.output),
                visual_pin(viewer, edge.to, edge.input),
            ) {
                state
                    .visual_edges
                    .entry((OutPinId { node: from, output }, InPinId { node: to, input }))
                    .or_default()
                    .push(*edge);
                state.edge_visual.insert(
                    *edge,
                    (OutPinId { node: from, output }, InPinId { node: to, input }),
                );
            }
        }
        if previous_visual_edges != state.visual_edges && state.preview.is_some() {
            state.preview = None;
            state.message = "Visible connections changed; preview the layout again.".into();
        }
        let nodes = snarl
            .node_ids()
            .map(|(_, node)| *node)
            .collect::<BTreeSet<_>>();
        if state.focus.is_some_and(|focus| match focus {
            Focus::Node(node) => !nodes.contains(&node),
            Focus::Edge(edge) => !state.edges.contains(&edge),
        }) {
            state.focus = None;
        }
        state.picker.retain(|edge| state.edges.contains(edge));
        recompute(state);
    });
}

fn recompute(state: &mut State) {
    state.bright.clear();
    let Some(focus) = state.focus else {
        return;
    };
    let mut upstream = BTreeSet::new();
    let mut downstream = BTreeSet::new();
    match focus {
        Focus::Node(node) => {
            upstream.insert(node);
            downstream.insert(node);
            if state.trace == Trace::Direct {
                state.bright.extend(
                    state
                        .edges
                        .iter()
                        .copied()
                        .filter(|edge| edge.from == node || edge.to == node),
                );
                return;
            }
        }
        Focus::Edge(edge) => {
            state.bright.insert(edge);
            upstream.insert(edge.from);
            downstream.insert(edge.to);
            if state.trace == Trace::Direct {
                return;
            }
        }
    }
    for (inputs, mut seen) in [(true, upstream), (false, downstream)] {
        if (inputs && !matches!(state.trace, Trace::Inputs | Trace::Both))
            || (!inputs && !matches!(state.trace, Trace::Outputs | Trace::Both))
        {
            continue;
        }
        let mut pending = seen.iter().copied().collect::<Vec<_>>();
        let adjacent =
            state
                .edges
                .iter()
                .fold(BTreeMap::<CanvasNode, Vec<Edge>>::new(), |mut map, edge| {
                    map.entry(if inputs { edge.to } else { edge.from })
                        .or_default()
                        .push(*edge);
                    map
                });
        while let Some(node) = pending.pop() {
            for edge in adjacent.get(&node).into_iter().flatten() {
                if (inputs && edge.to == node) || (!inputs && edge.from == node) {
                    state.bright.insert(*edge);
                    let next = if inputs { edge.from } else { edge.to };
                    if seen.insert(next) {
                        pending.push(next);
                    }
                }
            }
        }
    }
}

fn visual_pin(viewer: &GraphViewer<'_>, node: CanvasNode, pin: usize) -> Option<usize> {
    match node {
        CanvasNode::SourceBlock(block) => viewer.endpoint_scroll.visual_pin_for_semantic(
            node,
            pin,
            viewer.source_blocks.get(block)?.leaves.len(),
        ),
        CanvasNode::TargetBlock(block) => viewer.endpoint_scroll.visual_pin_for_semantic(
            node,
            pin,
            viewer.target_blocks.get(block)?.leaves.len(),
        ),
        _ => Some(pin),
    }
}

pub(crate) fn wire_bright(
    ctx: &egui::Context,
    viewer: &GraphViewer<'_>,
    snarl: &Snarl<CanvasNode>,
    from: OutPinId,
    to: InPinId,
) -> Option<bool> {
    let _ = (viewer, snarl);
    read(ctx, |state| {
        state.focus.map(|_| {
            state
                .visual_edges
                .get(&(from, to))
                .is_some_and(|edges| edges.iter().any(|edge| state.bright.contains(edge)))
        })
    })
    .flatten()
}

pub(crate) fn wire_click(
    ctx: &egui::Context,
    viewer: &mut GraphViewer<'_>,
    snarl: &Snarl<CanvasNode>,
    hits: &[(OutPinId, InPinId)],
) {
    let _ = snarl;
    let candidates = read(ctx, |state| {
        hits.iter()
            .filter_map(|hit| state.visual_edges.get(hit))
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    if candidates.len() == 1 {
        choose(ctx, viewer, candidates[0]);
    } else {
        edit(ctx, |state| {
            state.picker = candidates;
            state.cursor = 0;
            state.reveal_picker_cursor = true;
        });
    }
    ctx.request_repaint();
}

fn choose(ctx: &egui::Context, viewer: &mut GraphViewer<'_>, edge: Edge) {
    for (node, semantic) in [(edge.from, edge.output), (edge.to, edge.input)] {
        let total = match node {
            CanvasNode::SourceBlock(block) => {
                viewer.source_blocks.get(block).map(|b| b.leaves.len())
            }
            CanvasNode::TargetBlock(block) => {
                viewer.target_blocks.get(block).map(|b| b.leaves.len())
            }
            _ => None,
        };
        if let Some(total) = total {
            viewer.endpoint_scroll.reveal(node, total, semantic);
        }
    }
    edit(ctx, |state| {
        state.focus = if state.focus == Some(Focus::Edge(edge)) {
            None
        } else {
            Some(Focus::Edge(edge))
        };
        state.trace = Trace::Direct;
        state.picker.clear();
        recompute(state);
    });
}

pub(crate) fn node_click(ctx: &egui::Context, node: CanvasNode) {
    edit(ctx, |state| {
        state.focus = if state.focus == Some(Focus::Node(node)) {
            None
        } else {
            Some(Focus::Node(node))
        };
        state.trace = Trace::Direct;
        recompute(state);
    });
    ctx.request_repaint();
}

pub(crate) fn clear(ctx: &egui::Context) {
    edit(ctx, |state| {
        state.focus = None;
        state.picker.clear();
        state.bright.clear();
    });
    ctx.request_repaint();
}

pub(crate) fn record_ports(
    ctx: &egui::Context,
    viewer: &GraphViewer<'_>,
    node: NodeId,
    inputs: &[(usize, egui::Pos2)],
    outputs: &[(usize, egui::Pos2)],
    snarl: &Snarl<CanvasNode>,
) {
    let _ = viewer;
    if let Some(info) = snarl.get_node_info(node) {
        edit(ctx, |state| {
            let geometry = state.measured.entry(info.value).or_default();
            let inputs = inputs
                .iter()
                .map(|(pin, pos)| (*pin, *pos - info.pos))
                .collect::<Vec<_>>();
            let outputs = outputs
                .iter()
                .map(|(pin, pos)| (*pin, *pos - info.pos))
                .collect::<Vec<_>>();
            if (ports_changed(&geometry.inputs, &inputs)
                || ports_changed(&geometry.outputs, &outputs))
                && state.preview.is_some()
            {
                state.preview = None;
                state.message = "Port geometry changed; preview the layout again.".into();
            }
            geometry.inputs = inputs;
            geometry.outputs = outputs;
        });
    }
}

pub(crate) fn record_rect(ui: &egui::Ui, node: CanvasNode, position: egui::Pos2, rect: egui::Rect) {
    edit(ui.ctx(), |state| {
        let geometry = state.measured.entry(node).or_default();
        if ((geometry.size - rect.size()).abs().max_elem() > 0.5
            || (geometry.offset - (rect.min - position)).abs().max_elem() > 0.5)
            && state.preview.is_some()
        {
            state.preview = None;
            state.message = "Node geometry changed; preview the layout again.".into();
        }
        geometry.size = rect.size();
        geometry.offset = rect.min - position;
    });
    let outline = read(ui.ctx(), |state| {
        state.focus.is_some_and(|focus| match focus {
            Focus::Node(focused) => focused == node,
            Focus::Edge(edge) => edge.from == node || edge.to == node,
        })
    })
    .unwrap_or(false);
    if outline {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            4.0,
            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Outside,
        );
    }
    if let Some(Some(pos)) = read(ui.ctx(), |state| {
        state.preview.as_ref().and_then(|change| {
            change
                .after
                .get(&node)
                .map(|pos| *pos + state.measured[&node].offset)
        })
    }) {
        ui.painter().rect_stroke(
            egui::Rect::from_min_size(pos, rect.size()),
            4.0,
            egui::Stroke::new(1.5, ui.visuals().warn_fg_color),
            egui::StrokeKind::Outside,
        );
    }
}

fn ports_changed(before: &[(usize, egui::Vec2)], after: &[(usize, egui::Vec2)]) -> bool {
    before.len() != after.len()
        || before
            .iter()
            .zip(after)
            .any(|((old_pin, old), (new_pin, new))| {
                old_pin != new_pin || (*old - *new).abs().max_elem() > 0.5
            })
}

fn label(viewer: &mut GraphViewer<'_>, node: CanvasNode, pin: usize, input: bool) -> String {
    let name = match node {
        CanvasNode::SourceBlock(block) => viewer
            .source_blocks
            .get(block)
            .and_then(|b| b.leaves.get(pin))
            .map(|p| p.label.clone()),
        CanvasNode::TargetBlock(block) => viewer
            .target_blocks
            .get(block)
            .and_then(|b| b.leaves.get(pin))
            .map(|p| p.label.clone()),
        _ => None,
    };
    name.map(|name| format!("{} · {} · port {}", viewer.title(&node), name, pin + 1))
        .unwrap_or_else(|| {
            format!(
                "{} · {} {} · {:?}",
                viewer.title(&node),
                if input { "input" } else { "output" },
                pin + 1,
                node
            )
        })
}

fn edge_label(viewer: &mut GraphViewer<'_>, edge: Edge) -> String {
    format!(
        "{} → {}",
        label(viewer, edge.from, edge.output, false),
        label(viewer, edge.to, edge.input, true)
    )
}

fn positions(snarl: &Snarl<CanvasNode>) -> BTreeMap<CanvasNode, egui::Pos2> {
    snarl
        .nodes_pos_ids()
        .map(|(_, pos, node)| (*node, pos))
        .collect()
}

fn apply_positions(snarl: &mut Snarl<CanvasNode>, positions: &BTreeMap<CanvasNode, egui::Pos2>) {
    let nodes = snarl
        .node_ids()
        .map(|(id, node)| (id, *node))
        .collect::<Vec<_>>();
    for (id, node) in nodes {
        if let Some(pos) = positions.get(&node)
            && let Some(info) = snarl.get_node_info_mut(id)
        {
            info.pos = *pos;
        }
    }
}

fn propose(
    state: &mut State,
    snarl: &Snarl<CanvasNode>,
    selected: &[NodeId],
    wire: crate::appearance::WireAppearance,
    width: f32,
) {
    state.preview = None;
    if snarl.node_ids().count() > 2_000 || snarl.wires().count() > 8_000 {
        state.message = "Layout is bounded to 2,000 nodes / 8,000 displayed edges.".into();
        return;
    }
    let selected = selected
        .iter()
        .filter_map(|id| snarl.get_node(*id).copied())
        .collect::<BTreeSet<_>>();
    if state.selection_only && selected.is_empty() {
        state.message = "Select nodes to preview a selected-region layout.".into();
        return;
    }
    let sizes = state
        .measured
        .iter()
        .map(|(node, geometry)| (*node, geometry.size))
        .collect::<BTreeMap<_, _>>();
    if snarl.nodes_pos_ids().any(|(_, pos, node)| {
        !pos.is_finite()
            || !state
                .measured
                .get(node)
                .is_some_and(|geometry| geometry.offset.is_finite())
            || !sizes.get(node).is_some_and(|size| {
                size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0
            })
    }) {
        state.message = "Wait for measured node geometry before previewing.".into();
        return;
    }
    let mut preview = snarl.clone();
    let mut ports = BTreeMap::new();
    for (id, node) in snarl.node_ids() {
        if let Some(geometry) = state.measured.get(node) {
            for (pin, pos) in &geometry.inputs {
                ports.insert((id, true, *pin), pos.y);
            }
            for (pin, pos) in &geometry.outputs {
                ports.insert((id, false, *pin), pos.y);
            }
        }
    }
    if snarl.wires().any(|(from, to)| {
        !ports
            .get(&(from.node, false, from.output))
            .is_some_and(|offset| offset.is_finite())
            || !ports
                .get(&(to.node, true, to.input))
                .is_some_and(|offset| offset.is_finite())
    }) {
        state.message = "Wait for measured ports before previewing this layout.".into();
        return;
    }
    crate::canvas_layout::arrange_snarl_measured(&mut preview, &sizes, &ports, wire, width);
    let before = positions(snarl);
    let mut after = positions(&preview);
    let anchors = before
        .iter()
        .filter(|(node, _)| {
            state.pinned.contains(node) || (state.selection_only && !selected.contains(node))
        })
        .map(|(node, pos)| (*node, *pos))
        .collect::<BTreeMap<_, _>>();
    for (node, pos) in &anchors {
        after.insert(*node, *pos);
    }
    // Keep measured port lanes aligned where possible, then resolve collisions
    // against immutable anchors and already accepted placements deterministically.
    let mut placed = anchors
        .iter()
        .map(|(node, pos)| {
            (
                *node,
                egui::Rect::from_min_size(*pos + state.measured[node].offset, sizes[node]),
            )
        })
        .collect::<Vec<_>>();
    let mut moving = after
        .keys()
        .filter(|node| !anchors.contains_key(node))
        .copied()
        .collect::<Vec<_>>();
    moving.sort_by(|a, b| {
        after[a]
            .x
            .total_cmp(&after[b].x)
            .then_with(|| after[a].y.total_cmp(&after[b].y))
            .then(a.cmp(b))
    });
    for node in moving {
        let mut pos = after[&node];
        let geometry = &state.measured[&node];
        let incident = state
            .edges
            .iter()
            .filter(|edge| edge.to == node)
            .filter_map(|edge| {
                let from = state.measured.get(&edge.from)?;
                let (from_visual, to_visual) = state.edge_visual.get(edge)?;
                let output = from
                    .outputs
                    .iter()
                    .find(|(pin, _)| *pin == from_visual.output)?
                    .1
                    .y;
                let input = geometry
                    .inputs
                    .iter()
                    .find(|(pin, _)| *pin == to_visual.input)?
                    .1
                    .y;
                Some(after.get(&edge.from)?.y + output - input)
            })
            .collect::<Vec<_>>();
        if !incident.is_empty() {
            pos.y = incident.iter().sum::<f32>() / incident.len() as f32;
        }
        for _ in 0..placed.len() + 1 {
            let rect = egui::Rect::from_min_size(pos + geometry.offset, sizes[&node]).expand(12.0);
            let next = placed
                .iter()
                .filter(|(_, other)| rect.intersects(*other))
                .map(|(_, other)| other.bottom() + 24.0 - geometry.offset.y)
                .fold(pos.y, f32::max);
            if next == pos.y {
                break;
            }
            pos.y = next;
        }
        if !pos.x.is_finite() || !pos.y.is_finite() {
            state.message = "Invalid layout geometry; no preview applied.".into();
            return;
        }
        after.insert(node, pos);
        placed.push((
            node,
            egui::Rect::from_min_size(pos + geometry.offset, sizes[&node]),
        ));
    }
    state.preview = Some(Change { before, after });
    state.message = "Preview outlines only; saved positions stay unchanged until Apply.".into();
}

pub(crate) struct ToolbarOptions {
    pub wire: crate::appearance::WireAppearance,
    pub requested: bool,
    pub search_open: bool,
}

pub(crate) fn toolbar(
    ui: &mut egui::Ui,
    viewer: &mut GraphViewer<'_>,
    snarl: &mut Snarl<CanvasNode>,
    selected: &[NodeId],
    options: ToolbarOptions,
) {
    let ToolbarOptions {
        wire,
        requested,
        search_open,
    } = options;
    let Some(id) = active(ui.ctx()) else {
        return;
    };
    let mut state = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<State>(id))
        .unwrap_or_default();
    let selection = selected
        .iter()
        .filter_map(|id| snarl.get_node(*id).copied())
        .collect();
    if state.selection_only && state.selected != selection {
        state.preview = None;
        state.message.clear();
    }
    state.selected = selection;
    if !search_open
        && state.picker.is_empty()
        && !egui::Popup::is_any_open(ui.ctx())
        && !GraphViewer::properties_open(ui.ctx())
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2))
    {
        state.edit_requested = if selected.len() == 1 {
            snarl.get_node(selected[0]).copied()
        } else if let Some(Focus::Node(node)) = state.focus {
            Some(node)
        } else {
            None
        };
    }
    let mut preview = requested;
    ui.horizontal_wrapped(|ui| {
        preview |= ui.button("Auto Compact · Preview").clicked();
        if ui
            .checkbox(&mut state.selection_only, "Selected region")
            .changed()
        {
            state.preview = None;
            state.message.clear();
        }
        if !selected.is_empty() && ui.button("Pin / unpin selected").clicked() {
            state.preview = None;
            state.message.clear();
            for node in selected.iter().filter_map(|id| snarl.get_node(*id)) {
                if !state.pinned.remove(node) {
                    state.pinned.insert(*node);
                }
            }
        }
        ui.small(format!("{} pinned", state.pinned.len()));
        if state.preview.is_some() {
            if ui.button("Apply").clicked()
                && let Some(change) = state.preview.take()
            {
                apply_positions(snarl, &change.after);
                state.undo.push(change);
                state.redo.clear();
                state.message.clear();
            }
            if ui.button("Cancel").clicked() {
                state.preview = None;
                state.message.clear();
            }
        }
        if ui
            .add_enabled(!state.undo.is_empty(), egui::Button::new("Undo layout"))
            .clicked()
            && let Some(change) = state.undo.pop()
        {
            apply_positions(snarl, &change.before);
            state.redo.push(change);
            state.preview = None;
        }
        if ui
            .add_enabled(!state.redo.is_empty(), egui::Button::new("Redo layout"))
            .clicked()
            && let Some(change) = state.redo.pop()
        {
            apply_positions(snarl, &change.after);
            state.undo.push(change);
            state.preview = None;
        }
    });
    if preview {
        propose(&mut state, snarl, selected, wire, ui.available_width());
    }
    if !state.message.is_empty() {
        ui.small(&state.message);
    }
    if let Some(focus) = state.focus {
        let summary = match focus {
            Focus::Edge(edge) => edge_label(viewer, edge),
            Focus::Node(node) => format!(
                "{} · {} direct connections",
                viewer.title(&node),
                state
                    .edges
                    .iter()
                    .filter(|edge| edge.from == node || edge.to == node)
                    .count()
            ),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(summary);
            for (mode, label) in [
                (Trace::Direct, "Direct"),
                (Trace::Inputs, "← Inputs"),
                (Trace::Outputs, "Outputs →"),
                (Trace::Both, "Both"),
            ] {
                if ui.selectable_value(&mut state.trace, mode, label).changed() {
                    recompute(&mut state);
                }
            }
            if ui.button("Clear focus").clicked() {
                state.focus = None;
                state.bright.clear();
            }
        });
        if state.trace != Trace::Direct {
            ui.small("Static wiring only; all connected branches, not an execution trace.");
        }
    }
    let editing = search_open
        || ui.ctx().egui_wants_keyboard_input()
        || egui::Popup::is_any_open(ui.ctx())
        || GraphViewer::properties_open(ui.ctx());
    let top = ui.available_rect_before_wrap().top();
    state.fit_changed |= state.toolbar_top.is_some_and(|old| (old - top).abs() > 0.5);
    state.toolbar_top = Some(top);
    let picker_keyboard = !search_open
        && !egui::Popup::is_any_open(ui.ctx())
        && !GraphViewer::properties_open(ui.ctx());
    let escape = (!editing || (!state.picker.is_empty() && picker_keyboard))
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if !state.picker.is_empty() {
        if escape {
            state.picker.clear();
        } else {
            let mut choice = None;
            if picker_keyboard {
                if ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                }) {
                    state.cursor = (state.cursor + 1) % state.picker.len();
                    state.reveal_picker_cursor = true;
                }
                if ui
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                {
                    state.cursor = state
                        .cursor
                        .checked_sub(1)
                        .unwrap_or(state.picker.len() - 1);
                    state.reveal_picker_cursor = true;
                }
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    choice = state.picker.get(state.cursor).copied();
                }
            }
            egui::Window::new("Choose connection")
                .id(id.with("picker"))
                .collapsible(false)
                .resizable(true)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        "Choose an exact source and destination. This does not disconnect it.",
                    );
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for (index, edge) in state.picker.iter().enumerate() {
                                let response = ui.selectable_label(
                                    state.cursor == index,
                                    edge_label(viewer, *edge),
                                );
                                if state.cursor == index && state.reveal_picker_cursor {
                                    response.scroll_to_me(None);
                                }
                                if response.clicked() {
                                    choice = Some(*edge);
                                }
                            }
                        });
                    state.reveal_picker_cursor = false;
                    if ui.button("Cancel").clicked() {
                        state.picker.clear();
                        choice = None;
                    }
                });
            ui.ctx().data_mut(|data| data.insert_temp(id, state));
            if let Some(edge) = choice {
                choose(ui.ctx(), viewer, edge);
            }
            return;
        }
    } else if escape {
        state.focus = None;
        state.bright.clear();
    }
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}

pub(crate) fn fanout(
    ui: &mut egui::Ui,
    viewer: &mut GraphViewer<'_>,
    from: OutPinId,
    snarl: &Snarl<CanvasNode>,
) {
    let edges = read(ui.ctx(), |state| {
        state
            .edges
            .iter()
            .copied()
            .filter(|edge| {
                snarl.get_node(from.node) == Some(&edge.from)
                    && visual_pin(viewer, edge.from, edge.output) == Some(from.output)
            })
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    if edges.len() > 1 {
        let label = format!("Choose one of {} connections", edges.len());
        let text = egui::WidgetText::concat(
            ui,
            [
                egui::RichText::new(format!("{} ", edges.len())),
                crate::icons::text(lucide_icons::Icon::GitFork, 12.0),
            ],
        );
        let response = ui
            .add(egui::Button::new(text).small())
            .accessible_name(label)
            .on_hover_text("Choose an exact connection");
        if response.clicked() {
            edit(ui.ctx(), |state| {
                state.picker = edges;
                state.cursor = 0;
                state.reveal_picker_cursor = true;
            });
            ui.ctx().request_repaint();
        }
    }
}

pub(crate) fn manual_width(ctx: &egui::Context, node: CanvasNode) -> Option<f32> {
    read(ctx, |state| state.widths.get(&node).copied()).flatten()
}

pub(crate) fn size_controls(ui: &mut egui::Ui, node: CanvasNode) {
    let mut width = manual_width(ui.ctx(), node).unwrap_or(180.0);
    if ui
        .add(egui::Slider::new(&mut width, 60.0..=480.0).text("Card width"))
        .changed()
    {
        edit(ui.ctx(), |state| {
            state.widths.insert(node, width);
        });
    }
    if ui.button("Reset to auto size").clicked() {
        edit(ui.ctx(), |state| {
            state.widths.remove(&node);
        });
    }
}

pub(crate) fn take_edit(ctx: &egui::Context, node: CanvasNode) -> bool {
    edit(ctx, |state| {
        if state.edit_requested == Some(node) {
            state.edit_requested = None;
            true
        } else {
            false
        }
    })
    .unwrap_or(false)
}

pub(crate) fn consume_fit_change(ctx: &egui::Context) -> bool {
    edit(ctx, |state| std::mem::take(&mut state.fit_changed)).unwrap_or(false)
}

pub(crate) fn selected_node(ctx: &egui::Context, node: CanvasNode) -> bool {
    read(ctx, |state| state.selected.contains(&node)).unwrap_or(false)
}

pub(crate) fn focused_node(ctx: &egui::Context, node: CanvasNode) -> bool {
    read(ctx, |state| {
        state.focus.is_some_and(|focus| match focus {
            Focus::Node(value) => value == node,
            Focus::Edge(edge) => edge.from == node || edge.to == node,
        })
    })
    .unwrap_or(false)
}

pub(crate) fn picker_open(ctx: &egui::Context) -> bool {
    read(ctx, |state| !state.picker.is_empty()).unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn focused_edge(ctx: &egui::Context) -> Option<Edge> {
    read(ctx, |state| match state.focus {
        Some(Focus::Edge(edge)) => Some(edge),
        _ => None,
    })
    .flatten()
}

fn invalidate_moved_preview(state: &mut State, snarl: &Snarl<CanvasNode>) {
    if state
        .preview
        .as_ref()
        .is_some_and(|change| change.before != positions(snarl))
    {
        state.preview = None;
        state.message = "Node positions changed; preview the layout again.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_preview_is_deterministic_preserves_anchors_and_is_reversible() {
        let mut snarl = Snarl::new();
        let a = snarl.insert_node(egui::pos2(300.0, 400.0), CanvasNode::Graph(1));
        let b = snarl.insert_node(egui::pos2(600.0, 100.0), CanvasNode::Graph(2));
        let c = snarl.insert_node(egui::pos2(900.0, 200.0), CanvasNode::Graph(3));
        let from = OutPinId { node: a, output: 0 };
        let to = InPinId { node: b, input: 0 };
        snarl.connect(from, to);
        let edge = Edge {
            from: CanvasNode::Graph(1),
            output: 0,
            to: CanvasNode::Graph(2),
            input: 0,
        };
        let mut state = State {
            edges: vec![edge],
            edge_visual: BTreeMap::from([(edge, (from, to))]),
            selection_only: true,
            ..Default::default()
        };
        for node in [
            CanvasNode::Graph(1),
            CanvasNode::Graph(2),
            CanvasNode::Graph(3),
        ] {
            state.measured.insert(
                node,
                Geometry {
                    size: egui::vec2(120.0, 44.0),
                    offset: egui::Vec2::ZERO,
                    inputs: vec![(0, egui::vec2(0.0, 19.0))],
                    outputs: vec![(0, egui::vec2(120.0, 19.0))],
                },
            );
        }
        state.pinned.insert(CanvasNode::Graph(1));
        let before = positions(&snarl);
        let wires = snarl.wires().collect::<BTreeSet<_>>();
        propose(&mut state, &snarl, &[b], Default::default(), 1200.0);
        let change = state.preview.clone().expect("measured preview");
        assert_eq!(positions(&snarl), before);
        assert_eq!(
            change.after[&CanvasNode::Graph(1)],
            before[&CanvasNode::Graph(1)]
        );
        assert_eq!(
            change.after[&CanvasNode::Graph(3)],
            before[&CanvasNode::Graph(3)]
        );
        propose(&mut state, &snarl, &[b], Default::default(), 1200.0);
        assert_eq!(state.preview.as_ref().unwrap().after, change.after);
        apply_positions(&mut snarl, &change.after);
        assert_eq!(snarl.wires().collect::<BTreeSet<_>>(), wires);
        assert_eq!(snarl[c], CanvasNode::Graph(3));
        apply_positions(&mut snarl, &change.before);
        assert_eq!(positions(&snarl), before);
        assert_eq!(snarl.wires().collect::<BTreeSet<_>>(), wires);
        state
            .measured
            .get_mut(&CanvasNode::Graph(2))
            .unwrap()
            .size
            .x = f32::NAN;
        state.preview = None;
        propose(&mut state, &snarl, &[b], Default::default(), 1200.0);
        assert!(state.preview.is_none());
        assert_eq!(positions(&snarl), before);
    }
    #[test]
    fn exact_fanout_focus_and_static_cycle_tracing_are_distinct() {
        let edge = |a, b| Edge {
            from: CanvasNode::Graph(a),
            output: 0,
            to: CanvasNode::Graph(b),
            input: 0,
        };
        let mut state = State {
            edges: vec![edge(1, 2), edge(1, 3), edge(2, 4), edge(4, 2), edge(5, 6)],
            focus: Some(Focus::Edge(edge(1, 2))),
            ..Default::default()
        };
        recompute(&mut state);
        assert_eq!(state.bright, BTreeSet::from([edge(1, 2)]));
        state.focus = Some(Focus::Node(CanvasNode::Graph(1)));
        recompute(&mut state);
        assert_eq!(state.bright.len(), 2);
        state.trace = Trace::Outputs;
        recompute(&mut state);
        assert_eq!(state.bright.len(), 4);
        assert!(!state.bright.contains(&edge(5, 6)));
    }

    #[test]
    fn dragging_after_preview_invalidates_staged_positions() {
        let mut snarl = Snarl::new();
        let node = snarl.insert_node(egui::pos2(10.0, 20.0), CanvasNode::Graph(1));
        let mut state = State {
            preview: Some(Change {
                before: positions(&snarl),
                after: BTreeMap::from([(CanvasNode::Graph(1), egui::Pos2::ZERO)]),
            }),
            ..Default::default()
        };
        invalidate_moved_preview(&mut state, &snarl);
        assert!(state.preview.is_some());
        snarl.get_node_info_mut(node).unwrap().pos.x = 42.0;
        invalidate_moved_preview(&mut state, &snarl);
        assert!(state.preview.is_none());
        assert_eq!(snarl.get_node_info(node).unwrap().pos.x, 42.0);
    }
}
