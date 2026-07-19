use egui::{Key, Ui};
use mapping::{AggregateOp, Node, NodeId};

pub(super) const AGGREGATE_OPS: [(AggregateOp, &str); 7] = [
    (AggregateOp::Count, "Count"),
    (AggregateOp::Sum, "Sum"),
    (AggregateOp::Avg, "Average"),
    (AggregateOp::Min, "Minimum"),
    (AggregateOp::Max, "Maximum"),
    (AggregateOp::Join, "String join"),
    (AggregateOp::ItemAt, "Item at"),
];

pub(super) fn aggregate_needs_arg(function: AggregateOp) -> bool {
    matches!(function, AggregateOp::Join | AggregateOp::ItemAt)
}

pub(super) fn aggregate_node(function: AggregateOp, arg: Option<NodeId>) -> Node {
    Node::Aggregate {
        function,
        collection: Vec::new(),
        value: Vec::new(),
        expression: None,
        arg,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeTemplate {
    Constant,
    SourceField,
    Position,
    Call,
    If,
    ValueMap,
    Lookup,
    CollectionFind,
    Aggregate(AggregateOp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Input,
    Transform,
    Logic,
    Collection,
    Aggregate,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::Input => "Input & values",
            Self::Transform => "Transform",
            Self::Logic => "Logic",
            Self::Collection => "Collections",
            Self::Aggregate => "Aggregates",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaletteEntry {
    category: Category,
    label: &'static str,
    keywords: &'static str,
    template: NodeTemplate,
}

const ENTRIES: [PaletteEntry; 15] = [
    PaletteEntry {
        category: Category::Input,
        label: "Constant",
        keywords: "const literal value null string number boolean",
        template: NodeTemplate::Constant,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Source field (manual path)",
        keywords: "source input field path",
        template: NodeTemplate::SourceField,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Position",
        keywords: "index row item collection",
        template: NodeTemplate::Position,
    },
    PaletteEntry {
        category: Category::Transform,
        label: "Function call",
        keywords: "call function concat string math comparison builtin",
        template: NodeTemplate::Call,
    },
    PaletteEntry {
        category: Category::Transform,
        label: "Value map",
        keywords: "lookup table translate replace default",
        template: NodeTemplate::ValueMap,
    },
    PaletteEntry {
        category: Category::Logic,
        label: "If",
        keywords: "condition then else branch conditional",
        template: NodeTemplate::If,
    },
    PaletteEntry {
        category: Category::Collection,
        label: "Lookup",
        keywords: "collection key match value reference",
        template: NodeTemplate::Lookup,
    },
    PaletteEntry {
        category: Category::Collection,
        label: "Find in collection",
        keywords: "search predicate select item value",
        template: NodeTemplate::CollectionFind,
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Count",
        keywords: "aggregate total size",
        template: NodeTemplate::Aggregate(AggregateOp::Count),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Sum",
        keywords: "aggregate total add numeric",
        template: NodeTemplate::Aggregate(AggregateOp::Sum),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Average",
        keywords: "aggregate avg mean numeric",
        template: NodeTemplate::Aggregate(AggregateOp::Avg),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Minimum",
        keywords: "aggregate min smallest",
        template: NodeTemplate::Aggregate(AggregateOp::Min),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Maximum",
        keywords: "aggregate max largest",
        template: NodeTemplate::Aggregate(AggregateOp::Max),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "String join",
        keywords: "aggregate concatenate separator text",
        template: NodeTemplate::Aggregate(AggregateOp::Join),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Item at",
        keywords: "aggregate index select position",
        template: NodeTemplate::Aggregate(AggregateOp::ItemAt),
    },
];

#[derive(Clone, Debug, Default)]
struct PaletteState {
    query: String,
    selected: usize,
    last_frame: u64,
}

impl PaletteState {
    fn move_selection(&mut self, amount: isize, result_count: usize) {
        if result_count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(amount)
            .min(result_count - 1);
    }
}

pub(super) fn show(ui: &mut Ui) -> Option<NodeTemplate> {
    let state_id = ui.id().with("node_palette");
    let frame = ui.ctx().cumulative_frame_nr();
    let mut state = ui
        .data_mut(|data| data.get_temp::<PaletteState>(state_id))
        .unwrap_or_default();
    let newly_opened = state.last_frame.checked_add(1) != Some(frame);
    if newly_opened {
        state.query.clear();
        state.selected = 0;
    }
    state.last_frame = frame;

    ui.set_min_width(280.0);
    ui.strong("Add node");
    let search = ui.add(
        egui::TextEdit::singleline(&mut state.query)
            .hint_text("Search nodes")
            .desired_width(f32::INFINITY),
    );
    if newly_opened {
        search.request_focus();
    }

    let matches = matching_entries(&state.query);
    if search.changed() {
        state.selected = 0;
    }
    if search.has_focus() {
        let (up, down) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            )
        });
        if up {
            state.move_selection(-1, matches.len());
        }
        if down {
            state.move_selection(1, matches.len());
        }
    }
    state.selected = state.selected.min(matches.len().saturating_sub(1));
    let enter = search.has_focus()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter));
    let mut chosen = enter
        .then(|| matches.get(state.selected).map(|entry| entry.template))
        .flatten();

    ui.separator();
    if matches.is_empty() {
        ui.label("No matching nodes");
    } else {
        egui::ScrollArea::vertical()
            .id_salt("node_palette_results")
            .max_height(340.0)
            .show(ui, |ui| {
                let mut previous_category = None;
                for (index, entry) in matches.iter().enumerate() {
                    if previous_category != Some(entry.category) {
                        if previous_category.is_some() {
                            ui.add_space(4.0);
                        }
                        ui.weak(entry.category.label());
                        previous_category = Some(entry.category);
                    }
                    let response = ui.selectable_label(index == state.selected, entry.label);
                    if response.hovered() {
                        state.selected = index;
                    }
                    if response.clicked() {
                        chosen = Some(entry.template);
                    }
                }
            });
    }

    ui.data_mut(|data| {
        if chosen.is_some() {
            data.remove::<PaletteState>(state_id);
        } else {
            data.insert_temp(state_id, state);
        }
    });
    chosen
}

fn matching_entries(query: &str) -> Vec<&'static PaletteEntry> {
    let terms: Vec<_> = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    ENTRIES
        .iter()
        .filter(|entry| {
            let haystack = format!(
                "{} {} {}",
                entry.category.label(),
                entry.label,
                entry.keywords
            )
            .to_ascii_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

#[cfg(test)]
pub(super) fn templates() -> impl Iterator<Item = NodeTemplate> {
    ENTRIES.iter().map(|entry| entry.template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_preserves_every_pre_palette_creation_action() {
        let templates: Vec<_> = ENTRIES.iter().map(|entry| entry.template).collect();
        assert_eq!(templates.len(), 15);
        for expected in [
            NodeTemplate::Constant,
            NodeTemplate::SourceField,
            NodeTemplate::Position,
            NodeTemplate::Call,
            NodeTemplate::If,
            NodeTemplate::ValueMap,
            NodeTemplate::Lookup,
            NodeTemplate::CollectionFind,
        ] {
            assert!(templates.contains(&expected));
        }
        for (operation, _) in AGGREGATE_OPS {
            assert!(templates.contains(&NodeTemplate::Aggregate(operation)));
        }
    }

    #[test]
    fn search_matches_labels_categories_and_keywords_case_insensitively() {
        assert_eq!(
            matching_entries("STRING aggregate")
                .iter()
                .map(|entry| entry.template)
                .collect::<Vec<_>>(),
            vec![NodeTemplate::Aggregate(AggregateOp::Join)]
        );
        assert_eq!(
            matching_entries("conditional")
                .iter()
                .map(|entry| entry.template)
                .collect::<Vec<_>>(),
            vec![NodeTemplate::If]
        );
        assert!(matching_entries("does-not-exist").is_empty());
    }

    #[test]
    fn keyboard_selection_stays_inside_the_filtered_result_set() {
        let mut state = PaletteState::default();
        state.move_selection(1, 3);
        state.move_selection(8, 3);
        assert_eq!(state.selected, 2);
        state.move_selection(-1, 3);
        state.move_selection(-8, 3);
        assert_eq!(state.selected, 0);
        state.move_selection(1, 0);
        assert_eq!(state.selected, 0);
    }
}
