//! Private editor ownership, independent of rebuildable canvas geometry.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem::Discriminant,
};

use editor_ui::{GraphDocument, GraphPatch, TargetId};
use mapping::{Graph, Node, NodeId, Project};

use super::{FerruleApp, MappingDocument};
use crate::graph_draft_ui::{Cell, NodeOwner};

#[derive(Default)]
pub(super) struct GraphDraftLifetimes {
    document: Option<egui::Id>,
    canvases: BTreeMap<GraphDocument, CanvasLifetime>,
    context: Option<egui::Context>,
    live_owners: HashSet<egui::Id>,
}

struct CanvasLifetime {
    owner: egui::Id,
    nodes: BTreeMap<NodeId, NodeLifetime>,
}

struct NodeLifetime {
    kind: Discriminant<Node>,
    value_editor: bool,
    owner: egui::Id,
    cells: HashMap<Cell, SavedCell>,
}

struct SavedCell {
    value: ir::Value,
    owner: egui::Id,
}

fn fresh_owner() -> egui::Id {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    egui::Id::unique((
        "graph_draft_lifetime",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ))
}

impl GraphDraftLifetimes {
    fn reconcile(&mut self, document: egui::Id, project: &Project) {
        if self.document != Some(document) {
            self.canvases.clear();
            self.document = Some(document);
        }
        let graphs = std::iter::once((GraphDocument::Target(TargetId::Primary), &project.graph))
            .chain(project.extra_targets.iter().map(|target| {
                (
                    GraphDocument::Target(TargetId::Named(target.name.clone())),
                    &project.graph,
                )
            }))
            .chain(
                project
                    .user_functions
                    .iter()
                    .map(|(&id, function)| (GraphDocument::Function(id), &function.body)),
            )
            .collect::<BTreeMap<_, _>>();
        self.canvases
            .retain(|canvas, _| graphs.contains_key(canvas));
        for (canvas, graph) in graphs {
            self.canvases
                .entry(canvas)
                .or_insert_with(|| CanvasLifetime {
                    owner: document.with(fresh_owner()),
                    nodes: BTreeMap::new(),
                })
                .reconcile(graph);
        }
        self.clean_retired();
    }

    /// Consume each mutation, not just the end-of-frame graph. A deletion and
    /// an equal insertion can reuse an ID before there is another redraw.
    pub(super) fn record_patch(
        &mut self,
        document: &GraphDocument,
        patch: &GraphPatch,
        from_widgets: bool,
    ) {
        let parts = patch.project_parts();
        if let Some(change) = parts.named_targets() {
            for before in change.before() {
                if !change
                    .after()
                    .iter()
                    .any(|after| after.name == before.name && after.schema == before.schema)
                {
                    self.retire_canvas(&GraphDocument::Target(TargetId::Named(
                        before.name.clone(),
                    )));
                }
            }
        }
        for part in parts.parts() {
            if let editor_ui::ProjectPart::Function(id) = part {
                self.retire_canvas(&GraphDocument::Function(id));
            }
        }
        for (canvas, lifetime) in &mut self.canvases {
            let affected = match document {
                GraphDocument::Function(id) => canvas == &GraphDocument::Function(*id),
                GraphDocument::Target(_) | GraphDocument::Project => {
                    matches!(canvas, GraphDocument::Target(_))
                }
            };
            if !affected {
                continue;
            }
            for (id, change) in patch.nodes() {
                if change.before().is_none()
                    || change.after().is_none()
                    || change.before().map(std::mem::discriminant)
                        != change.after().map(std::mem::discriminant)
                {
                    lifetime.nodes.remove(&id);
                } else if let (Some(lifetime), Some(node)) =
                    (lifetime.nodes.get_mut(&id), change.after())
                {
                    // The active control already adopted its own edit. Keep its
                    // cursor/undo state, notably while typing ordinary text.
                    // Other canvases still invalidate their old stored values.
                    lifetime.reconcile_cells(node, from_widgets && canvas == document);
                }
            }
        }
        self.clean_retired();
    }

    pub(super) fn retire_canvas(&mut self, canvas: &GraphDocument) {
        self.canvases.remove(canvas);
        self.clean_retired();
    }

    fn clean_retired(&mut self) {
        let live = self
            .canvases
            .values()
            .flat_map(|canvas| {
                std::iter::once(canvas.owner).chain(canvas.nodes.values().flat_map(|node| {
                    std::iter::once(node.owner).chain(node.cells.values().map(|cell| cell.owner))
                }))
            })
            .collect::<HashSet<_>>();
        if let Some(context) = &self.context {
            for &owner in self.live_owners.difference(&live) {
                crate::graph_draft_ui::retire(context, owner);
            }
        }
        self.live_owners = live;
    }
}

impl Drop for GraphDraftLifetimes {
    fn drop(&mut self) {
        if let Some(context) = &self.context {
            for &owner in &self.live_owners {
                crate::graph_draft_ui::retire(context, owner);
            }
        }
    }
}

impl CanvasLifetime {
    fn reconcile(&mut self, graph: &Graph) {
        self.nodes.retain(|id, lifetime| {
            graph
                .nodes
                .get(id)
                .is_some_and(|node| std::mem::discriminant(node) == lifetime.kind)
        });
        for (&id, node) in &graph.nodes {
            let lifetime = self.nodes.entry(id).or_insert_with(|| NodeLifetime {
                kind: std::mem::discriminant(node),
                value_editor: matches!(node, Node::Const { .. } | Node::ValueMap { .. }),
                owner: self.owner.with(fresh_owner()),
                cells: HashMap::new(),
            });
            lifetime.reconcile_cells(node, false);
        }
    }
}

impl NodeLifetime {
    fn reconcile_cells(&mut self, node: &Node, from_widgets: bool) {
        let values: HashMap<_, _> = match node {
            Node::Const { value } => vec![(Cell::Constant, value)],
            Node::ValueMap { table, default, .. } => table
                .iter()
                .enumerate()
                .flat_map(|(index, (key, value))| {
                    [(Cell::Key(index), key), (Cell::Value(index), value)]
                })
                .chain(default.iter().map(|value| (Cell::Default, value)))
                .collect(),
            _ => Vec::new(),
        }
        .into_iter()
        .collect();
        self.cells.retain(|cell, _| values.contains_key(cell));
        for (cell, value) in values {
            let follows = self
                .cells
                .get(&cell)
                .is_some_and(|saved| match (&saved.value, value) {
                    (ir::Value::Float(a), ir::Value::Float(b)) => a.to_bits() == b.to_bits(),
                    (a, b) => a == b,
                });
            if !follows {
                let owner = from_widgets
                    .then(|| self.cells.get(&cell).map(|saved| saved.owner))
                    .flatten()
                    .unwrap_or_else(|| self.owner.with(fresh_owner()));
                self.cells.insert(
                    cell,
                    SavedCell {
                        value: value.clone(),
                        owner,
                    },
                );
            }
        }
    }
}

impl FerruleApp {
    pub(super) fn reconcile_graph_drafts(&mut self) {
        self.graph_drafts
            .reconcile(self.document_ui_id, &self.project);
    }

    pub(super) fn begin_graph_drafts(
        &mut self,
        document: MappingDocument,
        context: &egui::Context,
    ) {
        self.graph_drafts.context = Some(context.clone());
        self.reconcile_graph_drafts();
        let owners = self
            .graph_document(document)
            .and_then(|document| self.graph_drafts.canvases.get(&document))
            .map(|canvas| {
                canvas
                    .nodes
                    .iter()
                    .filter(|(_, lifetime)| lifetime.value_editor)
                    .map(|(&id, lifetime)| {
                        (
                            id,
                            NodeOwner {
                                id: lifetime.owner,
                                cells: lifetime
                                    .cells
                                    .iter()
                                    .map(|(&cell, saved)| (cell, saved.owner))
                                    .collect::<std::collections::HashMap<_, _>>()
                                    .into(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        crate::graph_draft_ui::begin(context, owners);
        let canvas = self
            .graph_document(document)
            .and_then(|document| self.graph_drafts.canvases.get(&document))
            .map(|canvas| canvas.owner);
        crate::graph_draft_ui::begin_canvas(context, canvas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt as _;
    use ir::{Decimal96, Value};

    fn fixture() -> FerruleApp {
        let mut app = FerruleApp::default();
        app.project.graph.nodes = [
            (
                0,
                Node::Const {
                    value: Value::decimal(Decimal96::parse_exact("7.25").unwrap()),
                },
            ),
            (
                1,
                Node::ValueMap {
                    input: 0,
                    input_type: None,
                    table: vec![(Value::Int(7), Value::Int(9))],
                    default: Some(Value::Int(11)),
                },
            ),
        ]
        .into();
        app
    }

    fn draw(app: &mut FerruleApp, context: &egui::Context) {
        let _ = context.run_test_ui(Default::default(), |ui| {
            app.begin_graph_drafts(MappingDocument::Main, context);
            for (&id, node) in &mut app.project.graph.nodes {
                crate::graph_draft_ui::node(ui, id, "retention_test", |ui| match node {
                    Node::Const { value } => {
                        crate::graph_draft_ui::cell(ui, Cell::Constant, |ui| {
                            crate::value_editor::show_value_editor(ui, value)
                        });
                    }
                    Node::ValueMap { table, default, .. } => {
                        crate::value_editor::show_value_map_editor(ui, table, default, None)
                    }
                    _ => {}
                });
            }
        });
    }

    fn draw_containers(app: &mut FerruleApp, context: &egui::Context) {
        let _ = context.run_test_ui(Default::default(), |ui| {
            app.begin_graph_drafts(MappingDocument::Main, context);
            let layer = crate::graph_draft_ui::canvas_layer(context).unwrap();
            context.set_sublayer(
                ui.layer_id(),
                egui::LayerId::new(ui.layer_id().order, layer),
            );
            for (&id, node) in &mut app.project.graph.nodes {
                let (_, popup) = crate::graph_draft_ui::node_controls(context, id).unwrap();
                let mut open = true;
                egui::Popup::new(
                    popup,
                    context.clone(),
                    egui::pos2(20.0 + id as f32 * 320.0, 30.0),
                    ui.layer_id(),
                )
                .open_bool(&mut open)
                .show(|ui| {
                    crate::graph_draft_ui::node(ui, id, "containers", |ui| match node {
                        Node::Const { value } => {
                            crate::graph_draft_ui::cell(ui, Cell::Constant, |ui| {
                                crate::value_editor::show_value_editor(ui, value)
                            });
                        }
                        Node::ValueMap { table, default, .. } => {
                            crate::value_editor::show_value_map_editor(ui, table, default, None)
                        }
                        _ => {}
                    });
                });
            }
        });
    }

    fn warm_menus(app: &mut FerruleApp, context: &egui::Context) {
        draw_containers(app, context);
        let owners = app
            .graph_drafts
            .canvases
            .values()
            .flat_map(|canvas| {
                canvas
                    .nodes
                    .values()
                    .flat_map(|node| node.cells.values().map(|cell| cell.owner))
            })
            .collect::<Vec<_>>();
        for owner in owners {
            for popup in crate::graph_draft_ui::owner_popups(context, owner) {
                egui::Popup::open_id(context, popup);
                draw_containers(app, context);
                egui::Popup::close_id(context, popup);
                draw_containers(app, context);
            }
        }
    }

    #[test]
    fn graph_draft_real_menu_areas_and_layers_are_bounded_across_retirements() {
        let context = egui::Context::default();
        let mut app = fixture();
        let mut other = fixture();
        warm_menus(&mut app, &context);
        warm_menus(&mut other, &context);
        let baseline = crate::graph_draft_ui::container_counts(&context);
        assert!(baseline.0 >= 8 && baseline.1 > baseline.0);
        let counts = crate::value_editor::graph_draft_counts(&context);
        let unrelated = egui::Id::unique("unrelated editor value");
        context.data_mut(|data| data.insert_temp(unrelated, "untouched".to_owned()));
        for step in 0..32 {
            match step % 4 {
                0 => {
                    let node = app.project.graph.nodes.remove(&1).unwrap();
                    app.reconcile_graph_drafts();
                    app.project.graph.nodes.insert(1, node);
                }
                1 => app
                    .graph_drafts
                    .retire_canvas(&GraphDocument::Target(TargetId::Primary)),
                2 => app.document_ui_id = super::super::new_document_ui_id(),
                _ => {
                    app = fixture();
                } // Closing/recreating a stage/editor drops its tracker.
            }
            warm_menus(&mut app, &context);
            assert_eq!(
                crate::graph_draft_ui::container_counts(&context),
                baseline,
                "retirement {step}"
            );
            assert_eq!(crate::value_editor::graph_draft_counts(&context), counts);
            assert_eq!(
                context
                    .data(|data| data.get_temp::<String>(unrelated))
                    .as_deref(),
                Some("untouched")
            );
        }
        drop(app);
        warm_menus(&mut other, &context);
        assert_eq!(crate::graph_draft_ui::container_counts(&context), baseline);
        assert_eq!(crate::value_editor::graph_draft_counts(&context), (1, 3, 4));
    }

    #[test]
    fn graph_draft_retirement_bounds_state_and_preserves_other_editors() {
        let context = egui::Context::default();
        let sentinel = egui::Id::unique("unrelated UI state");
        context.data_mut(|data| data.insert_temp(sentinel, "keep me".to_owned()));
        let mut app = fixture();
        let mut other = fixture();
        draw(&mut app, &context);
        draw(&mut other, &context);
        let baseline = crate::value_editor::graph_draft_counts(&context);
        let registered = crate::graph_draft_ui::retained_counts(&context);
        let total_state = context.data(|data| data.len());
        assert_eq!(baseline, (2, 6, 8));
        for index in 0..64 {
            // Both affected cells receive genuinely different saved values.
            app.project.graph.nodes.insert(
                0,
                Node::Const {
                    value: Value::decimal(Decimal96::parse_exact(&format!("{index}.5")).unwrap()),
                },
            );
            let Node::ValueMap { table, .. } = app.project.graph.nodes.get_mut(&1).unwrap() else {
                unreachable!()
            };
            table[0].0 = Value::Int(index);
            draw(&mut app, &context);
            assert_eq!(crate::value_editor::graph_draft_counts(&context), baseline);
            assert_eq!(crate::graph_draft_ui::retained_counts(&context), registered);
            assert!(context.data(|data| data.len()) <= total_state);
            match index % 3 {
                0 => {
                    let node = app.project.graph.nodes.remove(&1).unwrap();
                    app.reconcile_graph_drafts();
                    assert_eq!(crate::value_editor::graph_draft_counts(&context), (2, 3, 5));
                    app.project.graph.nodes.insert(1, node);
                }
                1 => app
                    .graph_drafts
                    .retire_canvas(&GraphDocument::Target(TargetId::Primary)),
                _ => {
                    app.document_ui_id = super::super::new_document_ui_id();
                    app.reconcile_graph_drafts();
                }
            }
            draw(&mut app, &context);
            assert_eq!(crate::value_editor::graph_draft_counts(&context), baseline);
            assert_eq!(crate::graph_draft_ui::retained_counts(&context), registered);
            assert!(context.data(|data| data.len()) <= total_state);
            assert_eq!(
                context
                    .data(|data| data.get_temp::<String>(sentinel))
                    .as_deref(),
                Some("keep me")
            );
        }
        drop(app);
        assert_eq!(crate::value_editor::graph_draft_counts(&context), (1, 3, 4));
        assert_eq!(
            crate::graph_draft_ui::retained_counts(&context).0 * 2,
            registered.0
        );
        draw(&mut other, &context);
        assert_eq!(crate::value_editor::graph_draft_counts(&context), (1, 3, 4));
        drop(other);
        assert_eq!(crate::value_editor::graph_draft_counts(&context), (0, 0, 0));
        assert_eq!(crate::graph_draft_ui::retained_counts(&context), (0, 0));
        assert_eq!(
            context
                .data(|data| data.get_temp::<String>(sentinel))
                .as_deref(),
            Some("keep me")
        );
    }
}
