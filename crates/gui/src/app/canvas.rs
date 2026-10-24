//! Mapping-project to canvas-node projection.

use egui_snarl::{InPinId, OutPinId, Snarl};
use mapping::{Node, NodeId, Project, Scope, UserFunction};

use super::{CanvasLayout, CanvasNode, PersistedCanvasNode};
#[cfg(test)]
use crate::canvas::source_blocks;
use crate::canvas::{
    SourceBlock, TargetBlock, project_source_blocks, source_conflicts, target_blocks,
};
use crate::canvas_endpoints::EndpointScrollState;
use crate::canvas_layout::arrange_snarl;

fn node_inputs(node: &Node) -> Vec<NodeId> {
    node.evaluated_inputs()
}

/// Builds the isolated graph canvas for one user-defined function.
pub(super) fn build_function_snarl(function: &UserFunction) -> Snarl<CanvasNode> {
    let mut snarl = Snarl::new();
    let mut snarl_ids = std::collections::BTreeMap::new();
    for (&id, node) in &function.body.nodes {
        if matches!(node, Node::Unconnected) {
            continue;
        }
        let snarl_id = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(id));
        snarl_ids.insert(id, snarl_id);
    }
    for (&id, node) in &function.body.nodes {
        let Some(&to_node) = snarl_ids.get(&id) else {
            continue;
        };
        for (input, dependency) in node_inputs(node).into_iter().enumerate() {
            let Some(&from_node) = snarl_ids.get(&dependency) else {
                continue;
            };
            snarl.connect(
                OutPinId {
                    node: from_node,
                    output: 0,
                },
                InPinId {
                    node: to_node,
                    input,
                },
            );
        }
    }
    arrange_snarl(
        &mut snarl,
        &std::collections::BTreeMap::new(),
        crate::appearance::WireAppearance::default(),
    );
    snarl
}

fn source_pin_for_field(
    blocks: &[SourceBlock],
    frame: &Option<Vec<String>>,
    path: &[String],
    unavailable_blocks: &std::collections::BTreeSet<usize>,
) -> Option<(usize, usize)> {
    let exact = blocks.iter().enumerate().find_map(|(block, section)| {
        if unavailable_blocks.contains(&block) {
            return None;
        }
        section
            .leaves
            .iter()
            .position(|leaf| &leaf.frame == frame && leaf.path == path)
            .map(|pin| (block, pin))
    });
    if exact.is_some() || frame.is_some() {
        return exact;
    }

    // Legacy unframed paths belonged to the primary catalog before named
    // endpoints existed. Additional catalogs must not redirect those nodes.
    let mut legacy_matches = blocks
        .iter()
        .take(1)
        .enumerate()
        .filter(|(block, _)| !unavailable_blocks.contains(block))
        .flat_map(|(block, section)| {
            section
                .leaves
                .iter()
                .enumerate()
                .filter(|(_, leaf)| leaf.path == path)
                .map(move |(pin, _)| (block, pin))
        });
    let first = legacy_matches.next()?;
    legacy_matches.next().is_none().then_some(first)
}

fn target_pin_for_binding(
    blocks: &[TargetBlock],
    chain: &[String],
    field: &str,
) -> Option<(usize, usize)> {
    blocks.iter().enumerate().find_map(|(block, section)| {
        section
            .leaves
            .iter()
            .position(|leaf| leaf.chain == chain && leaf.field == field)
            .map(|pin| (block, pin))
    })
}

/// Collects the block-local target pin for every binding while walking the
/// scope tree with its target-field chain.
fn walk_scopes(
    scope: &Scope,
    chain: &mut Vec<String>,
    target_blocks: &[TargetBlock],
    out: &mut Vec<(NodeId, usize, usize)>,
) {
    for binding in &scope.bindings {
        if let Some((block, pin)) =
            target_pin_for_binding(target_blocks, chain, &binding.target_field)
        {
            out.push((binding.node, block, pin));
        }
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            walk_scopes(segment, chain, target_blocks, out);
        }
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        walk_scopes(child, chain, target_blocks, out);
        chain.pop();
    }
}

/// Rebuilds the canvas from a project, recreating graph and scope-binding wires.
pub(super) fn build_snarl(project: &Project) -> Snarl<CanvasNode> {
    build_snarl_with_layout(project, None)
}

pub(super) fn build_snarl_with_layout(
    project: &Project,
    saved_layout: Option<&CanvasLayout>,
) -> Snarl<CanvasNode> {
    let saved_layout = saved_layout.filter(|layout| layout.matches_project(project));
    build_boundary_snarl(
        project,
        &project.target,
        &project.root,
        saved_layout.map(|layout| layout.nodes.as_slice()),
    )
}

pub(super) fn build_named_target_snarl(project: &Project, target: usize) -> Snarl<CanvasNode> {
    let Some(target) = project.extra_targets.get(target) else {
        return Snarl::new();
    };
    build_boundary_snarl(project, &target.schema, &target.root, None)
}

fn build_boundary_snarl(
    project: &Project,
    target_schema: &ir::SchemaNode,
    root_scope: &Scope,
    saved_nodes: Option<&[super::CanvasNodeLayout]>,
) -> Snarl<CanvasNode> {
    let source_blocks = project_source_blocks(project);
    let unavailable_blocks = source_conflicts(&project.source, &project.extra_sources);
    let target_blocks = target_blocks(target_schema);
    let owned_items = crate::graph_viewer::project_sequence_item_ids(project);

    let mut snarl = Snarl::new();
    for block in 0..source_blocks.len() {
        snarl.insert_node(egui::Pos2::ZERO, CanvasNode::SourceBlock(block));
    }

    // Exact frame identity distinguishes equal leaf paths in sibling collections.
    let hidden: std::collections::BTreeMap<NodeId, (usize, usize)> = project
        .graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| match node {
            Node::SourceField { path, frame } if !owned_items.contains(&id) => {
                source_pin_for_field(&source_blocks, frame, path, &unavailable_blocks)
                    .map(|pin| (id, pin))
            }
            _ => None,
        })
        .collect();

    let placeholders: std::collections::BTreeSet<NodeId> = saved_nodes
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry.node {
            PersistedCanvasNode::Placeholder { id }
                if matches!(
                    project.graph.nodes.get(&id),
                    Some(Node::Const {
                        value: ir::Value::Null
                    })
                ) =>
            {
                Some(id)
            }
            _ => None,
        })
        .collect();
    let mut snarl_ids = std::collections::BTreeMap::new();
    for (&id, node) in &project.graph.nodes {
        if hidden.contains_key(&id) || matches!(node, Node::Unconnected) {
            continue;
        }
        let snarl_id = snarl.insert_node(
            egui::Pos2::ZERO,
            if placeholders.contains(&id) {
                CanvasNode::Placeholder(id)
            } else {
                CanvasNode::Graph(id)
            },
        );
        snarl_ids.insert(id, snarl_id);
    }
    for block in 0..target_blocks.len() {
        snarl.insert_node(egui::Pos2::ZERO, CanvasNode::TargetBlock(block));
    }

    let out_pin_for = |id: NodeId| snarl_ids.get(&id).map(|&node| OutPinId { node, output: 0 });

    for (&id, node) in &project.graph.nodes {
        let Some(&to_node) = snarl_ids.get(&id) else {
            continue;
        };
        for (input, arg) in node_inputs(node).iter().enumerate() {
            if let Some(from) = out_pin_for(*arg) {
                snarl.connect(
                    from,
                    InPinId {
                        node: to_node,
                        input,
                    },
                );
            }
        }
    }

    sync_endpoint_wires_with_policy(
        &project.graph,
        root_scope,
        &source_blocks,
        &target_blocks,
        &EndpointScrollState::default(),
        &mut snarl,
        SourcePinPolicy {
            owned_items: &owned_items,
            unavailable_blocks: &unavailable_blocks,
        },
    );

    let mut initial_sizes = std::collections::BTreeMap::new();
    for (block, section) in source_blocks.iter().enumerate() {
        initial_sizes.insert(
            CanvasNode::SourceBlock(block),
            endpoint_block_size(&section.title, &section.pin_labels),
        );
    }
    for (block, section) in target_blocks.iter().enumerate() {
        initial_sizes.insert(
            CanvasNode::TargetBlock(block),
            endpoint_block_size(&section.title, &section.pin_labels),
        );
    }
    let wired = snarl
        .wires()
        .flat_map(|(from, to)| [from.node, to.node])
        .collect::<std::collections::BTreeSet<_>>();
    for (id, _, canvas) in snarl.nodes_pos_ids() {
        if !wired.contains(&id)
            && let CanvasNode::Graph(node) = canvas
            && let Some(value @ Node::Const { .. }) = project.graph.nodes.get(node)
        {
            // Before fonts exist, reserve a conservative bounded header width
            // for the disconnected shelf. Interactive layout uses measured
            // frames; saved sidecar positions are restored below unchanged.
            let width = (crate::graph_viewer::readable_node_label(value)
                .chars()
                .count() as f32
                * 14.0
                + 64.0)
                .clamp(100.0, 420.0);
            initial_sizes.insert(*canvas, egui::vec2(width, 88.0));
        }
    }
    arrange_snarl(
        &mut snarl,
        &initial_sizes,
        crate::appearance::WireAppearance::default(),
    );
    if let Some(nodes) = saved_nodes {
        CanvasLayout::apply_nodes(nodes, &mut snarl);
    }
    snarl
}

pub(crate) fn endpoint_block_size(title: &str, pin_labels: &[String]) -> egui::Vec2 {
    let label_chars = pin_labels
        .iter()
        .map(|label| label.chars().count().min(30))
        .chain([title.chars().count()])
        .max()
        .unwrap_or(0);
    // The first layout runs before fonts and actual node frames are available.
    // Reserve the full title/filter/count header at Standard density, both
    // possible proxy rows, and the footer. Saved positions are restored after
    // this initial placement; interactive compaction uses measured geometry.
    let density = crate::appearance::CanvasDensity::Standard;
    let row_pitch = density.font_size() + 6.0 + density.item_spacing().y;
    let visible = pin_labels
        .len()
        .min(crate::canvas_endpoints::VISIBLE_PIN_LIMIT);
    let proxy_rows = if pin_labels.len() > visible { 2 } else { 0 };
    let rows = 4 + visible + proxy_rows + 1;
    egui::vec2(
        (label_chars as f32 * 7.0 + 40.0).clamp(110.0, 230.0),
        rows as f32 * row_pitch + 16.0,
    )
}

/// Rebuilds one project canvas while migrating source positions by alias,
/// rather than lending a removed source's ordinal to its successor.
pub(super) fn rebuild_source_snarl(
    project: &Project,
    previous_names: &[String],
    previous: &Snarl<CanvasNode>,
    target: Option<usize>,
) -> Snarl<CanvasNode> {
    let nodes = CanvasLayout::capture_nodes(previous)
        .into_iter()
        .filter_map(|mut entry| {
            if let PersistedCanvasNode::Source { block } = &mut entry.node
                && *block != 0
            {
                let name = previous_names.get(*block - 1)?;
                *block = project
                    .extra_sources
                    .iter()
                    .position(|source| &source.name == name)?
                    + 1;
            }
            Some(entry)
        })
        .collect::<Vec<_>>();
    match target {
        Some(index) => project
            .extra_targets
            .get(index)
            .map_or_else(Snarl::new, |target| {
                build_boundary_snarl(project, &target.schema, &target.root, Some(&nodes))
            }),
        None => build_boundary_snarl(project, &project.target, &project.root, Some(&nodes)),
    }
}

pub(crate) struct SourcePinPolicy<'a> {
    pub(crate) owned_items: &'a std::collections::BTreeSet<NodeId>,
    pub(crate) unavailable_blocks: &'a std::collections::BTreeSet<usize>,
}

/// Reconciles the visual endpoint wires with the graph for the currently
/// visible source and target field windows. Mapping nodes remain the source
/// of truth, so scrolling never changes a binding.
#[cfg(test)]
pub(crate) fn sync_endpoint_wires(
    graph: &mapping::Graph,
    root_scope: &Scope,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    scroll: &EndpointScrollState,
    snarl: &mut Snarl<CanvasNode>,
) {
    let owned_items =
        crate::graph_viewer::sequence_item_ids(graph, root_scope, &[], &[], Default::default());
    sync_endpoint_wires_with_owned_items(
        graph,
        root_scope,
        source_blocks,
        target_blocks,
        scroll,
        snarl,
        &owned_items,
    );
}

/// Project-aware synchronization retains generated item identity even when
/// malformed imported source paths happen to match physical source leaves.
pub(crate) fn sync_endpoint_wires_with_owned_items(
    graph: &mapping::Graph,
    root_scope: &Scope,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    scroll: &EndpointScrollState,
    snarl: &mut Snarl<CanvasNode>,
    owned_items: &std::collections::BTreeSet<NodeId>,
) {
    sync_endpoint_wires_with_policy(
        graph,
        root_scope,
        source_blocks,
        target_blocks,
        scroll,
        snarl,
        SourcePinPolicy {
            owned_items,
            unavailable_blocks: &Default::default(),
        },
    );
}

/// Canonical wires use original schema indices for endpoints, independent of
/// scrolling, filtering and search. Both membership and visual routing derive
/// from this projection so a filtered view cannot hide an actual connection.
pub(crate) fn semantic_canvas_wires(
    graph: &mapping::Graph,
    root_scope: &Scope,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    snarl: &Snarl<CanvasNode>,
    policy: SourcePinPolicy<'_>,
) -> std::collections::BTreeSet<(OutPinId, InPinId)> {
    let mut source_nodes = std::collections::BTreeMap::new();
    let mut target_nodes = std::collections::BTreeMap::new();
    let mut graph_nodes = std::collections::BTreeMap::new();
    for (id, _, node) in snarl.nodes_pos_ids() {
        match *node {
            CanvasNode::SourceBlock(block) => {
                source_nodes.insert(block, id);
            }
            CanvasNode::TargetBlock(block) => {
                target_nodes.insert(block, id);
            }
            CanvasNode::Graph(mapping) | CanvasNode::Placeholder(mapping) => {
                graph_nodes.insert(mapping, id);
            }
        }
    }

    let source_fields = graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| match node {
            Node::SourceField { path, frame } if !policy.owned_items.contains(&id) => {
                source_pin_for_field(source_blocks, frame, path, policy.unavailable_blocks)
                    .map(|pin| (id, pin))
            }
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let semantic_output = |id: NodeId| -> Option<OutPinId> {
        if let Some(&(block, semantic_pin)) = source_fields.get(&id) {
            Some(OutPinId {
                node: *source_nodes.get(&block)?,
                output: semantic_pin,
            })
        } else {
            Some(OutPinId {
                node: *graph_nodes.get(&id)?,
                output: 0,
            })
        }
    };

    let mut expected = std::collections::BTreeSet::new();
    for (&id, node) in &graph.nodes {
        let Some(&to_node) = graph_nodes.get(&id) else {
            continue;
        };
        for (input, argument) in node_inputs(node).into_iter().enumerate() {
            if let Some(from) = semantic_output(argument) {
                expected.insert((
                    from,
                    InPinId {
                        node: to_node,
                        input,
                    },
                ));
            }
        }
    }

    let mut bindings = Vec::new();
    walk_scopes(root_scope, &mut Vec::new(), target_blocks, &mut bindings);
    for (node_id, block, semantic_pin) in bindings {
        if let (Some(from), Some(&to_node)) = (semantic_output(node_id), target_nodes.get(&block)) {
            expected.insert((
                from,
                InPinId {
                    node: to_node,
                    input: semantic_pin,
                },
            ));
        }
    }

    expected
}

pub(crate) fn update_endpoint_connections_with_policy(
    graph: &mapping::Graph,
    root_scope: &Scope,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    scroll: &mut EndpointScrollState,
    snarl: &Snarl<CanvasNode>,
    policy: SourcePinPolicy<'_>,
) -> bool {
    let mut connected = std::collections::BTreeMap::<CanvasNode, Vec<usize>>::new();
    for (from, to) in semantic_canvas_wires(
        graph,
        root_scope,
        source_blocks,
        target_blocks,
        snarl,
        policy,
    ) {
        if let Some(&CanvasNode::SourceBlock(block)) = snarl.get_node(from.node) {
            connected
                .entry(CanvasNode::SourceBlock(block))
                .or_default()
                .push(from.output);
        }
        if let Some(&CanvasNode::TargetBlock(block)) = snarl.get_node(to.node) {
            connected
                .entry(CanvasNode::TargetBlock(block))
                .or_default()
                .push(to.input);
        }
    }
    let mut changed = false;
    for (_, _, &node) in snarl.nodes_pos_ids() {
        let total = match node {
            CanvasNode::SourceBlock(block) => {
                source_blocks.get(block).map(|block| block.leaves.len())
            }
            CanvasNode::TargetBlock(block) => {
                target_blocks.get(block).map(|block| block.leaves.len())
            }
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => None,
        };
        if let Some(total) = total {
            changed |=
                scroll.set_connected(node, total, connected.remove(&node).unwrap_or_default());
        }
    }
    changed
}

pub(crate) fn sync_endpoint_wires_with_policy(
    graph: &mapping::Graph,
    root_scope: &Scope,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    scroll: &EndpointScrollState,
    snarl: &mut Snarl<CanvasNode>,
    policy: SourcePinPolicy<'_>,
) {
    let expected = semantic_canvas_wires(
        graph,
        root_scope,
        source_blocks,
        target_blocks,
        snarl,
        policy,
    )
    .into_iter()
    .filter_map(|(mut from, mut to)| {
        if let Some(&CanvasNode::SourceBlock(block)) = snarl.get_node(from.node) {
            from.output = scroll.visual_pin_for_semantic(
                CanvasNode::SourceBlock(block),
                from.output,
                source_blocks.get(block)?.leaves.len(),
            )?;
        }
        if let Some(&CanvasNode::TargetBlock(block)) = snarl.get_node(to.node) {
            to.input = scroll.visual_pin_for_semantic(
                CanvasNode::TargetBlock(block),
                to.input,
                target_blocks.get(block)?.leaves.len(),
            )?;
        }
        Some((from, to))
    })
    .collect::<std::collections::BTreeSet<_>>();

    let aggregate_inputs = snarl
        .nodes_pos_ids()
        .filter_map(|(node, _, &canvas_node)| {
            let id = match canvas_node {
                CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => id,
                CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_) => return None,
            };
            (matches!(graph.nodes.get(&id), Some(Node::Aggregate { .. }))
                || graph
                    .nodes
                    .get(&id)
                    .and_then(crate::filter_map_editor::sequence)
                    .is_some_and(|sequence| {
                        matches!(sequence, mapping::SequenceExpr::FilterMapV1(_))
                    }))
            .then_some(node)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let endpoint_nodes = snarl
        .nodes_pos_ids()
        .filter_map(|(id, _, node)| {
            matches!(
                node,
                CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_)
            )
            .then_some(id)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let existing = snarl.wires().collect::<std::collections::BTreeSet<_>>();
    for &(from, to) in &existing {
        if (endpoint_nodes.contains(&from.node)
            || endpoint_nodes.contains(&to.node)
            || aggregate_inputs.contains(&to.node))
            && !expected.contains(&(from, to))
        {
            snarl.disconnect(from, to);
        }
    }
    for (from, to) in expected {
        if !existing.contains(&(from, to)) {
            snarl.connect(from, to);
        }
    }
}

#[cfg(test)]
#[path = "canvas/sequence_item_tests.rs"]
mod sequence_item_tests;

#[cfg(test)]
#[path = "canvas/endpoint_filter_tests.rs"]
mod endpoint_filter_tests;

#[cfg(test)]
#[path = "canvas/visual_tests.rs"]
mod visual_tests;

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn replacing_a_document_never_reuses_layout_history_or_camera_ownership() {
        let first = super::super::CanvasDocumentState::with_snarl(Snarl::new());
        let second = super::super::CanvasDocumentState::with_snarl(Snarl::new());
        assert_ne!(first.tools_id, second.tools_id);
        assert_eq!(first.view_generation, second.view_generation);
    }
}
