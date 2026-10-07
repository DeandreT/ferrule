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
    SourceRootField,
    SourceRootXmlTypeEquals,
    Position,
    HostInput,
    HostInputDefault,
    Builtin(&'static str),
    If,
    Raise,
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

const STRUCTURAL_ENTRIES: [PaletteEntry; 19] = [
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
        label: "Primary source field",
        keywords: "root exact source field required XML",
        documentation: "Reads one field from the primary source root. Its required setting is checked only when the field is evaluated.",
        template: NodeTemplate::SourceRootField,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Primary XML type comparison",
        keywords: "root XML type annotation condition equality",
        documentation: "Compares the XML type annotation actually read on the primary source root with one declared type.",
        template: NodeTemplate::SourceRootXmlTypeEquals,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Position",
        keywords: "index row item collection",
        documentation: "Reads the one-based position in a collection.",
        template: NodeTemplate::Position,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Host input",
        keywords: "host runtime parameter run value required named input",
        documentation: "Reads a named value supplied when the mapping runs.",
        template: NodeTemplate::HostInput,
    },
    PaletteEntry {
        category: Category::Input,
        label: "Host input with default",
        keywords: "host runtime parameter run value optional fallback default named input",
        documentation: "Uses a named run value when supplied, otherwise its connected default.",
        template: NodeTemplate::HostInputDefault,
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
        category: Category::Logic,
        label: "Raise error",
        keywords: "exception abort fail optional message",
        documentation: "Stops the mapping with an error only when this expression is reached. Its message input is optional.",
        template: NodeTemplate::Raise,
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

pub(super) fn show_available(
    ui: &mut Ui,
    root_fields: bool,
    root_types: bool,
) -> Option<NodeTemplate> {
    #[cfg(test)]
    tests::begin_palette_response_capture();
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
            // The palette handles Enter after the search field processes text.
            .return_key(None)
            .desired_width(f32::INFINITY),
    );
    if newly_opened {
        search.request_focus();
    }

    let matches = matching_entries(&state.query)
        .into_iter()
        .filter(|entry| match entry.template {
            NodeTemplate::SourceRootField => root_fields,
            NodeTemplate::SourceRootXmlTypeEquals => root_types,
            _ => true,
        })
        .collect::<Vec<_>>();
    if search.changed() {
        state.selected = 0;
    }
    let mut keyboard_navigation = false;
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
        keyboard_navigation = up || down;
    }
    state.selected = state.selected.min(matches.len().saturating_sub(1));
    let reveal_selection = keyboard_navigation.then_some(state.selected);
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
                let mut selected_response = None;
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
                    #[cfg(test)]
                    tests::observe_palette_response(entry.label, &response, ui.clip_rect());
                    if reveal_selection == Some(index) {
                        selected_response = Some(response.clone());
                    }
                    if response.hovered() {
                        state.selected = index;
                    }
                    if response.clicked() {
                        chosen = Some(entry.template);
                    }
                }
                // Keep pointer selection authoritative when it differs from navigation.
                if reveal_selection == Some(state.selected)
                    && let Some(response) = selected_response
                {
                    response.scroll_to_me(None);
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
    let normalized_query = terms.join(" ");
    let mut matches = entries()
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
        .collect::<Vec<_>>();
    matches.sort_by_key(|entry| {
        let label = entry
            .label
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .join(" ");
        if label == normalized_query {
            0
        } else if matches!(entry.template, NodeTemplate::Builtin(name)
            if name.eq_ignore_ascii_case(&normalized_query))
        {
            1
        } else {
            2
        }
    });
    matches
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
            NodeTemplate::HostInput,
            NodeTemplate::HostInputDefault,
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
            matching_entries("host input")
                .iter()
                .map(|entry| entry.template)
                .collect::<Vec<_>>(),
            vec![NodeTemplate::HostInput, NodeTemplate::HostInputDefault]
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

    #[test]
    fn keyboard_root_creation_choices_follow_the_active_canvas_policy() {
        for (query, expected) in [
            ("Primary source field", NodeTemplate::SourceRootField),
            (
                "Primary XML type comparison",
                NodeTemplate::SourceRootXmlTypeEquals,
            ),
        ] {
            for allowed in [false, true] {
                let context = egui::Context::default();
                let mut selected = None;
                let mut run = |events| {
                    let _ = context.run_ui(
                        egui::RawInput {
                            events,
                            ..Default::default()
                        },
                        |ui| selected = show_available(ui, allowed, allowed),
                    );
                };
                run(Vec::new());
                run(vec![egui::Event::Text(query.into())]);
                run(vec![egui::Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
                assert_eq!(
                    selected,
                    allowed.then_some(expected),
                    "{query}: allowed={allowed}"
                );
            }
        }
    }

    fn palette_frame(context: &egui::Context, events: Vec<egui::Event>) -> Option<NodeTemplate> {
        let mut selected = None;
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| selected = show_available(ui, true, true),
        );
        selected
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn exact_display_labels_precede_documentation_matches_and_accept_return() {
        for query in ["value map", "VALUE MAP", "  VaLuE   mAp  "] {
            let matches = matching_entries(query);
            assert_eq!(
                matches.first().map(|entry| entry.template),
                Some(NodeTemplate::ValueMap)
            );
            assert!(
                matches
                    .iter()
                    .skip(1)
                    .any(|entry| entry.template == NodeTemplate::HostInput)
            );
            let context = egui::Context::default();
            assert_eq!(palette_frame(&context, Vec::new()), None);
            assert_eq!(
                palette_frame(&context, vec![egui::Event::Text(query.into())]),
                None
            );
            let selected = palette_frame(&context, vec![key(Key::Enter)]);
            eprintln!("query={query:?}, returned={selected:?}");
            assert_eq!(selected, Some(NodeTemplate::ValueMap));
        }
    }

    #[test]
    fn exact_builtin_native_alias_accepts_return_without_changing_catalog_identity() {
        for (query, expected) in [
            (
                "  NORMALIZE_SPACE  ",
                NodeTemplate::Builtin("normalize_space"),
            ),
            ("upper", NodeTemplate::Builtin("upper")),
        ] {
            assert_eq!(
                matching_entries(query).first().map(|entry| entry.template),
                Some(expected)
            );
            let context = egui::Context::default();
            assert_eq!(palette_frame(&context, Vec::new()), None);
            assert_eq!(
                palette_frame(&context, vec![egui::Event::Text(query.into())]),
                None
            );
            let selected = palette_frame(&context, vec![key(Key::Enter)]);
            eprintln!("alias={query:?}, returned={selected:?}");
            assert_eq!(selected, Some(expected));
        }
    }

    #[test]
    fn partial_and_empty_searches_keep_catalog_order_and_zero_matches_do_not_choose() {
        assert_eq!(matching_entries(" \t\n "), entries());
        assert_eq!(
            matching_entries("host")
                .iter()
                .map(|entry| entry.template)
                .collect::<Vec<_>>(),
            vec![NodeTemplate::HostInput, NodeTemplate::HostInputDefault],
        );
        let context = egui::Context::default();
        assert_eq!(palette_frame(&context, Vec::new()), None);
        assert_eq!(
            palette_frame(&context, vec![egui::Event::Text("does-not-exist".into())]),
            None
        );
        let selected = palette_frame(&context, vec![key(Key::Enter)]);
        eprintln!("zero-match Return={selected:?}");
        assert_eq!(selected, None);
        assert_eq!(
            palette_frame(&context, vec![key(Key::ArrowDown), key(Key::ArrowUp)]),
            None
        );
    }

    #[test]
    fn explicit_arrow_selection_still_overrides_the_exact_match_default() {
        for move_back in [false, true] {
            let context = egui::Context::default();
            assert_eq!(palette_frame(&context, Vec::new()), None);
            assert_eq!(
                palette_frame(&context, vec![egui::Event::Text("value map".into())]),
                None
            );
            assert_eq!(palette_frame(&context, vec![key(Key::ArrowDown)]), None);
            if move_back {
                assert_eq!(palette_frame(&context, vec![key(Key::ArrowUp)]), None);
            }
            let selected = palette_frame(&context, vec![key(Key::Enter)]);
            eprintln!("exact-query arrow selection: move_back={move_back}, returned={selected:?}");
            assert_eq!(
                selected,
                Some(if move_back {
                    NodeTemplate::ValueMap
                } else {
                    NodeTemplate::HostInput
                })
            );
        }
    }

    #[derive(Clone, Debug)]
    struct PaletteRowObservation {
        label: &'static str,
        response: egui::Response,
        clip: egui::Rect,
    }

    thread_local! {
        static PALETTE_RESPONSE_CAPTURE:
            std::cell::RefCell<Option<Vec<PaletteRowObservation>>> = const {
                std::cell::RefCell::new(None)
            };
    }

    pub(super) fn begin_palette_response_capture() {
        PALETTE_RESPONSE_CAPTURE.with(|capture| {
            if let Some(rows) = capture.borrow_mut().as_mut() {
                rows.clear();
            }
        });
    }

    pub(super) fn observe_palette_response(
        label: &'static str,
        response: &egui::Response,
        clip: egui::Rect,
    ) {
        PALETTE_RESPONSE_CAPTURE.with(|capture| {
            if let Some(rows) = capture.borrow_mut().as_mut() {
                rows.push(PaletteRowObservation {
                    label,
                    response: response.clone(),
                    clip,
                });
            }
        });
    }

    struct OverflowPaletteFrame {
        chosen: Option<NodeTemplate>,
        rows: Vec<PaletteRowObservation>,
        output: egui::FullOutput,
    }

    fn overflow_palette_frame(
        context: &egui::Context,
        events: Vec<egui::Event>,
        time: &mut f64,
    ) -> OverflowPaletteFrame {
        *time += 0.1;
        PALETTE_RESPONSE_CAPTURE.with(|capture| *capture.borrow_mut() = Some(Vec::new()));
        let mut chosen = None;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 240.0),
                )),
                time: Some(*time),
                events,
                ..Default::default()
            },
            |ui| {
                let canvas = ui.allocate_rect(ui.max_rect(), egui::Sense::click());
                canvas.context_menu(|ui| {
                    chosen = show_available(ui, true, true);
                    if chosen.is_some() {
                        ui.close();
                    }
                });
            },
        );
        let rows = PALETTE_RESPONSE_CAPTURE.with(|capture| capture.borrow_mut().take().unwrap());
        OverflowPaletteFrame {
            chosen,
            rows,
            output,
        }
    }

    fn pointer_button(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn open_overflow_palette(context: &egui::Context, time: &mut f64) -> OverflowPaletteFrame {
        let anchor = egui::pos2(360.0, 180.0);
        // egui hit-tests widgets from the preceding pass before processing a press.
        let registered = overflow_palette_frame(context, Vec::new(), time);
        assert_eq!(registered.chosen, None);
        let hovered =
            overflow_palette_frame(context, vec![egui::Event::PointerMoved(anchor)], time);
        assert_eq!(hovered.chosen, None);
        let first = overflow_palette_frame(
            context,
            vec![
                egui::Event::PointerMoved(anchor),
                pointer_button(anchor, egui::PointerButton::Secondary, true),
            ],
            time,
        );
        assert_eq!(first.chosen, None);
        let released = overflow_palette_frame(
            context,
            vec![pointer_button(
                anchor,
                egui::PointerButton::Secondary,
                false,
            )],
            time,
        );
        assert_eq!(released.chosen, None);
        let away = overflow_palette_frame(context, vec![egui::Event::PointerGone], time);
        assert_eq!(away.chosen, None);
        settle_overflow_palette(context, time)
    }

    fn settle_overflow_palette(context: &egui::Context, time: &mut f64) -> OverflowPaletteFrame {
        for _ in 0..2 {
            let frame = overflow_palette_frame(context, Vec::new(), time);
            assert_eq!(frame.chosen, None);
        }
        overflow_palette_frame(context, Vec::new(), time)
    }

    fn visible_selected_palette_row<'a>(
        context: &egui::Context,
        frame: &'a OverflowPaletteFrame,
    ) -> &'a PaletteRowObservation {
        let update = frame
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("actual popup accessibility update");
        let selected = frame
            .rows
            .iter()
            .filter(|row| {
                update.nodes.iter().any(|(id, node)| {
                    *id == row.response.id.accesskit_id()
                        && node.role() == egui::accesskit::Role::Button
                        && node.toggled() == Some(egui::accesskit::Toggled::True)
                })
            })
            .collect::<Vec<_>>();
        eprintln!(
            "real palette rows={}, selected={:?}",
            frame.rows.len(),
            selected
                .iter()
                .map(|row| (row.label, row.response.id, row.response.rect, row.clip))
                .collect::<Vec<_>>()
        );
        assert_eq!(selected.len(), 1, "exactly one rendered selected result");
        let row = selected[0];
        let actual = context
            .read_response(row.response.id)
            .expect("registered result response");
        assert_eq!(actual.rect, row.response.rect);
        assert!(
            row.clip.contains_rect(actual.rect),
            "selected {} response {:?} must fit its actual scroll clip {:?}",
            row.label,
            actual.rect,
            row.clip
        );
        row
    }

    fn overflow_palette_context() -> egui::Context {
        let context = egui::Context::default();
        context.enable_accesskit();
        context
            .all_styles_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
        context
    }

    #[test]
    fn keyboard_navigation_reveals_real_overflow_results_down_and_up_before_return() {
        let context = overflow_palette_context();
        let mut time = 0.0;
        let initial = open_overflow_palette(&context, &mut time);
        assert!(initial.rows.len() > 18, "actual full catalog must overflow");
        let item_at = initial
            .rows
            .iter()
            .find(|row| row.label == "Item at")
            .unwrap();
        eprintln!(
            "initial offscreen Item at: {:?}, clip={:?}",
            item_at.response.rect, item_at.clip
        );
        assert!(!item_at.clip.contains_rect(item_at.response.rect));
        assert_eq!(
            visible_selected_palette_row(&context, &initial).label,
            "Constant"
        );
        for _ in 0..18 {
            let moved = overflow_palette_frame(&context, vec![key(Key::ArrowDown)], &mut time);
            assert_eq!(moved.chosen, None);
            let settled = settle_overflow_palette(&context, &mut time);
            visible_selected_palette_row(&context, &settled);
        }
        let bottom = settle_overflow_palette(&context, &mut time);
        assert_eq!(
            visible_selected_palette_row(&context, &bottom).label,
            "Item at"
        );
        for _ in 0..12 {
            let moved = overflow_palette_frame(&context, vec![key(Key::ArrowUp)], &mut time);
            assert_eq!(moved.chosen, None);
            let settled = settle_overflow_palette(&context, &mut time);
            visible_selected_palette_row(&context, &settled);
        }
        let top = settle_overflow_palette(&context, &mut time);
        assert_eq!(
            visible_selected_palette_row(&context, &top).label,
            "Host input with default"
        );
        let returned = overflow_palette_frame(&context, vec![key(Key::Enter)], &mut time);
        eprintln!("overflow Up/Down Return={:?}", returned.chosen);
        assert_eq!(returned.chosen, Some(NodeTemplate::HostInputDefault));
    }

    #[test]
    fn reopened_short_popup_reveals_partial_match_after_exact_query_navigation() {
        let context = overflow_palette_context();
        let mut time = 0.0;
        open_overflow_palette(&context, &mut time);
        let alias = overflow_palette_frame(
            &context,
            vec![egui::Event::Text("normalize_space".into())],
            &mut time,
        );
        assert_eq!(alias.chosen, None);
        let narrowed = settle_overflow_palette(&context, &mut time);
        assert_eq!(narrowed.rows.len(), 1);
        let returned = overflow_palette_frame(&context, vec![key(Key::Enter)], &mut time);
        assert_eq!(
            returned.chosen,
            Some(NodeTemplate::Builtin("normalize_space"))
        );
        open_overflow_palette(&context, &mut time);
        let query = overflow_palette_frame(
            &context,
            vec![egui::Event::Text("value map".into())],
            &mut time,
        );
        assert_eq!(query.chosen, None);
        let exact = settle_overflow_palette(&context, &mut time);
        assert_eq!(exact.rows.len(), 2);
        assert_eq!(
            visible_selected_palette_row(&context, &exact).label,
            "Value map"
        );
        let down = overflow_palette_frame(&context, vec![key(Key::ArrowDown)], &mut time);
        assert_eq!(down.chosen, None);
        let revealed = settle_overflow_palette(&context, &mut time);
        assert_eq!(
            visible_selected_palette_row(&context, &revealed).label,
            "Host input"
        );
        let returned = overflow_palette_frame(&context, vec![key(Key::Enter)], &mut time);
        eprintln!("reopened exact-query Down/Return={:?}", returned.chosen);
        assert_eq!(returned.chosen, Some(NodeTemplate::HostInput));
    }

    #[test]
    fn manual_wheel_and_pointer_selection_remain_free_after_keyboard_reveal() {
        let context = overflow_palette_context();
        let mut time = 0.0;
        open_overflow_palette(&context, &mut time);
        for _ in 0..18 {
            overflow_palette_frame(&context, vec![key(Key::ArrowDown)], &mut time);
            settle_overflow_palette(&context, &mut time);
        }
        let before = settle_overflow_palette(&context, &mut time);
        let item_at = visible_selected_palette_row(&context, &before);
        assert_eq!(item_at.label, "Item at");
        let before_constant = before
            .rows
            .iter()
            .find(|row| row.label == "Constant")
            .unwrap()
            .response
            .rect;
        let wheel = overflow_palette_frame(
            &context,
            vec![
                egui::Event::PointerMoved(item_at.response.rect.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 75.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            &mut time,
        );
        assert_eq!(wheel.chosen, None);
        overflow_palette_frame(&context, vec![egui::Event::PointerGone], &mut time);
        let scrolled = settle_overflow_palette(&context, &mut time);
        let after_constant = scrolled
            .rows
            .iter()
            .find(|row| row.label == "Constant")
            .unwrap()
            .response
            .rect;
        eprintln!("manual wheel: before={before_constant:?}, after={after_constant:?}");
        assert!(after_constant.top() > before_constant.top() + 20.0);
        let idle = settle_overflow_palette(&context, &mut time);
        let idle_constant = idle
            .rows
            .iter()
            .find(|row| row.label == "Constant")
            .unwrap()
            .response
            .rect;
        assert!(
            (idle_constant.top() - after_constant.top()).abs() < 0.1,
            "no-key idle frames must not force the keyboard-selected row back into view"
        );
        // Sum is five aggregate rows above the keyboard-revealed Item at.
        // A 75-point wheel keeps its whole row inside this short popup.
        let label = "Sum";
        let expected = NodeTemplate::Aggregate(AggregateOp::Sum);
        let row = idle
            .rows
            .iter()
            .find(|row| row.label == label)
            .expect("literal Sum creation row");
        assert!(
            row.clip.contains_rect(row.response.rect),
            "literal Sum creation row {:?} must fit its actual scroll clip {:?}",
            row.response.rect,
            row.clip
        );
        let pos = row.response.rect.center();
        overflow_palette_frame(
            &context,
            vec![
                egui::Event::PointerMoved(pos),
                pointer_button(pos, egui::PointerButton::Primary, true),
            ],
            &mut time,
        );
        let clicked = overflow_palette_frame(
            &context,
            vec![pointer_button(pos, egui::PointerButton::Primary, false)],
            &mut time,
        );
        eprintln!(
            "manual pointer label={label:?}, returned={:?}",
            clicked.chosen
        );
        assert_eq!(clicked.chosen, Some(expected));
    }
}
