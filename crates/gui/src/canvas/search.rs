//! Selected endpoint field search for the mapping canvas.

use egui_snarl::{NodeId as SnarlNodeId, Snarl};

use crate::canvas::{CanvasNode, SourceBlock, TargetBlock};
use crate::canvas_endpoints::EndpointScrollState;

#[derive(Default)]
pub struct CanvasSearchState {
    endpoint: Option<CanvasNode>,
    query: String,
    match_cursor: usize,
    request_focus: bool,
    revealed_endpoint: Option<(CanvasNode, usize)>,
}

impl CanvasSearchState {
    pub(crate) fn is_open(&self) -> bool {
        self.endpoint.is_some()
    }
    pub fn open_selected(&mut self, selected: &[SnarlNodeId], snarl: &Snarl<CanvasNode>) -> bool {
        let Some(&node_id) = selected.first().filter(|_| selected.len() == 1) else {
            return false;
        };
        let Some(&endpoint) = snarl.get_node(node_id).filter(|node| {
            matches!(
                node,
                CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_)
            )
        }) else {
            return false;
        };
        if self.endpoint != Some(endpoint) {
            self.query.clear();
            self.match_cursor = 0;
        }
        self.endpoint = Some(endpoint);
        self.request_focus = true;
        true
    }

    pub fn active_match(
        &self,
        source_blocks: &[SourceBlock],
        target_blocks: &[TargetBlock],
    ) -> Option<(CanvasNode, usize)> {
        let endpoint = self.endpoint?;
        let matches = self.matches(source_blocks, target_blocks);
        matches
            .get(self.match_cursor.min(matches.len().saturating_sub(1)))
            .copied()
            .map(|field| (endpoint, field))
    }

    fn matches(&self, source_blocks: &[SourceBlock], target_blocks: &[TargetBlock]) -> Vec<usize> {
        let Some(endpoint) = self.endpoint else {
            return Vec::new();
        };
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        endpoint_fields(endpoint, source_blocks, target_blocks)
            .into_iter()
            .enumerate()
            .filter_map(|(index, (label, path))| {
                (label.to_lowercase().contains(&query) || path.to_lowercase().contains(&query))
                    .then_some(index)
            })
            .collect()
    }

    fn update_reveal(
        &mut self,
        source_blocks: &[SourceBlock],
        target_blocks: &[TargetBlock],
        scroll: &mut EndpointScrollState,
    ) -> bool {
        let active = self.active_match(source_blocks, target_blocks);
        let mut changed = false;
        if let Some((previous, total)) = self.revealed_endpoint
            && active.is_none_or(|(node, _)| node != previous)
        {
            changed |= scroll.clear_reveal(previous, total);
            self.revealed_endpoint = None;
        }
        if let Some((node, semantic)) = active {
            let total = endpoint_fields(node, source_blocks, target_blocks).len();
            changed |= scroll.reveal(node, total, semantic);
            self.revealed_endpoint = Some((node, total));
        }
        changed
    }
}

pub fn show(
    ctx: &egui::Context,
    id: egui::Id,
    viewport: egui::Rect,
    state: &mut CanvasSearchState,
    source_blocks: &[SourceBlock],
    target_blocks: &[TargetBlock],
    scroll: &mut EndpointScrollState,
) {
    if state.update_reveal(source_blocks, target_blocks, scroll) {
        ctx.request_repaint();
    }
    let Some(endpoint) = state.endpoint else {
        return;
    };
    let fields = endpoint_fields(endpoint, source_blocks, target_blocks);
    if fields.is_empty() {
        state.endpoint = None;
        if state.update_reveal(source_blocks, target_blocks, scroll) {
            ctx.request_repaint();
        }
        return;
    }

    let width = 420.0_f32.min((viewport.width() - 24.0).max(240.0));
    let position = egui::pos2(viewport.center().x - width / 2.0, viewport.top() + 12.0);
    let mut close = false;
    let mut move_match = 0_isize;
    egui::Area::new(id.with("field_search"))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [(width - 142.0).max(120.0), ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut state.query).hint_text("Find field"),
                    );
                    if state.request_focus {
                        response.request_focus();
                        state.request_focus = false;
                    }
                    if response.changed() {
                        state.match_cursor = 0;
                    }

                    let matches = state.matches(source_blocks, target_blocks);
                    let position = if matches.is_empty() {
                        "0/0".to_string()
                    } else {
                        format!(
                            "{}/{}",
                            state.match_cursor.min(matches.len() - 1) + 1,
                            matches.len()
                        )
                    };
                    ui.label(position);
                    if crate::icons::button(
                        ui,
                        !matches.is_empty(),
                        lucide_icons::Icon::ChevronUp,
                        "Previous match",
                    )
                    .clicked()
                    {
                        move_match = -1;
                    }
                    if crate::icons::button(
                        ui,
                        !matches.is_empty(),
                        lucide_icons::Icon::ChevronDown,
                        "Next match",
                    )
                    .clicked()
                    {
                        move_match = 1;
                    }
                    if crate::icons::button(ui, true, lucide_icons::Icon::X, "Close field search")
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
        });

    let (escape, previous, next) = ctx.input_mut(|input| {
        (
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            input.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter),
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    close |= escape;
    if previous {
        move_match = -1;
    } else if next {
        move_match = 1;
    }
    if close {
        state.endpoint = None;
        if state.update_reveal(source_blocks, target_blocks, scroll) {
            ctx.request_repaint();
        }
        return;
    }

    let matches = state.matches(source_blocks, target_blocks);
    if !matches.is_empty() {
        state.match_cursor = state.match_cursor.min(matches.len() - 1);
        if move_match < 0 {
            state.match_cursor = state
                .match_cursor
                .checked_sub(1)
                .unwrap_or(matches.len() - 1);
        } else if move_match > 0 {
            state.match_cursor = (state.match_cursor + 1) % matches.len();
        }
    }
    if state.update_reveal(source_blocks, target_blocks, scroll) {
        ctx.request_repaint();
    }
}

fn endpoint_fields<'a>(
    endpoint: CanvasNode,
    source_blocks: &'a [SourceBlock],
    target_blocks: &'a [TargetBlock],
) -> Vec<(&'a str, &'a str)> {
    match endpoint {
        CanvasNode::SourceBlock(block) => {
            source_blocks.get(block).map_or_else(Vec::new, |section| {
                section
                    .pin_labels
                    .iter()
                    .zip(&section.leaves)
                    .map(|(label, leaf)| (label.as_str(), leaf.label.as_str()))
                    .collect()
            })
        }
        CanvasNode::TargetBlock(block) => {
            target_blocks.get(block).map_or_else(Vec::new, |section| {
                section
                    .pin_labels
                    .iter()
                    .zip(&section.leaves)
                    .map(|(label, leaf)| (label.as_str(), leaf.label.as_str()))
                    .collect()
            })
        }
        CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{SourceLeaf, TargetLeaf};

    #[test]
    fn clearing_closing_and_switching_search_remove_temporary_filtered_fields() {
        let schema = ir::SchemaNode::group(
            "source",
            (0..5)
                .map(|index| ir::SchemaNode::scalar(format!("s{index}"), ir::ScalarType::String))
                .collect(),
        );
        let sources = crate::canvas::source_blocks(&schema);
        let targets = crate::canvas::target_blocks(&schema);
        let source = CanvasNode::SourceBlock(0);
        let target = CanvasNode::TargetBlock(0);
        let mut scroll = EndpointScrollState::default();
        scroll.set_connected(source, 5, [1]);
        scroll.set_connected(target, 5, [2]);
        scroll.set_connected_only(source, true);
        scroll.set_connected_only(target, true);
        let mut search = CanvasSearchState {
            endpoint: Some(source),
            query: "s3".into(),
            ..Default::default()
        };
        assert!(search.update_reveal(&sources, &targets, &mut scroll));
        assert_eq!(scroll.eligible_indices(source, 5), vec![1, 3]);
        search.query.clear();
        assert!(search.update_reveal(&sources, &targets, &mut scroll));
        assert_eq!(scroll.eligible_indices(source, 5), vec![1]);
        search.query = "s4".into();
        search.update_reveal(&sources, &targets, &mut scroll);
        assert_eq!(scroll.eligible_indices(source, 5), vec![1, 4]);
        search.endpoint = Some(target);
        assert!(search.update_reveal(&sources, &targets, &mut scroll));
        assert_eq!(scroll.eligible_indices(source, 5), vec![1]);
        assert_eq!(scroll.eligible_indices(target, 5), vec![2, 4]);
        search.endpoint = None;
        assert!(search.update_reveal(&sources, &targets, &mut scroll));
        assert_eq!(scroll.eligible_indices(target, 5), vec![2]);
        assert!(!search.update_reveal(&sources, &targets, &mut scroll));
    }

    #[test]
    fn search_matches_compact_labels_and_full_paths_case_insensitively() {
        let source = vec![SourceBlock {
            title: "Source: row".into(),
            frame: None,
            leaves: vec![
                SourceLeaf {
                    label: "Workbook/Office/PrimaryKey".into(),
                    frame: None,
                    path: vec!["PrimaryKey".into()],
                    ty: crate::schema_scalar::ScalarDomain::Single(ir::ScalarType::String),
                },
                SourceLeaf {
                    label: "Workbook/Office/Name".into(),
                    frame: None,
                    path: vec!["Name".into()],
                    ty: crate::schema_scalar::ScalarDomain::Single(ir::ScalarType::String),
                },
            ],
            pin_labels: vec!["PrimaryKey".into(), "Name".into()],
        }];
        let mut state = CanvasSearchState {
            endpoint: Some(CanvasNode::SourceBlock(0)),
            query: "office/name".into(),
            ..Default::default()
        };

        assert_eq!(state.matches(&source, &[]), vec![1]);
        state.query = "PRIMARY".into();
        assert_eq!(state.matches(&source, &[]), vec![0]);
    }

    #[test]
    fn active_match_tracks_the_selected_target_field() {
        let target = vec![TargetBlock {
            title: "Target: row".into(),
            chain: Vec::new(),
            leaves: vec![
                TargetLeaf {
                    label: "first".into(),
                    chain: Vec::new(),
                    field: "first".into(),
                    ty: crate::schema_scalar::ScalarDomain::Single(ir::ScalarType::String),
                },
                TargetLeaf {
                    label: "second".into(),
                    chain: Vec::new(),
                    field: "second".into(),
                    ty: crate::schema_scalar::ScalarDomain::Single(ir::ScalarType::String),
                },
            ],
            pin_labels: vec!["first".into(), "second".into()],
        }];
        let state = CanvasSearchState {
            endpoint: Some(CanvasNode::TargetBlock(0)),
            query: "second".into(),
            ..Default::default()
        };

        assert_eq!(
            state.active_match(&[], &target),
            Some((CanvasNode::TargetBlock(0), 1))
        );
    }
}
