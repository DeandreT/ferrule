//! The inspector and bright wires are two views of the same semantic edges.

use super::{Edge, State, edit, recompute};
use crate::canvas::CanvasNode;
use crate::canvas_search::{FieldPin, Fields};
use crate::graph_viewer::GraphViewer;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(crate) struct FieldWiring {
    pub(crate) pin: FieldPin,
    pub(crate) chain: Vec<String>,
    pub(crate) field: String,
    pub(crate) label: String,
    pub(crate) connections: Vec<(Edge, String)>,
}

/// Walk producer inputs only. An endpoint may represent hundreds of fields;
/// only edges into the chosen target port seed the walk. Postorder shows each
/// dependency before its consumer, even when inputs share a producer.
fn upstream(edges: &[Edge], pin: FieldPin) -> Vec<Edge> {
    let mut inputs = BTreeMap::<CanvasNode, Vec<Edge>>::new();
    for &edge in edges {
        inputs.entry(edge.to).or_default().push(edge);
    }
    for incoming in inputs.values_mut() {
        incoming.sort_by_key(|e| (e.input, e.from, e.output));
    }
    let mut pending: Vec<_> = inputs
        .get(&pin.node)
        .into_iter()
        .flatten()
        .rev()
        .filter(|e| e.input == pin.semantic)
        .map(|&e| (e, false))
        .collect();
    let mut visited = BTreeSet::new();
    let mut expanded_nodes = BTreeSet::new();
    let mut ordered = Vec::new();
    while let Some((edge, expanded)) = pending.pop() {
        if expanded {
            ordered.push(edge);
        } else if visited.insert(edge) {
            pending.push((edge, true));
            if expanded_nodes.insert(edge.from)
                && let Some(dependencies) = inputs.get(&edge.from)
            {
                pending.extend(dependencies.iter().rev().map(|&e| (e, false)));
            }
        }
    }
    ordered
}

fn port_label(viewer: &GraphViewer<'_>, node: CanvasNode, index: usize, input: bool) -> String {
    if let Some(label) = Fields::of(viewer).label(FieldPin {
        node,
        semantic: index,
    }) {
        return label;
    }
    let id = match node {
        CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => id,
        _ => return "Missing field".into(),
    };
    let title = match viewer.graph.nodes.get(&id) {
        Some(mapping::Node::Call { function, .. }) => functions::builtin(function).map_or_else(
            || function.clone(),
            |builtin| format!("{} ({function})", builtin.display_name),
        ),
        Some(mapping::Node::UserFunctionCall { function, .. }) => viewer
            .function_names
            .get(function)
            .cloned()
            .unwrap_or_else(|| format!("Function #{}", function.get())),
        Some(node) => crate::graph_viewer::readable_node_label(node),
        None => "Missing node".into(),
    };
    format!(
        "{title} #{id} · {} {}",
        if input { "input" } else { "output" },
        index + 1
    )
}

fn connection_label(viewer: &GraphViewer<'_>, edge: Edge) -> String {
    let from = port_label(viewer, edge.from, edge.output, false);
    let to = port_label(viewer, edge.to, edge.input, true);
    format!("{from}  →  {to}")
}

impl FieldWiring {
    fn for_pin(viewer: &GraphViewer<'_>, pin: FieldPin, edges: &[Edge]) -> Option<Self> {
        let CanvasNode::TargetBlock(i) = pin.node else {
            return None;
        };
        let leaf = viewer.target_blocks.get(i)?.leaves.get(pin.semantic)?;
        let connections = upstream(edges, pin)
            .into_iter()
            .map(|edge| (edge, connection_label(viewer, edge)))
            .collect();
        Some(Self {
            pin,
            chain: leaf.chain.clone(),
            field: leaf.field.clone(),
            label: Fields::of(viewer).label(pin)?,
            connections,
        })
    }
}

/// Resolve the saved identity against the current catalog, including after a
/// schema edit. Never silently transfer the inspector to the next field.
pub(super) fn refresh(state: &mut State, viewer: &GraphViewer<'_>, connections_changed: bool) {
    let Some(previous) = &mut state.wiring else {
        return;
    };
    if let CanvasNode::TargetBlock(i) = previous.pin.node
        && !connections_changed
        && viewer
            .target_blocks
            .get(i)
            .and_then(|b| b.leaves.get(previous.pin.semantic))
            .is_some_and(|leaf| leaf.chain == previous.chain && leaf.field == previous.field)
    {
        // Editing a literal or renaming a function needn't change its wires.
        if let Some(label) = Fields::of(viewer).label(previous.pin) {
            previous.label = label;
        }
        for (edge, label) in &mut previous.connections {
            *label = connection_label(viewer, *edge);
        }
        return;
    }
    let pin = viewer
        .target_blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.chain == previous.chain)
        .find_map(|(i, block)| {
            let semantic = block
                .leaves
                .iter()
                .position(|leaf| leaf.field == previous.field)?;
            Some(FieldPin {
                node: CanvasNode::TargetBlock(i),
                semantic,
            })
        });
    state.wiring = pin.and_then(|p| FieldWiring::for_pin(viewer, p, &state.edges));
}

pub(crate) fn select_field(ctx: &egui::Context, viewer: &GraphViewer<'_>, pin: FieldPin) {
    edit(ctx, |state| {
        state.wiring = FieldWiring::for_pin(viewer, pin, &state.edges);
        recompute(state);
    });
    ctx.request_repaint();
}

/// The inspector can be drawn before the canvas, or next to a different
/// function canvas. Look up the owner explicitly, never the last-painted one.
pub(crate) fn field_wiring(ctx: &egui::Context, tools_id: egui::Id) -> Option<FieldWiring> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<State>(tools_id.with("canvas_tools"))
            .wiring
            .clone()
    })
}

pub(crate) fn active_field(ctx: &egui::Context) -> Option<FieldPin> {
    super::read(ctx, |state| state.wiring.as_ref().map(|wiring| wiring.pin)).flatten()
}

pub(crate) fn dismiss_wiring(ctx: &egui::Context, tools_id: egui::Id) {
    ctx.data_mut(|data| {
        let id = tools_id.with("canvas_tools");
        data.insert_temp(id.with("restore_field_view"), true);
        let state = data.get_temp_mut_or_default::<State>(id);
        state.wiring = None;
        recompute(state);
    });
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_wiring_dismiss_restores_the_previous_connection_focus_on_its_own_canvas() {
        let ctx = egui::Context::default();
        let owner = egui::Id::unique("primary");
        let other = egui::Id::unique("other");
        let old = Edge {
            from: CanvasNode::SourceBlock(0),
            output: 0,
            to: CanvasNode::TargetBlock(0),
            input: 0,
        };
        let traced = Edge {
            input: 1,
            output: 1,
            ..old
        };
        let mut state = State {
            focus: Some(super::super::Focus::Edge(old)),
            edges: vec![old, traced],
            wiring: Some(FieldWiring {
                pin: FieldPin {
                    node: traced.to,
                    semantic: traced.input,
                },
                chain: vec![],
                field: "Desc".into(),
                label: "row.Desc".into(),
                connections: vec![(traced, "source → target".into())],
            }),
            ..Default::default()
        };
        recompute(&mut state);
        assert_eq!(state.bright, BTreeSet::from([traced]));
        ctx.data_mut(|data| data.insert_temp(owner.with("canvas_tools"), state));
        assert!(field_wiring(&ctx, other).is_none());
        dismiss_wiring(&ctx, owner);
        let restored = ctx
            .data(|data| data.get_temp::<State>(owner.with("canvas_tools")))
            .unwrap();
        assert!(restored.wiring.is_none());
        assert_eq!(restored.focus, Some(super::super::Focus::Edge(old)));
        assert_eq!(restored.bright, BTreeSet::from([old]));
    }

    #[test]
    fn field_wiring_walks_shared_inputs_once_and_excludes_other_target_ports() {
        let source = CanvasNode::SourceBlock(0);
        let upper = CanvasNode::Graph(4);
        let combine = CanvasNode::Graph(5);
        let target = CanvasNode::TargetBlock(0);
        let edge = |from, output, to, input| Edge {
            from,
            output,
            to,
            input,
        };
        let read = edge(source, 7, upper, 0);
        let left = edge(upper, 0, combine, 0);
        let right = edge(upper, 0, combine, 1);
        let write = edge(combine, 0, target, 3);
        let unrelated = edge(upper, 0, target, 1);
        let edges = [
            unrelated,
            right,
            write,
            left,
            read,
            edge(source, 1, target, 0),
        ];
        assert_eq!(
            upstream(
                &edges,
                FieldPin {
                    node: target,
                    semantic: 3
                }
            ),
            [read, left, right, write]
        );
        assert_eq!(
            upstream(
                &edges,
                FieldPin {
                    node: target,
                    semantic: 1
                }
            ),
            [read, unrelated]
        );
        assert!(
            upstream(
                &edges,
                FieldPin {
                    node: target,
                    semantic: 2
                }
            )
            .is_empty()
        );
    }
}
