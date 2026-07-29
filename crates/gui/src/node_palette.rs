use egui::{Key, Ui};
use functions::{BuiltinCategory, BuiltinDefinition, BuiltinExposure};
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
    Builtin(&'static str),
    If,
    ValueMap,
    Lookup,
    CollectionFind,
    Aggregate(AggregateOp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Input,
    Transform,
    Logic,
    Collection,
    Aggregate,
    Builtin(BuiltinCategory),
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::Input => "Input & values",
            Self::Transform => "Transform",
            Self::Logic => "Logic",
            Self::Collection => "Collections",
            Self::Aggregate => "Aggregates",
            Self::Builtin(BuiltinCategory::Boolean) => "Functions: Boolean",
            Self::Builtin(BuiltinCategory::String) => "Functions: String",
            Self::Builtin(BuiltinCategory::Numeric) => "Functions: Numeric",
            Self::Builtin(BuiltinCategory::DateTime) => "Functions: Date & time",
            Self::Builtin(BuiltinCategory::Path) => "Functions: Paths",
            Self::Builtin(BuiltinCategory::Json) => "Functions: JSON",
            Self::Builtin(BuiltinCategory::FlexText) => "Functions: FlexText",
            Self::Builtin(BuiltinCategory::Generator) => "Functions: Generators",
            Self::Builtin(BuiltinCategory::Conversion) => "Functions: Conversion",
            Self::Builtin(BuiltinCategory::Validation) => "Functions: Validation",
            Self::Builtin(BuiltinCategory::Internal) => "Functions: Internal",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaletteEntry {
    category: Category,
    label: &'static str,
    keywords: &'static str,
    documentation: &'static str,
    template: NodeTemplate,
}

const STRUCTURAL_ENTRIES: [PaletteEntry; 14] = [
    PaletteEntry {
        category: Category::Input,
        label: "Constant",
        keywords: "const literal value null string number boolean",
        documentation: "Adds one editable scalar literal.",
        template: NodeTemplate::Constant,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Source field (manual path)",
        keywords: "source input field path",
        documentation: "Reads one source field using an editable path.",
        template: NodeTemplate::SourceField,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Position",
        keywords: "index row item collection",
        documentation: "Reads the one-based position in a collection.",
        template: NodeTemplate::Position,
    },
    PaletteEntry {
        category: Category::Transform,
        label: "Value map",
        keywords: "lookup table translate replace default",
        documentation: "Translates scalar values through an editable table.",
        template: NodeTemplate::ValueMap,
    },
    PaletteEntry {
        category: Category::Logic,
        label: "If",
        keywords: "condition then else branch conditional",
        documentation: "Evaluates only the selected conditional branch.",
        template: NodeTemplate::If,
    },
    PaletteEntry {
        category: Category::Collection,
        label: "Lookup",
        keywords: "collection key match value reference",
        documentation: "Finds a matching item in a source collection.",
        template: NodeTemplate::Lookup,
    },
    PaletteEntry {
        category: Category::Collection,
        label: "Find in collection",
        keywords: "search predicate select item value",
        documentation: "Finds the first collection item selected by a predicate.",
        template: NodeTemplate::CollectionFind,
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Count",
        keywords: "aggregate total size",
        documentation: "Counts items in a source collection.",
        template: NodeTemplate::Aggregate(AggregateOp::Count),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Sum",
        keywords: "aggregate total add numeric",
        documentation: "Sums numeric values from a source collection.",
        template: NodeTemplate::Aggregate(AggregateOp::Sum),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Average",
        keywords: "aggregate avg mean numeric",
        documentation: "Averages numeric values from a source collection.",
        template: NodeTemplate::Aggregate(AggregateOp::Avg),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Minimum",
        keywords: "aggregate min smallest",
        documentation: "Returns the minimum collection value.",
        template: NodeTemplate::Aggregate(AggregateOp::Min),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Maximum",
        keywords: "aggregate max largest",
        documentation: "Returns the maximum collection value.",
        template: NodeTemplate::Aggregate(AggregateOp::Max),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "String join",
        keywords: "aggregate concatenate separator text",
        documentation: "Joins collection values with a separator.",
        template: NodeTemplate::Aggregate(AggregateOp::Join),
    },
    PaletteEntry {
        category: Category::Aggregate,
        label: "Item at",
        keywords: "aggregate index select position",
        documentation: "Returns a one-based collection item.",
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
                    let label = builtin_definition(entry.template).map_or_else(
                        || entry.label.to_owned(),
                        |builtin| format!("{}  ·  {}", entry.label, arity_label(builtin)),
                    );
                    let response = ui
                        .selectable_label(index == state.selected, label)
                        .on_hover_ui(|ui| show_entry_documentation(ui, entry));
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

fn entries() -> Vec<PaletteEntry> {
    let mut entries = STRUCTURAL_ENTRIES.to_vec();
    entries.extend(
        functions::builtin_catalog()
            .iter()
            .filter(|builtin| builtin.exposure == BuiltinExposure::Authoring)
            .map(|builtin| PaletteEntry {
                category: Category::Builtin(builtin.category),
                label: builtin.display_name,
                keywords: builtin.native_name,
                documentation: builtin.documentation,
                template: NodeTemplate::Builtin(builtin.native_name),
            }),
    );
    entries.sort_by_key(|entry| entry.category);
    entries
}

fn matching_entries(query: &str) -> Vec<PaletteEntry> {
    let terms: Vec<_> = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    entries()
        .into_iter()
        .filter(|entry| {
            let signature = builtin_definition(entry.template)
                .map(signature)
                .unwrap_or_default();
            let haystack = format!(
                "{} {} {} {} {}",
                entry.category.label(),
                entry.label,
                entry.keywords,
                entry.documentation,
                signature,
            )
            .to_ascii_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

fn builtin_definition(template: NodeTemplate) -> Option<&'static BuiltinDefinition> {
    let NodeTemplate::Builtin(name) = template else {
        return None;
    };
    functions::builtin(name)
}

fn arity_label(builtin: &BuiltinDefinition) -> String {
    let minimum = builtin.arity.minimum();
    match builtin.arity.maximum() {
        Some(maximum) if minimum == maximum => argument_count(minimum),
        Some(maximum) => format!("{minimum}-{maximum} arguments"),
        None if builtin.arity.step() == Some(1) => format!("at least {minimum} arguments"),
        None => format!(
            "{minimum}+ arguments in groups of {}",
            builtin.arity.step().unwrap_or(1)
        ),
    }
}

fn argument_count(count: usize) -> String {
    format!("{count} argument{}", if count == 1 { "" } else { "s" })
}

fn signature(builtin: &BuiltinDefinition) -> String {
    let parameters = builtin
        .parameters
        .iter()
        .map(|parameter| parameter.name)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({parameters})", builtin.native_name)
}

fn show_entry_documentation(ui: &mut Ui, entry: &PaletteEntry) {
    ui.strong(entry.label);
    if let Some(builtin) = builtin_definition(entry.template) {
        ui.monospace(signature(builtin));
        ui.weak(arity_label(builtin));
    }
    ui.label(entry.documentation);
}

#[cfg(test)]
pub(super) fn templates() -> impl Iterator<Item = NodeTemplate> {
    entries().into_iter().map(|entry| entry.template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_preserves_every_pre_palette_creation_action() {
        let templates: Vec<_> = entries().iter().map(|entry| entry.template).collect();
        for expected in [
            NodeTemplate::Constant,
            NodeTemplate::SourceField,
            NodeTemplate::Position,
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
        assert!(templates.contains(&NodeTemplate::Builtin("concat")));
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
        assert_eq!(
            matching_entries("UPPERCASE")
                .iter()
                .map(|entry| entry.template)
                .collect::<Vec<_>>(),
            vec![NodeTemplate::Builtin("upper")]
        );
        assert_eq!(
            matching_entries("whitespace runs")
                .iter()
                .map(|entry| entry.template)
                .collect::<Vec<_>>(),
            vec![NodeTemplate::Builtin("normalize_space")]
        );
        assert!(matching_entries("does-not-exist").is_empty());
    }

    #[test]
    fn builtin_entries_are_authoritative_grouped_and_hide_internal_functions() {
        let entries = entries();
        let actual = entries
            .iter()
            .filter_map(|entry| match entry.template {
                NodeTemplate::Builtin(name) => Some(name),
                _ => None,
            })
            .collect::<Vec<_>>();
        let expected = functions::builtin_catalog()
            .iter()
            .filter(|builtin| builtin.exposure == BuiltinExposure::Authoring)
            .map(|builtin| builtin.native_name)
            .collect::<Vec<_>>();

        assert_eq!(
            actual
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
        );
        assert!(!actual.contains(&"json_parse_field"));
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].category <= pair[1].category)
        );
    }

    #[test]
    fn builtin_arity_labels_cover_fixed_range_and_variadic_shapes() {
        let Some(upper) = functions::builtin("upper") else {
            panic!("upper metadata is missing");
        };
        assert_eq!(arity_label(upper), "1 argument");
        let Some(concat) = functions::builtin("concat") else {
            panic!("concat metadata is missing");
        };
        assert_eq!(arity_label(concat), "at least 0 arguments");
        let Some(matches) = functions::builtin("matches") else {
            panic!("matches metadata is missing");
        };
        assert_eq!(arity_label(matches), "2-3 arguments");
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
