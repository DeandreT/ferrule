//! Mapping-project to canvas-node projection.

use egui_snarl::{InPinId, OutPinId, Snarl};
use mapping::{Node, NodeId, Project, Scope};

use super::{CanvasLayout, CanvasNode, PersistedCanvasNode};
use crate::canvas::{SourceBlock, TargetBlock, source_blocks, target_blocks};
use crate::canvas_endpoints::EndpointScrollState;
use crate::canvas_layout::arrange_snarl;

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => vec![],
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![*input],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

fn source_pin_for_field(
    blocks: &[SourceBlock],
    frame: &Option<Vec<String>>,
    path: &[String],
) -> Option<(usize, usize)> {
    let exact = blocks.iter().enumerate().find_map(|(block, section)| {
        section
            .leaves
            .iter()
            .position(|leaf| &leaf.frame == frame && leaf.path == path)
            .map(|pin| (block, pin))
    });
    if exact.is_some() || frame.is_some() {
        return exact;
    }

    let mut legacy_matches = blocks.iter().enumerate().flat_map(|(block, section)| {
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
    let source_blocks = source_blocks(&project.source);
    let target_blocks = target_blocks(&project.target);

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
            Node::SourceField { path, frame } => {
                source_pin_for_field(&source_blocks, frame, path).map(|pin| (id, pin))
            }
            _ => None,
        })
        .collect();

    let placeholders: std::collections::BTreeSet<NodeId> = saved_layout
        .into_iter()
        .flat_map(|layout| &layout.nodes)
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
    for &id in project
        .graph
        .nodes
        .keys()
        .filter(|id| !hidden.contains_key(id))
    {
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

    sync_endpoint_wires(
        &project.graph,
        &project.root,
        &source_blocks,
        &target_blocks,
        &EndpointScrollState::default(),
        &mut snarl,
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
    arrange_snarl(
        &mut snarl,
        &initial_sizes,
        crate::appearance::WireAppearance::default(),
    );
    if let Some(layout) = saved_layout {
        layout.apply(&mut snarl);
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
    egui::vec2(
        (label_chars as f32 * 7.0 + 40.0).clamp(110.0, 230.0),
        (34.0
            + pin_labels
                .len()
                .min(crate::canvas_endpoints::VISIBLE_PIN_LIMIT) as f32
                * 20.0)
            .max(58.0),
    )
}

/// Reconciles the visual endpoint wires with the graph for the currently
/// visible source and target field windows. Mapping nodes remain the source
/// of truth, so scrolling never changes a binding.
pub(crate) fn sync_endpoint_wires(
    graph: &mapping::Graph,
    root_scope: &Scope,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    scroll: &EndpointScrollState,
    snarl: &mut Snarl<CanvasNode>,
) {
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
            Node::SourceField { path, frame } => {
                source_pin_for_field(source_blocks, frame, path).map(|pin| (id, pin))
            }
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let visible_output = |id: NodeId| -> Option<OutPinId> {
        if let Some(&(block, semantic_pin)) = source_fields.get(&id) {
            let section = source_blocks.get(block)?;
            let output = scroll.displayed_pin(
                CanvasNode::SourceBlock(block),
                semantic_pin,
                section.leaves.len(),
            )?;
            Some(OutPinId {
                node: *source_nodes.get(&block)?,
                output,
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
            if let Some(from) = visible_output(argument) {
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
        let Some(section) = target_blocks.get(block) else {
            continue;
        };
        let Some(input) = scroll.displayed_pin(
            CanvasNode::TargetBlock(block),
            semantic_pin,
            section.leaves.len(),
        ) else {
            continue;
        };
        if let (Some(from), Some(&to_node)) = (visible_output(node_id), target_nodes.get(&block)) {
            expected.insert((
                from,
                InPinId {
                    node: to_node,
                    input,
                },
            ));
        }
    }

    let endpoint_nodes = source_nodes
        .values()
        .chain(target_nodes.values())
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let existing = snarl.wires().collect::<std::collections::BTreeSet<_>>();
    for &(from, to) in &existing {
        if (endpoint_nodes.contains(&from.node) || endpoint_nodes.contains(&to.node))
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
