//! Deterministic, wire-aware placement for the mapping canvas.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Pos2, Vec2, pos2, vec2};
use egui_snarl::{NodeId as SnarlNodeId, Snarl};

use crate::appearance::{WireAppearance, WireFrameAdjustment, WireGeometry};
use crate::canvas::CanvasNode;

const PIN_TOP: f32 = 36.0;
const PIN_PITCH: f32 = 22.0;
const ENDPOINT_PIN_PITCH: f32 = 22.0;
const NODE_GAP: f32 = 24.0;
const ENDPOINT_BLOCK_GAP: f32 = 8.0;
const SWEEP_COUNT: usize = 6;

#[derive(Clone, Copy)]
struct Edge {
    from: SnarlNodeId,
    to: SnarlNodeId,
    output: usize,
    input: usize,
}

#[derive(Clone, Copy)]
struct EndpointGeometry<'a> {
    source_positions: &'a BTreeMap<SnarlNodeId, f32>,
    target_positions: &'a BTreeMap<SnarlNodeId, f32>,
    source_extent: f32,
    target_extent: f32,
}

/// Repositions existing nodes without rebuilding the snarl. Node identity,
/// open state, selection state, and wires therefore remain untouched.
pub fn arrange_snarl(
    snarl: &mut Snarl<CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
    wire: WireAppearance,
) {
    let positions = layout_positions(snarl, measured_sizes, wire);
    for (node, next) in positions {
        if let Some(info) = snarl.get_node_info_mut(node) {
            info.pos = next;
        }
    }
}

fn layout_positions(
    snarl: &Snarl<CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
    wire: WireAppearance,
) -> BTreeMap<SnarlNodeId, Pos2> {
    let semantics: BTreeMap<_, _> = snarl
        .nodes_pos_ids()
        .map(|(id, _, node)| (id, *node))
        .collect();
    let source_positions = stacked_endpoint_positions(&semantics, measured_sizes, |node| {
        matches!(node, CanvasNode::SourceBlock(_))
    });
    let target_positions = stacked_endpoint_positions(&semantics, measured_sizes, |node| {
        matches!(node, CanvasNode::TargetBlock(_))
    });
    let source_extent = endpoint_extent(&source_positions, &semantics, measured_sizes);
    let target_extent = endpoint_extent(&target_positions, &semantics, measured_sizes);
    let source_nodes = semantics
        .iter()
        .filter_map(|(&id, node)| matches!(node, CanvasNode::SourceBlock(_)).then_some(id))
        .collect::<BTreeSet<_>>();
    let graph_nodes: BTreeSet<_> = semantics
        .iter()
        .filter_map(|(&id, node)| {
            matches!(node, CanvasNode::Graph(_) | CanvasNode::Placeholder(_)).then_some(id)
        })
        .collect();
    let edges: Vec<_> = snarl
        .wires()
        .map(|(from, to)| Edge {
            from: from.node,
            to: to.node,
            output: from.output,
            input: to.input,
        })
        .collect();

    let mut depths = BTreeMap::new();
    for &node in &graph_nodes {
        let mut visiting = BTreeSet::new();
        depth_of(node, &graph_nodes, &edges, &mut depths, &mut visiting);
    }
    let mut columns: BTreeMap<usize, Vec<SnarlNodeId>> = BTreeMap::new();
    for (&node, &depth) in &depths {
        columns.entry(depth).or_default().push(node);
    }

    let mut upstream = BTreeMap::new();
    let mut downstream = BTreeMap::new();
    let mut desired = BTreeMap::new();
    for &node in &graph_nodes {
        let before = endpoint_anchor(
            node,
            Direction::Upstream,
            &source_positions,
            &target_positions,
            &graph_nodes,
            &edges,
            &mut upstream,
            &mut BTreeSet::new(),
        );
        let after = endpoint_anchor(
            node,
            Direction::Downstream,
            &source_positions,
            &target_positions,
            &graph_nodes,
            &edges,
            &mut downstream,
            &mut BTreeSet::new(),
        );
        let center = match (before, after) {
            (Some(before), Some(after)) => (before + after) / 2.0,
            (Some(anchor), None) | (None, Some(anchor)) => anchor,
            (None, None) => PIN_TOP,
        };
        desired.insert(node, center);
    }

    for nodes in columns.values_mut() {
        nodes.sort_by(|a, b| {
            desired[a]
                .total_cmp(&desired[b])
                .then_with(|| semantics[a].cmp(&semantics[b]))
        });
    }
    reduce_crossings(
        &mut columns,
        &semantics,
        &graph_nodes,
        &edges,
        EndpointGeometry {
            source_positions: &source_positions,
            target_positions: &target_positions,
            source_extent,
            target_extent,
        },
    );

    let packed_y: BTreeMap<_, _> = columns
        .values()
        .flat_map(|nodes| pack_column(nodes, &semantics, measured_sizes, &desired))
        .collect();
    let channel = adaptive_routing_channel(
        routing_channel(wire),
        wire,
        &edges,
        &source_positions,
        &target_positions,
        &depths,
        &semantics,
        measured_sizes,
        &packed_y,
    );
    let source_width = source_nodes
        .iter()
        .filter_map(|id| semantics.get(id))
        .map(|node| node_size(*node, measured_sizes).x)
        .fold(0.0, f32::max)
        .max(240.0);
    let mut column_x = BTreeMap::new();
    let mut next_x = source_width + channel;
    for (&column, nodes) in &columns {
        column_x.insert(column, next_x);
        let width = nodes
            .iter()
            .filter_map(|node| semantics.get(node))
            .map(|node| node_size(*node, measured_sizes).x)
            .fold(0.0, f32::max);
        next_x += width + channel;
    }

    let mut out = BTreeMap::new();
    for (&source, &y) in &source_positions {
        out.insert(source, pos2(0.0, y));
    }
    for (&target, &y) in &target_positions {
        out.insert(target, pos2(next_x, y));
    }
    for (&column, nodes) in &columns {
        let x = column_x[&column];
        for node in nodes {
            out.insert(*node, pos2(x, packed_y[node]));
        }
    }
    out
}

fn stacked_endpoint_positions(
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
    include: impl Fn(CanvasNode) -> bool,
) -> BTreeMap<SnarlNodeId, f32> {
    let mut endpoints = semantics
        .iter()
        .filter_map(|(&id, &node)| include(node).then_some((id, node)))
        .collect::<Vec<_>>();
    endpoints.sort_by_key(|(_, node)| *node);

    let mut y = 0.0;
    endpoints
        .into_iter()
        .map(|(id, node)| {
            let position = (id, y);
            y += node_size(node, measured_sizes).y + ENDPOINT_BLOCK_GAP;
            position
        })
        .collect()
}

fn endpoint_extent(
    positions: &BTreeMap<SnarlNodeId, f32>,
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
) -> f32 {
    positions
        .iter()
        .filter_map(|(id, y)| {
            semantics
                .get(id)
                .map(|node| y + node_size(*node, measured_sizes).y)
        })
        .fold(1.0, f32::max)
}

fn depth_of(
    node: SnarlNodeId,
    graph_nodes: &BTreeSet<SnarlNodeId>,
    edges: &[Edge],
    memo: &mut BTreeMap<SnarlNodeId, usize>,
    visiting: &mut BTreeSet<SnarlNodeId>,
) -> usize {
    if let Some(&depth) = memo.get(&node) {
        return depth;
    }
    if !visiting.insert(node) {
        return 0;
    }
    let depth = edges
        .iter()
        .filter(|edge| edge.to == node && graph_nodes.contains(&edge.from))
        .map(|edge| depth_of(edge.from, graph_nodes, edges, memo, visiting) + 1)
        .max()
        .unwrap_or(0);
    visiting.remove(&node);
    memo.insert(node, depth);
    depth
}

#[derive(Clone, Copy)]
enum Direction {
    Upstream,
    Downstream,
}

#[allow(clippy::too_many_arguments)]
fn endpoint_anchor(
    node: SnarlNodeId,
    direction: Direction,
    source_positions: &BTreeMap<SnarlNodeId, f32>,
    target_positions: &BTreeMap<SnarlNodeId, f32>,
    graph_nodes: &BTreeSet<SnarlNodeId>,
    edges: &[Edge],
    memo: &mut BTreeMap<SnarlNodeId, Option<f32>>,
    visiting: &mut BTreeSet<SnarlNodeId>,
) -> Option<f32> {
    if let Some(anchor) = memo.get(&node) {
        return *anchor;
    }
    if !visiting.insert(node) {
        return None;
    }
    let anchors: Vec<_> = edges
        .iter()
        .filter_map(|edge| match direction {
            Direction::Upstream if edge.to == node => {
                if let Some(y) = source_positions.get(&edge.from) {
                    Some(y + pin_center(edge.output))
                } else if graph_nodes.contains(&edge.from) {
                    endpoint_anchor(
                        edge.from,
                        direction,
                        source_positions,
                        target_positions,
                        graph_nodes,
                        edges,
                        memo,
                        visiting,
                    )
                } else {
                    None
                }
            }
            Direction::Downstream if edge.from == node => {
                if let Some(y) = target_positions.get(&edge.to) {
                    Some(y + pin_center(edge.input))
                } else if graph_nodes.contains(&edge.to) {
                    endpoint_anchor(
                        edge.to,
                        direction,
                        source_positions,
                        target_positions,
                        graph_nodes,
                        edges,
                        memo,
                        visiting,
                    )
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    visiting.remove(&node);
    let anchor = average(anchors.into_iter());
    memo.insert(node, anchor);
    anchor
}

fn reduce_crossings(
    columns: &mut BTreeMap<usize, Vec<SnarlNodeId>>,
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    graph_nodes: &BTreeSet<SnarlNodeId>,
    edges: &[Edge],
    endpoints: EndpointGeometry<'_>,
) {
    for _ in 0..SWEEP_COUNT {
        let ranks = normalized_ranks(columns);
        for nodes in columns.values_mut() {
            sort_by_neighbors(
                nodes,
                Direction::Upstream,
                semantics,
                graph_nodes,
                edges,
                endpoints,
                &ranks,
            );
        }
        let ranks = normalized_ranks(columns);
        for nodes in columns.values_mut().rev() {
            sort_by_neighbors(
                nodes,
                Direction::Downstream,
                semantics,
                graph_nodes,
                edges,
                endpoints,
                &ranks,
            );
        }
    }
}

fn normalized_ranks(columns: &BTreeMap<usize, Vec<SnarlNodeId>>) -> BTreeMap<SnarlNodeId, f32> {
    columns
        .values()
        .flat_map(|nodes| {
            let count = nodes.len().max(1) as f32;
            nodes
                .iter()
                .enumerate()
                .map(move |(index, &node)| (node, (index as f32 + 0.5) / count))
        })
        .collect()
}

fn sort_by_neighbors(
    nodes: &mut [SnarlNodeId],
    direction: Direction,
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    graph_nodes: &BTreeSet<SnarlNodeId>,
    edges: &[Edge],
    endpoints: EndpointGeometry<'_>,
    ranks: &BTreeMap<SnarlNodeId, f32>,
) {
    let previous: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(rank, &node)| (node, rank))
        .collect();
    nodes.sort_by(|a, b| {
        let a_rank = neighbor_rank(*a, direction, graph_nodes, edges, endpoints, ranks);
        let b_rank = neighbor_rank(*b, direction, graph_nodes, edges, endpoints, ranks);
        match (a_rank, b_rank) {
            (Some(a_rank), Some(b_rank)) => a_rank.total_cmp(&b_rank),
            _ => std::cmp::Ordering::Equal,
        }
        .then_with(|| previous[a].cmp(&previous[b]))
        .then_with(|| semantics[a].cmp(&semantics[b]))
    });
}

fn neighbor_rank(
    node: SnarlNodeId,
    direction: Direction,
    graph_nodes: &BTreeSet<SnarlNodeId>,
    edges: &[Edge],
    endpoints: EndpointGeometry<'_>,
    ranks: &BTreeMap<SnarlNodeId, f32>,
) -> Option<f32> {
    average(edges.iter().filter_map(|edge| match direction {
        Direction::Upstream if edge.to == node => {
            if let Some(y) = endpoints.source_positions.get(&edge.from) {
                Some((y + pin_center(edge.output)) / endpoints.source_extent)
            } else if graph_nodes.contains(&edge.from) {
                ranks.get(&edge.from).copied()
            } else {
                None
            }
        }
        Direction::Downstream if edge.from == node => {
            let target_rank = endpoints
                .target_positions
                .get(&edge.to)
                .map(|y| (y + pin_center(edge.input)) / endpoints.target_extent);
            let input_bias = target_rank.unwrap_or_default() * 0.02;
            if let Some(rank) = target_rank {
                Some(rank)
            } else if graph_nodes.contains(&edge.to) {
                ranks.get(&edge.to).map(|rank| rank + input_bias)
            } else {
                None
            }
        }
        _ => None,
    }))
}

fn pack_column(
    nodes: &[SnarlNodeId],
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
    desired: &BTreeMap<SnarlNodeId, f32>,
) -> Vec<(SnarlNodeId, f32)> {
    let mut packed = Vec::with_capacity(nodes.len());
    let mut previous_bottom = f32::NEG_INFINITY;
    for (index, &node) in nodes.iter().enumerate() {
        let height = node_size(semantics[&node], measured_sizes).y;
        let desired_center = desired[&node].max(PIN_TOP + index as f32 * PIN_PITCH);
        let y = (desired_center - height / 2.0).max(previous_bottom + NODE_GAP);
        packed.push((node, y));
        previous_bottom = y + height;
    }
    if let Some(min_y) = packed.iter().map(|(_, y)| *y).reduce(f32::min)
        && min_y < 0.0
    {
        for (_, y) in &mut packed {
            *y -= min_y;
        }
    }
    packed
}

fn node_size(node: CanvasNode, measured_sizes: &BTreeMap<CanvasNode, Vec2>) -> Vec2 {
    measured_sizes
        .get(&node)
        .copied()
        .filter(|size| size.x.is_finite() && size.y.is_finite() && size.x > 1.0 && size.y > 1.0)
        .unwrap_or_else(|| match node {
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_) => vec2(180.0, 140.0),
            CanvasNode::Graph(_) => vec2(180.0, 88.0),
            CanvasNode::Placeholder(_) => vec2(150.0, 64.0),
        })
}

fn routing_channel(wire: WireAppearance) -> f32 {
    let shape = match wire.geometry() {
        WireGeometry::Straight => 48.0,
        WireGeometry::Bezier3 | WireGeometry::Bezier5 => wire.frame_size() * 1.25 + 32.0,
        WireGeometry::Orthogonal { corner_radius } => {
            (wire.frame_size() * 1.25).max(corner_radius * 3.0) + 32.0
        }
    };
    shape + wire.width() * 2.0
}

#[allow(clippy::too_many_arguments)]
fn adaptive_routing_channel(
    base: f32,
    wire: WireAppearance,
    edges: &[Edge],
    source_positions: &BTreeMap<SnarlNodeId, f32>,
    target_positions: &BTreeMap<SnarlNodeId, f32>,
    depths: &BTreeMap<SnarlNodeId, usize>,
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
    packed_y: &BTreeMap<SnarlNodeId, f32>,
) -> f32 {
    if matches!(wire.geometry(), WireGeometry::Straight)
        || !matches!(
            wire.frame_adjustment(),
            WireFrameAdjustment::UpscaleDistant | WireFrameAdjustment::Adaptive
        )
    {
        return base;
    }
    let target_stage = depths.values().copied().max().unwrap_or(0) + 2;
    edges
        .iter()
        .filter_map(|edge| {
            let from_y = if let Some(y) = source_positions.get(&edge.from) {
                y + pin_center(edge.output)
            } else {
                node_center(edge.from, semantics, measured_sizes, packed_y)?
            };
            let to_y = if let Some(y) = target_positions.get(&edge.to) {
                y + pin_center(edge.input)
            } else {
                node_center(edge.to, semantics, measured_sizes, packed_y)?
            };
            let from_stage = if source_positions.contains_key(&edge.from) {
                0
            } else {
                depths.get(&edge.from).copied()? + 1
            };
            let to_stage = if target_positions.contains_key(&edge.to) {
                target_stage
            } else {
                depths.get(&edge.to).copied()? + 1
            };
            let span = to_stage.saturating_sub(from_stage).max(1) as f32;
            Some((to_y - from_y).abs() / 8.0_f32.sqrt() / span + 32.0)
        })
        .fold(base, f32::max)
}

fn node_center(
    node: SnarlNodeId,
    semantics: &BTreeMap<SnarlNodeId, CanvasNode>,
    measured_sizes: &BTreeMap<CanvasNode, Vec2>,
    packed_y: &BTreeMap<SnarlNodeId, f32>,
) -> Option<f32> {
    let semantic = semantics.get(&node)?;
    Some(packed_y.get(&node)? + node_size(*semantic, measured_sizes).y / 2.0)
}

fn pin_center(pin: usize) -> f32 {
    PIN_TOP + pin as f32 * ENDPOINT_PIN_PITCH
}

fn average(values: impl Iterator<Item = f32>) -> Option<f32> {
    let (sum, count) = values.fold((0.0, 0usize), |(sum, count), value| {
        (sum + value, count + 1)
    });
    (count > 0).then_some(sum / count as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_snarl::{InPinId, OutPinId};

    fn connect(
        snarl: &mut Snarl<CanvasNode>,
        from: SnarlNodeId,
        output: usize,
        to: SnarlNodeId,
        input: usize,
    ) {
        snarl.connect(OutPinId { node: from, output }, InPinId { node: to, input });
    }

    #[test]
    fn arrange_preserves_identity_wires_and_open_state() {
        let mut snarl = Snarl::new();
        let source = snarl.insert_node(pos2(900.0, 500.0), CanvasNode::SourceBlock(0));
        let first = snarl.insert_node(pos2(10.0, 800.0), CanvasNode::Placeholder(10));
        let second = snarl.insert_node(pos2(20.0, 100.0), CanvasNode::Placeholder(11));
        let call = snarl.insert_node_collapsed(pos2(0.0, 0.0), CanvasNode::Graph(12));
        let target = snarl.insert_node(pos2(-100.0, 0.0), CanvasNode::TargetBlock(0));
        connect(&mut snarl, first, 0, call, 0);
        connect(&mut snarl, second, 0, call, 1);
        connect(&mut snarl, call, 0, target, 0);
        let wires_before: Vec<_> = snarl.wires().collect();

        arrange_snarl(&mut snarl, &BTreeMap::new(), WireAppearance::default());

        assert_eq!(snarl[source], CanvasNode::SourceBlock(0));
        assert_eq!(snarl[first], CanvasNode::Placeholder(10));
        assert_eq!(snarl[second], CanvasNode::Placeholder(11));
        assert_eq!(snarl[call], CanvasNode::Graph(12));
        assert!(!snarl.get_node_info(call).is_none_or(|node| node.open));
        assert_eq!(snarl.wires().collect::<Vec<_>>(), wires_before);
        assert!(snarl.get_node_info(first).is_some_and(|node| {
            snarl
                .get_node_info(second)
                .is_some_and(|second| node.pos.y < second.pos.y)
        }));
    }

    #[test]
    fn measured_nodes_are_packed_without_overlap() {
        let mut snarl = Snarl::new();
        let source = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
        let upper = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(1));
        let lower = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(2));
        let target = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::TargetBlock(0));
        connect(&mut snarl, source, 0, upper, 0);
        connect(&mut snarl, source, 1, lower, 0);
        connect(&mut snarl, upper, 0, target, 0);
        connect(&mut snarl, lower, 0, target, 1);
        let sizes = BTreeMap::from([
            (CanvasNode::Graph(1), vec2(260.0, 240.0)),
            (CanvasNode::Graph(2), vec2(180.0, 180.0)),
        ]);

        arrange_snarl(&mut snarl, &sizes, WireAppearance::default());

        let upper_pos = snarl.get_node_info(upper).map(|node| node.pos);
        let lower_pos = snarl.get_node_info(lower).map(|node| node.pos);
        assert!(
            matches!((upper_pos, lower_pos), (Some(upper), Some(lower)) if
            upper.y + 240.0 + NODE_GAP <= lower.y)
        );
        assert!(snarl.get_node_info(target).is_some_and(|target| {
            snarl
                .get_node_info(upper)
                .is_some_and(|upper| upper.pos.x + 260.0 < target.pos.x)
        }));
    }

    #[test]
    fn endpoint_blocks_stack_and_anchor_their_own_wires() {
        let mut snarl = Snarl::new();
        let first_source = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
        let second_source = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::SourceBlock(1));
        let upper = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(1));
        let lower = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(2));
        let first_target = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::TargetBlock(0));
        let second_target = snarl.insert_node(pos2(0.0, 0.0), CanvasNode::TargetBlock(1));
        connect(&mut snarl, first_source, 0, upper, 0);
        connect(&mut snarl, second_source, 0, lower, 0);
        connect(&mut snarl, upper, 0, first_target, 0);
        connect(&mut snarl, lower, 0, second_target, 0);
        let sizes = BTreeMap::from([
            (CanvasNode::SourceBlock(0), vec2(245.0, 120.0)),
            (CanvasNode::SourceBlock(1), vec2(245.0, 180.0)),
            (CanvasNode::TargetBlock(0), vec2(245.0, 100.0)),
            (CanvasNode::TargetBlock(1), vec2(245.0, 140.0)),
        ]);

        arrange_snarl(&mut snarl, &sizes, WireAppearance::default());

        let position = |node| snarl.get_node_info(node).map(|info| info.pos);
        assert!(matches!(
            (position(first_source), position(second_source)),
            (Some(first), Some(second)) if
                first.x == 0.0 && second.x == 0.0 &&
                first.y + 120.0 + ENDPOINT_BLOCK_GAP <= second.y
        ));
        assert!(matches!(
            (position(first_target), position(second_target)),
            (Some(first), Some(second)) if
                first.x == second.x && first.y + 100.0 + ENDPOINT_BLOCK_GAP <= second.y
        ));
        assert!(matches!(
            (position(upper), position(lower)),
            (Some(upper), Some(lower)) if upper.y < lower.y
        ));
    }

    #[test]
    fn wire_geometry_controls_the_routing_channel() {
        let mut straight = Snarl::new();
        let source = straight.insert_node(pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
        let call = straight.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(1));
        let target = straight.insert_node(pos2(0.0, 0.0), CanvasNode::TargetBlock(0));
        connect(&mut straight, source, 0, call, 0);
        connect(&mut straight, call, 0, target, 0);
        let mut curved = straight.clone();
        let straight_wire = WireAppearance::new(
            WireGeometry::Straight,
            2.0,
            80.0,
            crate::appearance::WireFrameAdjustment::Fixed,
        )
        .expect("straight appearance is valid");

        arrange_snarl(&mut straight, &BTreeMap::new(), straight_wire);
        arrange_snarl(&mut curved, &BTreeMap::new(), WireAppearance::default());

        let straight_target_x = straight.nodes_pos().find_map(|(position, node)| {
            (*node == CanvasNode::TargetBlock(0)).then_some(position.x)
        });
        let curved_target_x = curved.nodes_pos().find_map(|(position, node)| {
            (*node == CanvasNode::TargetBlock(0)).then_some(position.x)
        });
        assert!(
            matches!((straight_target_x, curved_target_x), (Some(straight), Some(curved)) if
            curved > straight)
        );
    }

    #[test]
    fn adaptive_frames_expand_for_large_vertical_spans() {
        let mut fixed = Snarl::new();
        let source = fixed.insert_node(pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
        let upper = fixed.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(1));
        let lower = fixed.insert_node(pos2(0.0, 0.0), CanvasNode::Graph(2));
        let target = fixed.insert_node(pos2(0.0, 0.0), CanvasNode::TargetBlock(0));
        connect(&mut fixed, source, 0, upper, 0);
        connect(&mut fixed, source, 1, lower, 0);
        connect(&mut fixed, upper, 0, target, 0);
        connect(&mut fixed, lower, 0, target, 1);
        let mut adaptive = fixed.clone();
        let sizes = BTreeMap::from([
            (CanvasNode::Graph(1), vec2(220.0, 700.0)),
            (CanvasNode::Graph(2), vec2(220.0, 700.0)),
        ]);
        let fixed_wire =
            WireAppearance::new(WireGeometry::Bezier5, 2.0, 80.0, WireFrameAdjustment::Fixed)
                .expect("fixed appearance is valid");
        let adaptive_wire = WireAppearance::new(
            WireGeometry::Bezier5,
            2.0,
            80.0,
            WireFrameAdjustment::Adaptive,
        )
        .expect("adaptive appearance is valid");

        arrange_snarl(&mut fixed, &sizes, fixed_wire);
        arrange_snarl(&mut adaptive, &sizes, adaptive_wire);

        let target_x = |snarl: &Snarl<CanvasNode>| {
            snarl.nodes_pos().find_map(|(position, node)| {
                (*node == CanvasNode::TargetBlock(0)).then_some(position.x)
            })
        };
        assert!(matches!((target_x(&fixed), target_x(&adaptive)),
            (Some(fixed), Some(adaptive)) if adaptive > fixed));
    }

    #[test]
    fn repeated_arrange_is_deterministic() {
        let mut snarl = Snarl::new();
        let source = snarl.insert_node(pos2(500.0, 500.0), CanvasNode::SourceBlock(0));
        let left = snarl.insert_node(pos2(400.0, 700.0), CanvasNode::Graph(8));
        let right = snarl.insert_node(pos2(300.0, 100.0), CanvasNode::Graph(2));
        let target = snarl.insert_node(pos2(200.0, 200.0), CanvasNode::TargetBlock(0));
        connect(&mut snarl, source, 1, left, 0);
        connect(&mut snarl, source, 0, right, 0);
        connect(&mut snarl, left, 0, target, 1);
        connect(&mut snarl, right, 0, target, 0);

        arrange_snarl(&mut snarl, &BTreeMap::new(), WireAppearance::default());
        let once: Vec<_> = snarl
            .nodes_pos_ids()
            .map(|(id, position, _)| (id, position))
            .collect();
        arrange_snarl(&mut snarl, &BTreeMap::new(), WireAppearance::default());
        let twice: Vec<_> = snarl
            .nodes_pos_ids()
            .map(|(id, position, _)| (id, position))
            .collect();

        assert_eq!(once, twice);
    }
}
