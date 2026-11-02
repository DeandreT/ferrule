//! The active semantic canvas's private property-widget identities.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use egui::util::id_type_map::{IdTypeMap, RawKey};

type RemoveState = fn(&mut IdTypeMap, egui::Id);

#[derive(Clone)]
enum OwnedState {
    Typed(egui::Id, RemoveState),
    Temporary,
}

#[derive(Clone, Default)]
struct StateRegistry {
    owners: HashMap<egui::Id, HashMap<RawKey, OwnedState>>,
    keys: HashMap<RawKey, egui::Id>,
    popups: HashMap<egui::Id, HashSet<egui::Id>>,
    text_fields: HashMap<egui::Id, HashSet<egui::Id>>,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum ContainerKind {
    Canvas,
    Node,
    Constant,
    MapRow,
    Default,
}

#[derive(Clone, Default)]
struct ContainerPool {
    owners: HashMap<egui::Id, (ContainerKind, usize)>,
    free: HashMap<ContainerKind, Vec<usize>>,
    next: HashMap<ContainerKind, usize>,
    widgets: HashMap<egui::Id, (egui::Id, egui::Id)>,
}

fn pool_id() -> egui::Id {
    active_id().with("container_pool")
}

fn container(context: &egui::Context, owner: egui::Id, kind: ContainerKind) -> egui::Id {
    context.data_mut(|data| {
        let pool = data.get_temp_mut_or_default::<ContainerPool>(pool_id());
        let ContainerPool {
            owners, free, next, ..
        } = pool;
        let (kind, slot) = *owners.entry(owner).or_insert_with(|| {
            let slot = free.entry(kind).or_default().pop().unwrap_or_else(|| {
                let next = next.entry(kind).or_default();
                let slot = *next;
                *next += 1;
                slot
            });
            (kind, slot)
        });
        let kind = match kind {
            ContainerKind::Canvas => "canvas",
            ContainerKind::Node => "node",
            ContainerKind::Constant => "constant",
            ContainerKind::MapRow => "row",
            ContainerKind::Default => "default",
        };
        active_id().with((kind, slot))
    })
}

pub(crate) fn begin_canvas(context: &egui::Context, owner: Option<egui::Id>) {
    context.data_mut(|data| {
        let id = active_id().with("canvas");
        data.remove::<egui::Id>(id);
        if let Some(owner) = owner {
            data.insert_temp(id, owner);
        }
    });
}

pub(crate) fn canvas_layer(context: &egui::Context) -> Option<egui::Id> {
    context
        .data(|data| data.get_temp::<egui::Id>(active_id().with("canvas")))
        .map(|owner| container(context, owner, ContainerKind::Canvas))
}

pub(crate) fn canvas_widget(context: &egui::Context, id: egui::Id, tools: egui::Id) {
    let owner = context.data(|data| data.get_temp::<egui::Id>(active_id().with("canvas")));
    if let Some(owner) = owner {
        container(context, owner, ContainerKind::Canvas);
        context.data_mut(|data| {
            let pool = data.get_temp_mut_or_default::<ContainerPool>(pool_id());
            // A target reorder can move a generation-specific canvas state
            // to another semantic target. Do not retain its old routing alias.
            pool.widgets
                .retain(|candidate, ids| *candidate == owner || (ids.0 != id && ids.1 != tools));
            pool.widgets.insert(owner, (id, tools));
        });
    }
    context.data_mut(|data| data.insert_temp(active_id().with("canvas_widget"), id));
}

pub(crate) fn paint_canvas(context: &egui::Context, widget: egui::Id) -> egui::Id {
    context
        .data(|data| {
            let pool = data
                .get_temp_raw(RawKey::new::<ContainerPool>(pool_id()))?
                .downcast_ref::<ContainerPool>()?;
            let (&owner, _) = pool
                .widgets
                .iter()
                .find(|(_, ids)| ids.0 == widget || ids.1 == widget)?;
            let (_, slot) = pool.owners.get(&owner)?;
            Some(active_id().with(("canvas", *slot)))
        })
        .unwrap_or(widget)
}

#[cfg(test)]
pub(crate) fn widget_canvas(context: &egui::Context, layer: egui::Id) -> egui::Id {
    context
        .data(|data| {
            let pool = data
                .get_temp_raw(RawKey::new::<ContainerPool>(pool_id()))?
                .downcast_ref::<ContainerPool>()?;
            pool.widgets.iter().find_map(|(owner, ids)| {
                let (_, slot) = pool.owners.get(owner)?;
                (active_id().with(("canvas", *slot)) == layer).then_some(ids.0)
            })
        })
        .unwrap_or(layer)
}

pub(crate) fn property_canvas(ui: &egui::Ui) -> egui::Id {
    ui.ctx()
        .data(|data| {
            data.get_temp::<egui::Id>(active_id().with("canvas"))
                .and_then(|_| data.get_temp::<egui::Id>(active_id().with("canvas_widget")))
        })
        .unwrap_or(ui.layer_id().id)
}

fn node_owner(context: &egui::Context, node: mapping::NodeId) -> Option<NodeOwner> {
    context
        .data(|data| data.get_temp::<Owners>(active_id()))
        .and_then(|owners| owners.get(&node).cloned())
}

pub(crate) fn node_container(
    context: &egui::Context,
    node: mapping::NodeId,
    salt: &str,
) -> Option<egui::Id> {
    node_owner(context, node)
        .map(|owner| container(context, owner.id, ContainerKind::Node).with(salt))
}

fn track_for<T: 'static>(data: &mut IdTypeMap, owner: egui::Id, id: egui::Id) {
    let registry = data.get_temp_mut_or_default::<StateRegistry>(registry_id());
    let key = RawKey::new::<T>(id);
    registry.keys.insert(key, owner);
    registry
        .owners
        .entry(owner)
        .or_default()
        .insert(key, OwnedState::Typed(id, IdTypeMap::remove::<T>));
    if std::any::TypeId::of::<T>() == std::any::TypeId::of::<egui::text_edit::TextEditState>() {
        registry.text_fields.entry(owner).or_default().insert(id);
    }
}

pub(crate) fn node_controls(
    context: &egui::Context,
    node: mapping::NodeId,
) -> Option<(egui::Id, egui::Id)> {
    let owner = node_owner(context, node)?.id;
    let root = container(context, owner, ContainerKind::Node);
    let anchor = root.with("anchor");
    let popup = root.with("properties");
    context.data_mut(|data| {
        track_for::<egui::Response>(data, owner, anchor);
        track_for::<bool>(data, owner, popup);
        track_for::<egui::containers::menu::MenuState>(
            data,
            owner,
            popup.with(egui::containers::menu::MenuState::ID),
        );
        track_for::<bool>(data, owner, popup.with("press_started_while_open"));
        data.get_temp_mut_or_default::<StateRegistry>(registry_id())
            .popups
            .entry(owner)
            .or_default()
            .insert(popup);
    });
    Some((anchor, popup))
}

pub(crate) fn track_popup(context: &egui::Context, response: &egui::Response) {
    context.data_mut(|data| {
        if let Some(owner) = data.get_temp::<egui::Id>(active_id().with("state_owner")) {
            data.get_temp_mut_or_default::<StateRegistry>(registry_id())
                .popups
                .entry(owner)
                .or_default()
                .insert(egui::Popup::default_response_id(response));
        }
    });
}

pub(crate) fn track_node_popup(
    context: &egui::Context,
    node: mapping::NodeId,
    response: &egui::Response,
) {
    if let Some(owner) = node_owner(context, node) {
        let popup = egui::Popup::default_response_id(response);
        context.data_mut(|data| {
            track_for::<egui::containers::menu::MenuState>(
                data,
                owner.id,
                popup.with(egui::containers::menu::MenuState::ID),
            );
            track_for::<bool>(data, owner.id, popup.with("press_started_while_open"));
            data.get_temp_mut_or_default::<StateRegistry>(registry_id())
                .popups
                .entry(owner.id)
                .or_default()
                .insert(popup);
        });
    }
}

fn registry_id() -> egui::Id {
    active_id().with("owned_states")
}

fn tracked_entries(data: &IdTypeMap) -> usize {
    data.get_temp_raw(RawKey::new::<StateRegistry>(registry_id()))
        .and_then(|value| value.downcast_ref::<StateRegistry>())
        .map_or(0, |registry| registry.keys.len() + 1)
}

#[cfg(test)]
pub(crate) fn retained_counts(context: &egui::Context) -> (usize, usize) {
    context.data(|data| {
        (
            data.get_temp::<StateRegistry>(registry_id())
                .map_or(0, |registry| registry.owners.len()),
            data.get_temp::<StateRegistry>(registry_id())
                .map_or(0, |registry| registry.keys.len()),
        )
    })
}

/// Register an exact typed key, including persisted TextEdit state. No other
/// editor's keys or egui's shared caches are swept during retirement.
pub(crate) fn track<T: 'static>(context: &egui::Context, id: egui::Id) {
    context.data_mut(|data| {
        if let Some(owner) = data.get_temp::<egui::Id>(active_id().with("state_owner")) {
            track_for::<T>(data, owner, id);
        }
    });
}

pub(crate) fn claim<T: 'static>(context: &egui::Context, id: egui::Id) {
    let fresh = context.data_mut(|data| {
        let Some(owner) = data.get_temp::<egui::Id>(active_id().with("state_owner")) else {
            return false;
        };
        let key = RawKey::new::<T>(id);
        let previous = data
            .get_temp_raw(RawKey::new::<StateRegistry>(registry_id()))
            .and_then(|value| value.downcast_ref::<StateRegistry>())
            .and_then(|registry| registry.keys.get(&key))
            .copied();
        let fresh = previous != Some(owner);
        // Container slots can also be restored by egui persistence. An
        // unowned state key must not become a new document's editing state.
        if fresh {
            data.remove::<T>(id);
        }
        track_for::<T>(data, owner, id);
        fresh
    });
    if fresh
        && std::any::TypeId::of::<T>() == std::any::TypeId::of::<egui::text_edit::TextEditState>()
    {
        context.memory_mut(|memory| memory.surrender_focus(id));
    }
}

pub(crate) fn retire(context: &egui::Context, owner: egui::Id) {
    let mut popups = HashSet::new();
    let mut text_fields = HashSet::new();
    context.data_mut(|data| {
        if let Some(mut registry) = data.remove_temp::<StateRegistry>(registry_id()) {
            text_fields = registry.text_fields.remove(&owner).unwrap_or_default();
            if let Some(states) = registry.owners.remove(&owner) {
                for (key, state) in states {
                    registry.keys.remove(&key);
                    match state {
                        OwnedState::Typed(id, remove) => {
                            if let Some(response) = data.get_temp::<egui::Response>(id) {
                                text_fields.insert(response.id);
                            }
                            remove(data, id);
                        }
                        OwnedState::Temporary => {
                            data.remove_temp_raw(key);
                        }
                    }
                }
            }
            popups = registry.popups.remove(&owner).unwrap_or_default();
            if !registry.owners.is_empty() || !registry.popups.is_empty() {
                data.insert_temp(registry_id(), registry);
            }
        }
        if let Some(mut pool) = data.remove_temp::<ContainerPool>(pool_id()) {
            if let Some((kind, slot)) = pool.owners.remove(&owner) {
                pool.free.entry(kind).or_default().push(slot);
            }
            pool.widgets.remove(&owner);
            if !pool.owners.is_empty() {
                data.insert_temp(pool_id(), pool);
            }
        }
        let canvas = active_id().with("canvas");
        if data.get_temp::<egui::Id>(canvas) == Some(owner) {
            data.remove::<egui::Id>(canvas);
            data.remove::<egui::Id>(active_id().with("canvas_widget"));
        }
    });
    if let Some(field) = context.memory(|memory| memory.focused())
        && context
            .read_response(field)
            .is_some_and(|response| popups.contains(&response.layer_id.id))
    {
        text_fields.insert(field);
    }
    for popup in popups {
        egui::Popup::close_id(context, popup);
    }
    context.memory_mut(|memory| {
        for field in text_fields {
            memory.surrender_focus(field);
        }
    });
}

fn owned_scope<R>(
    ui: &mut egui::Ui,
    owner: Option<egui::Id>,
    builder: egui::UiBuilder,
    capture_temporary: bool,
    show: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // Opaque egui temporary state (for example Grid's private State) has no
    // typed removal API. Attribute only newly allocated keys in this lexical
    // property scope; nested scopes register first and keep their ownership.
    type Baseline = std::sync::Arc<HashSet<RawKey>>;
    let capture_key = active_id().with("capture_baseline");
    let parent_baseline = ui.ctx().data(|data| data.get_temp::<Baseline>(capture_key));
    let before = owner.filter(|_| capture_temporary).map(|_| {
        parent_baseline.clone().unwrap_or_else(|| {
            std::sync::Arc::new(ui.ctx().data(|data| data.temp_keys().collect()))
        })
    });
    if let Some(before) = &before {
        ui.ctx()
            .data_mut(|data| data.insert_temp(capture_key, before.clone()));
    }
    let key = active_id().with("state_owner");
    let previous = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<egui::Id>(key);
        data.remove::<egui::Id>(key);
        if let Some(owner) = owner {
            data.insert_temp(key, owner);
        }
        previous
    });
    let (entries_before, tracked_before) =
        ui.ctx().data(|data| (data.len(), tracked_entries(data)));
    let result = ui.scope_builder(builder, show).inner;
    ui.ctx().data_mut(|data| {
        let tracked_after = tracked_entries(data);
        let changed_entries = (data.len() >= entries_before) != (tracked_after >= tracked_before)
            || data.len().abs_diff(entries_before) != tracked_after.abs_diff(tracked_before);
        data.remove::<egui::Id>(key);
        if let Some(previous) = previous {
            data.insert_temp(key, previous);
        }
        data.remove::<Baseline>(capture_key);
        if let Some(parent) = &parent_baseline {
            data.insert_temp(capture_key, parent.clone());
        }
        // Known draft/TextEdit inserts are already registered; don't rescan
        // the whole map per new table cell. A root scan still claims state created
        // outside the nested controls (for example Grid's end-of-pass state).
        if let (Some(owner), Some(before)) = (owner, before)
            && (changed_entries || parent_baseline.is_none())
        {
            let new_keys = data
                .temp_keys()
                .filter(|key| {
                    !before.contains(key) && *key != RawKey::new::<StateRegistry>(registry_id())
                        && *key != RawKey::new::<Baseline>(capture_key)
                        && *key != RawKey::new::<egui::Id>(active_id().with("state_owner"))
                        // Tooltip's shared last-shown timer is created inside
                        // property callbacks too, but is not editor-owned.
                        // DragValue's owned f64 state is registered explicitly.
                        && !data.get_temp_raw(*key).is_some_and(|value| value.is::<f64>())
                })
                .collect::<Vec<_>>();
            let registry = data.get_temp_mut_or_default::<StateRegistry>(registry_id());
            for key in new_keys {
                if let std::collections::hash_map::Entry::Vacant(entry) = registry.keys.entry(key) {
                    entry.insert(owner);
                    registry
                        .owners
                        .entry(owner)
                        .or_default()
                        .insert(key, OwnedState::Temporary);
                }
            }
        }
    });
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Cell {
    Constant,
    Key(usize),
    Value(usize),
    Default,
}

#[derive(Clone)]
pub(crate) struct NodeOwner {
    pub(crate) id: egui::Id,
    pub(crate) cells: Arc<HashMap<Cell, egui::Id>>,
}

type Owners = std::sync::Arc<BTreeMap<mapping::NodeId, NodeOwner>>;

pub(crate) fn begin(context: &egui::Context, owners: BTreeMap<mapping::NodeId, NodeOwner>) {
    context.data_mut(|data| data.insert_temp(active_id(), std::sync::Arc::new(owners)));
    begin_canvas(context, None);
}

#[cfg(test)]
pub(crate) fn container_counts(context: &egui::Context) -> (usize, usize) {
    context.memory(|memory| {
        // Areas exposes no public length accessor. Its derived Debug output
        // names each retained AreaState; layer_ids is the public full order.
        let areas = format!("{:?}", memory.areas())
            .matches("AreaState {")
            .count();
        (areas, memory.layer_ids().len())
    })
}

#[cfg(test)]
pub(crate) fn owner_popups(context: &egui::Context, owner: egui::Id) -> Vec<egui::Id> {
    context.data(|data| {
        data.get_temp::<StateRegistry>(registry_id())
            .and_then(|registry| registry.popups.get(&owner).cloned())
            .unwrap_or_default()
            .into_iter()
            .collect()
    })
}

fn active_id() -> egui::Id {
    egui::Id::unique("active_graph_draft_owners")
}

pub(crate) fn node<R>(
    ui: &mut egui::Ui,
    node: mapping::NodeId,
    kind: &str,
    show: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let owner = ui
        .ctx()
        .data(|data| data.get_temp::<Owners>(active_id()))
        .and_then(|owners| owners.get(&node).cloned());
    let current = active_id().with("node");
    let previous = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<NodeOwner>(current);
        data.remove::<NodeOwner>(current);
        if let Some(owner) = &owner {
            data.insert_temp(current, owner.clone());
        }
        previous
    });
    let owner_id = owner.as_ref().map(|owner| owner.id);
    let builder = owner.map_or_else(egui::UiBuilder::new, |owner| {
        // An absolute child identity follows the semantic canvas even when
        // its tab moves, its snarl is rebuilt, or its popup is reopened.
        egui::UiBuilder::new()
            .scope_id(container(ui.ctx(), owner.id, ContainerKind::Node).with(kind))
    });
    let result = owned_scope(ui, owner_id, builder, true, show);
    ui.ctx().data_mut(|data| {
        data.remove::<NodeOwner>(current);
        if let Some(previous) = previous {
            data.insert_temp(current, previous);
        }
    });
    result
}

pub(crate) fn cell<R>(ui: &mut egui::Ui, cell: Cell, show: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let id = ui
        .ctx()
        .data(|data| data.get_temp::<NodeOwner>(active_id().with("node")))
        .and_then(|owner| owner.cells.get(&cell).copied());
    let kind = match cell {
        Cell::Constant => ContainerKind::Constant,
        Cell::Key(_) | Cell::Value(_) => ContainerKind::MapRow,
        Cell::Default => ContainerKind::Default,
    };
    let builder = id.map_or_else(
        || egui::UiBuilder::new().id_salt(cell),
        |id| egui::UiBuilder::new().scope_id(container(ui.ctx(), id, kind)),
    );
    // A row/default can be enabled and rendered before the frame observer
    // allocates its cell lifetime. Its first-frame state still belongs to the
    // enclosing node, rather than becoming unregistered persisted TextEdit state.
    let state_owner = id.or_else(|| {
        ui.ctx()
            .data(|data| data.get_temp::<egui::Id>(active_id().with("state_owner")))
    });
    owned_scope(ui, state_owner, builder, true, show)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt as _;

    #[test]
    fn graph_draft_claim_discards_unowned_restored_state_but_keeps_self_edits() {
        let context = egui::Context::default();
        let owner = egui::Id::unique("claim owner");
        let field = egui::Id::unique("restored field");
        context.data_mut(|data| data.insert_persisted(field, "previous document".to_owned()));
        begin(
            &context,
            [(
                0,
                NodeOwner {
                    id: owner,
                    cells: Default::default(),
                },
            )]
            .into(),
        );
        for frame in 0..2 {
            let _ = context.run_test_ui(Default::default(), |ui| {
                node(ui, 0, "claim", |ui| {
                    claim::<String>(ui.ctx(), field);
                    if frame == 0 {
                        assert!(
                            ui.ctx()
                                .data_mut(|data| data.get_persisted::<String>(field))
                                .is_none()
                        );
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(field, "self edit".to_owned()));
                    } else {
                        assert_eq!(
                            ui.ctx()
                                .data(|data| data.get_temp::<String>(field))
                                .as_deref(),
                            Some("self edit")
                        );
                    }
                });
            });
        }
        retire(&context, owner);
        assert!(
            context
                .data(|data| data.get_temp::<String>(field))
                .is_none()
        );
    }

    #[test]
    fn graph_draft_thousand_row_identity_lookups_share_one_map() {
        let context = egui::Context::default();
        let owner = egui::Id::unique("linear row owner");
        let cells = Arc::new(
            (0..1_000)
                .flat_map(|row| {
                    [
                        (Cell::Key(row), egui::Id::unique(("key", row))),
                        (Cell::Value(row), egui::Id::unique(("value", row))),
                    ]
                })
                .collect::<HashMap<_, _>>(),
        );
        begin(
            &context,
            [(
                0,
                NodeOwner {
                    id: owner,
                    cells: cells.clone(),
                },
            )]
            .into(),
        );
        let _ = context.run_test_ui(Default::default(), |ui| {
            node(ui, 0, "linear", |ui| {
                let references = Arc::strong_count(&cells);
                for row in 0..1_000 {
                    for key in [Cell::Key(row), Cell::Value(row)] {
                        cell(ui, key, |ui| {
                            let active = ui
                                .ctx()
                                .data(|data| data.get_temp::<NodeOwner>(active_id().with("node")))
                                .unwrap();
                            assert!(Arc::ptr_eq(&active.cells, &cells));
                            assert_eq!(Arc::strong_count(&cells), references + 1);
                        });
                        assert_eq!(Arc::strong_count(&cells), references);
                    }
                }
            });
        });
        for &owner in cells.values() {
            retire(&context, owner);
        }
        retire(&context, owner);
    }

    #[test]
    fn graph_draft_retirement_does_not_claim_shared_timers_created_inside_widgets() {
        let context = egui::Context::default();
        let owner = egui::Id::unique("retired property owner");
        let timer = egui::Id::unique("shared timer");
        let draft = owner.with("test draft");
        begin(
            &context,
            [(
                0,
                NodeOwner {
                    id: owner,
                    cells: Default::default(),
                },
            )]
            .into(),
        );
        let before = context.data(|data| data.len());
        let _ = context.run_test_ui(Default::default(), |ui| {
            node(ui, 0, "test", |ui| {
                // egui's tooltip machinery can create shared timing metadata
                // during this callback. It must not become an owned draft.
                ui.ctx().data_mut(|data| {
                    data.insert_temp(timer, 42.0_f64);
                    data.insert_temp(draft, "owned draft".to_owned());
                });
                track::<String>(ui.ctx(), draft);
                egui::Grid::new("owned grid").show(ui, |ui| {
                    ui.label("cell");
                    ui.end_row();
                });
            });
        });
        assert!(retained_counts(&context).1 >= 2);
        retire(&context, owner);
        assert_eq!(retained_counts(&context), (0, 0));
        assert!(
            context
                .data(|data| data.get_temp::<String>(draft))
                .is_none()
        );
        assert_eq!(context.data(|data| data.get_temp::<f64>(timer)), Some(42.0));
        assert_eq!(context.data(|data| data.len()), before + 1);
    }

    #[test]
    fn graph_draft_new_cells_register_first_frame_state_with_their_node() {
        let context = egui::Context::default();
        let owner = egui::Id::unique("new cells owner");
        // The real Add entry/Default controls can create cells after begin().
        // These cells have no entry in this frame's published cell-owner map.
        begin(
            &context,
            [(
                0,
                NodeOwner {
                    id: owner,
                    cells: Default::default(),
                },
            )]
            .into(),
        );
        let mut table = vec![(ir::Value::Int(7), ir::Value::Int(9))];
        let mut default = Some(ir::Value::Int(11));
        let _ = context.run_test_ui(Default::default(), |ui| {
            node(ui, 0, "table", |ui| {
                crate::value_editor::show_value_map_editor(ui, &mut table, &mut default, None)
            })
        });
        assert_eq!(crate::value_editor::graph_draft_counts(&context), (0, 3, 3));
        retire(&context, owner);
        assert_eq!(crate::value_editor::graph_draft_counts(&context), (0, 0, 0));
        assert_eq!(retained_counts(&context), (0, 0));
    }
}
