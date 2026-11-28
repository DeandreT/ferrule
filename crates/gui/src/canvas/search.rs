//! Find fields by path, then reveal the selected semantic pin on this canvas.

use crate::canvas::{CanvasNode, SourceBlock, TargetBlock};
use crate::canvas_endpoints::EndpointScrollState;
use crate::canvas_keyboard::focus::CanvasPin;
use crate::graph_viewer::{GraphViewer, ProjectGraphReferences};
use egui_snarl::{InPinId, OutPinId, Snarl};

/// Catalog index, independent of filtering, scroll position and proxy pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldPin {
    pub node: CanvasNode,
    pub semantic: usize,
}

impl FieldPin {
    pub(crate) fn from_canvas(
        viewer: &GraphViewer<'_>,
        snarl: &Snarl<CanvasNode>,
        pin: CanvasPin,
    ) -> Option<Self> {
        let node = *snarl.get_node(pin.node())?;
        let total = match (node, pin) {
            (CanvasNode::SourceBlock(i), CanvasPin::Output(_)) => {
                viewer.source_blocks.get(i)?.leaves.len()
            }
            (CanvasNode::TargetBlock(i), CanvasPin::Input(_)) => {
                viewer.target_blocks.get(i)?.leaves.len()
            }
            _ => return None,
        };
        Some(Self {
            node,
            semantic: viewer
                .endpoint_scroll
                .semantic_pin(node, pin.index(), total)?,
        })
    }

    fn count(self, sources: &[SourceBlock], targets: &[TargetBlock]) -> Option<usize> {
        match self.node {
            CanvasNode::SourceBlock(i) => Some(sources.get(i)?.leaves.len()),
            CanvasNode::TargetBlock(i) => Some(targets.get(i)?.leaves.len()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Fields<'a> {
    sources: &'a [SourceBlock],
    targets: &'a [TargetBlock],
    owners: ProjectGraphReferences<'a>,
}

impl<'a> Fields<'a> {
    pub(crate) fn of(viewer: &'a GraphViewer<'_>) -> Self {
        Self {
            sources: viewer.source_blocks,
            targets: viewer.target_blocks,
            owners: viewer.project_references,
        }
    }

    pub(crate) fn label(self, pin: FieldPin) -> Option<String> {
        match pin.node {
            CanvasNode::SourceBlock(i) => {
                let block = self.sources.get(i)?;
                let leaf = block.leaves.get(pin.semantic)?;
                let named = i.checked_sub(1).and_then(|i| self.owners.named_source(i));
                let root = named
                    .map(|s| s.schema.name.as_str())
                    .or_else(|| self.owners.primary_source_root())
                    .unwrap_or_else(|| block.title.trim_start_matches("Source: "));
                let owner = named.map(|s| format!(" ({})", s.name)).unwrap_or_default();
                // Named source frames/paths start with their runtime alias.
                // The label already names the owner; the path starts at its schema.
                let segments = leaf
                    .frame
                    .iter()
                    .flatten()
                    .chain(&leaf.path)
                    .skip(usize::from(named.is_some()))
                    .map(String::as_str);
                let mut path = String::new();
                for part in std::iter::once(root).chain(segments) {
                    path.push('/');
                    path.push_str(&part.replace('~', "~0").replace('/', "~1"));
                }
                Some(format!("Source{owner}  {path}"))
            }
            CanvasNode::TargetBlock(i) => {
                let block = self.targets.get(i)?;
                let leaf = block.leaves.get(pin.semantic)?;
                let (root, owner) = self
                    .owners
                    .target
                    .unwrap_or_else(|| (block.title.trim_start_matches("Target: "), None));
                let owner = owner.map(|s| format!(" ({s})")).unwrap_or_default();
                let mut path = String::new();
                for part in std::iter::once(root)
                    .chain(leaf.chain.iter().map(String::as_str))
                    .chain([crate::target_xml_type::field_label(&leaf.field)])
                {
                    if part.contains(['.', '[', ']', '\\', '"']) {
                        path.push_str(&format!("[{part:?}]"));
                    } else {
                        if !path.is_empty() {
                            path.push('.');
                        }
                        path.push_str(part);
                    }
                }
                Some(format!("Target{owner}  {path}"))
            }
            _ => None,
        }
    }

    fn matches(self, query: &str) -> Vec<(FieldPin, String)> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, b)| (CanvasNode::SourceBlock(i), b.leaves.len()));
        let targets = self
            .targets
            .iter()
            .enumerate()
            .map(|(i, b)| (CanvasNode::TargetBlock(i), b.leaves.len()));
        sources
            .chain(targets)
            .flat_map(|(node, count)| (0..count).map(move |semantic| FieldPin { node, semantic }))
            .filter_map(|pin| {
                let label = self.label(pin)?;
                label
                    .to_lowercase()
                    .contains(&query)
                    .then_some((pin, label))
            })
            .collect()
    }
}

struct Revealed {
    pin: FieldPin,
    total: usize,
    offset: usize,
    was_open: bool,
}

#[derive(Default)]
pub struct CanvasSearchState {
    open: bool,
    query: String,
    cursor: usize,
    focus_query: bool,
    return_focus: Option<egui::Id>,
    pending: Option<FieldPin>,
    revealed: Option<Revealed>,
}

impl CanvasSearchState {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn open(&mut self, return_focus: Option<egui::Id>) {
        self.open = true;
        self.focus_query = true;
        self.return_focus = return_focus;
        self.query.clear();
        self.cursor = 0;
    }

    pub(crate) fn request_reveal(&mut self, pin: FieldPin) {
        self.open = false;
        self.pending = Some(pin);
    }

    pub(crate) fn highlighted(&self) -> Option<(CanvasNode, usize)> {
        self.revealed.as_ref().map(|r| (r.pin.node, r.pin.semantic))
    }

    /// Undo only the temporary endpoint projection installed by the last reveal.
    pub(crate) fn clear_reveal(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        scroll: &mut EndpointScrollState,
    ) {
        if let Some(previous) = self.revealed.take() {
            let node = previous.pin.node;
            scroll.clear_reveal(node, previous.total);
            scroll.set_offset(node, previous.total, previous.offset);
            let id = snarl
                .node_ids()
                .find(|(_, n)| **n == node)
                .map(|(id, _)| id);
            if let Some(id) = id
                && !previous.was_open
            {
                snarl.open_node(id, false);
            }
        }
    }

    pub(crate) fn apply_reveal(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        scroll: &mut EndpointScrollState,
        sources: &[SourceBlock],
        targets: &[TargetBlock],
    ) -> Option<CanvasPin> {
        let pin = self.pending.take()?;
        let total = pin
            .count(sources, targets)
            .filter(|&total| pin.semantic < total)?;
        let id = snarl.node_ids().find(|(_, n)| **n == pin.node)?.0;
        self.clear_reveal(snarl, scroll);
        self.revealed = Some(Revealed {
            pin,
            total,
            offset: scroll.offset(pin.node, total),
            was_open: snarl.get_node_info(id)?.open,
        });
        snarl.open_node(id, true);
        scroll.reveal(pin.node, total, pin.semantic);
        let visual = scroll.visual_pin_for_semantic(pin.node, pin.semantic, total)?;
        debug_assert_eq!(
            scroll.semantic_pin(pin.node, visual, total),
            Some(pin.semantic)
        );
        match pin.node {
            CanvasNode::SourceBlock(_) => Some(CanvasPin::Output(OutPinId {
                node: id,
                output: visual,
            })),
            CanvasNode::TargetBlock(_) => Some(CanvasPin::Input(InPinId {
                node: id,
                input: visual,
            })),
            _ => None,
        }
    }
}

pub(crate) fn show(
    ctx: &egui::Context,
    id: egui::Id,
    viewport: egui::Rect,
    state: &mut CanvasSearchState,
    fields: Fields<'_>,
    surface: egui::Id,
) {
    if !state.open {
        return;
    }
    let query_id = id.with("field_query");
    // Take navigation keys before the single-line editor sees Enter and drops focus.
    let key = (!crate::ui_kit::modal_open(ctx))
        .then(|| {
            crate::canvas_keyboard::focus::take_key(
                ctx,
                query_id,
                &[
                    egui::Key::ArrowUp,
                    egui::Key::ArrowDown,
                    egui::Key::Enter,
                    egui::Key::Escape,
                ],
            )
        })
        .flatten();
    let mut close = key == Some(egui::Key::Escape);
    let mut reveal = None;
    let width = (viewport.width() - 32.0).clamp(80.0, 465.0);
    let position = egui::pos2(
        viewport.center().x - width * 0.5 - 6.0,
        viewport.top() + 8.0,
    );
    egui::Area::new(crate::graph_draft_ui::paint_canvas(ctx, id).with("field_search"))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    let label = ui.strong("Find a field");
                    let response = ui
                        .add(
                            egui::TextEdit::singleline(&mut state.query)
                                .id(query_id)
                                .hint_text("Field name or full path")
                                .desired_width((ui.available_width() - 36.0).max(40.0)),
                        )
                        .labelled_by(label.id);
                    if std::mem::take(&mut state.focus_query) {
                        response.request_focus();
                    }
                    if response.changed() {
                        state.cursor = 0;
                    }
                    close |=
                        crate::icons::button(ui, true, lucide_icons::Icon::X, "Close field search")
                            .clicked();
                });
                let matches = fields.matches(&state.query);
                if matches.is_empty() {
                    ui.weak(if state.query.trim().is_empty() {
                        "Search source and target paths"
                    } else {
                        "No matching fields"
                    });
                    return;
                }
                state.cursor = state.cursor.min(matches.len() - 1);
                match key {
                    Some(egui::Key::ArrowUp) => {
                        state.cursor = state.cursor.checked_sub(1).unwrap_or(matches.len() - 1)
                    }
                    Some(egui::Key::ArrowDown) => state.cursor = (state.cursor + 1) % matches.len(),
                    Some(egui::Key::Enter) => reveal = Some(matches[state.cursor].0),
                    _ => {}
                }
                egui::ScrollArea::vertical()
                    .max_height((viewport.height() * 0.36).min(230.0))
                    .show(ui, |ui| {
                        for (i, (pin, label)) in matches.iter().enumerate() {
                            let selected = state.cursor == i;
                            let response =
                                ui.add(egui::Button::selectable(selected, label.as_str()).wrap());
                            if selected
                                && matches!(key, Some(egui::Key::ArrowUp | egui::Key::ArrowDown))
                            {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                reveal = Some(*pin);
                            }
                        }
                    });
                ui.weak("↑ ↓  Choose     Enter  Reveal     Esc  Close");
            });
        });
    if close {
        state.open = false;
        let restore = state
            .return_focus
            .take()
            .filter(|id| ctx.read_response(*id).is_some())
            .unwrap_or(surface);
        ctx.memory_mut(|memory| memory.request_focus(restore));
    } else if let Some(pin) = reveal {
        state.request_reveal(pin);
    }
    if !state.open {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_finder_distinguishes_named_sources_and_literal_path_separators() {
        let mut project =
            mapping::project_file::decode_bytes(cli::orders_example::PROJECT).unwrap();
        project.extra_sources.push(mapping::NamedSource {
            name: "repeat".into(),
            path: "other.xml".into(),
            schema: project.source.clone(),
            options: Default::default(),
            dynamic_path: None,
        });
        let scalar = |name| ir::SchemaNode::scalar(name, ir::ScalarType::String);
        project.target = ir::SchemaNode::group(
            "row",
            vec![scalar("a.b"), ir::SchemaNode::group("a", vec![scalar("b")])],
        );
        let sources = crate::canvas::project_source_blocks(&project);
        let targets = crate::canvas::target_blocks(&project.target);
        let fields = Fields {
            sources: &sources,
            targets: &targets,
            owners: ProjectGraphReferences::new(&[], &project.extra_sources)
                .with_primary_source(&project.source)
                .with_target("row", Some("archive")),
        };
        let found = fields.matches("/Orders/Order/Items/Item/Desc");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0.node, CanvasNode::SourceBlock(0));
        assert_eq!(found[1].0.node, CanvasNode::SourceBlock(1));
        assert!(found[1].1.starts_with("Source (repeat)"));
        assert_eq!(
            fields.matches("row.a.b")[0].0.node,
            CanvasNode::TargetBlock(1)
        );
        assert_eq!(
            fields.matches("row[\"a.b\"]")[0].0.node,
            CanvasNode::TargetBlock(0)
        );
    }

    #[test]
    fn field_finder_uses_the_semantic_pin_even_when_filtered_scrolled_and_collapsed() {
        let schema = ir::SchemaNode::group(
            "row",
            (0..32)
                .map(|i| ir::SchemaNode::scalar(format!("f{i}"), ir::ScalarType::String))
                .collect(),
        );
        let targets = crate::canvas::target_blocks(&schema);
        let node = CanvasNode::TargetBlock(0);
        let mut snarl = Snarl::new();
        let id = snarl.insert_node(egui::pos2(12.0, 30.0), node);
        snarl.open_node(id, false);
        let mut scroll = EndpointScrollState::default();
        scroll.set_connected(node, 32, (0..31).step_by(2));
        scroll.set_connected_only(node, true);
        scroll.scroll_rows(node, 32, 2);
        let mut search = CanvasSearchState::default();
        search.request_reveal(FieldPin { node, semantic: 31 });
        let visual = search
            .apply_reveal(&mut snarl, &mut scroll, &[], &targets)
            .unwrap();
        assert_eq!(visual.node(), id);
        assert_eq!(scroll.semantic_pin(node, visual.index(), 32), Some(31));
        assert!(snarl.get_node_info(id).unwrap().open);
        search.clear_reveal(&mut snarl, &mut scroll);
        assert!(scroll.connected_only(node));
        assert!(!scroll.eligible_indices(node, 32).contains(&31));
        assert_eq!(scroll.offset(node, 32), 2);
        assert!(!snarl.get_node_info(id).unwrap().open);
    }
}
