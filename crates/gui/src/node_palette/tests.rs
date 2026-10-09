use super::*;

#[test]
fn source_document_path_keyboard_creation_follows_the_primary_file_set_capability() {
    for allowed in [false, true] {
        let context = egui::Context::default();
        let mut selected = None;
        let mut run = |events| {
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| selected = show_available(ui, true, true, true, allowed, true, false),
            );
            eprintln!(
                "Document-path palette capability={allowed}, original selection={selected:?}, shapes={}",
                output.shapes.len()
            );
        };
        run(Vec::new());
        run(vec![egui::Event::Text("Source document path".into())]);
        run(vec![egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(
            selected,
            allowed.then_some(NodeTemplate::SourceDocumentPath)
        );
    }
}

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
fn runtime_query_discovers_exact_run_values_and_existing_host_inputs() {
    for query in ["runtime", "RuNtImE"] {
        let matches = matching_entries(query);
        let values = matches
            .iter()
            .filter_map(|entry| match entry.template {
                NodeTemplate::RuntimeValue(value) => Some((entry.label, value)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 3, "{query}: {values:?}");
        for expected in [
            ("Mapping path", RuntimeValue::MappingFilePath),
            ("Main mapping path", RuntimeValue::MainMappingFilePath),
            ("Run date and time", RuntimeValue::CurrentDateTime),
        ] {
            assert!(values.contains(&expected), "{query}: missing {expected:?}");
        }
        for expected in [NodeTemplate::HostInput, NodeTemplate::HostInputDefault] {
            assert!(
                matches.iter().any(|entry| entry.template == expected),
                "{query}: missing {expected:?}"
            );
        }
    }
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
                    |ui| {
                        selected =
                            show_available(ui, allowed, allowed, allowed, allowed, allowed, false)
                    },
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
        |ui| selected = show_available(ui, true, true, true, true, true, false),
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
                chosen = show_available(ui, true, true, true, true, true, false);
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
    let hovered = overflow_palette_frame(context, vec![egui::Event::PointerMoved(anchor)], time);
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
    context.all_styles_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
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
    for _ in 0..24 {
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
    for _ in 0..16 {
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
    assert_eq!(exact.rows.len(), 4);
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
    for _ in 0..24 {
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

#[test]
fn filter_map_keyboard_creation_requires_project_stage_capability() {
    for (label, expected) in [
        ("Filter/map item at", NodeTemplate::FilterMapItemAt),
        ("Filter/map any", NodeTemplate::FilterMapExists),
        ("Filter/map sum", NodeTemplate::FilterMapSum),
    ] {
        for allowed in [false, true] {
            let context = egui::Context::default();
            let mut selected = None;
            let mut run = |events| {
                let output = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| selected = show_available(ui, false, false, false, false, false, allowed),
                );
                eprintln!(
                    "filter/map keyboard capability={allowed} original selected={selected:?} full shapes={:#?}",
                    output.shapes
                );
            };
            run(Vec::new());
            run(vec![egui::Event::Text(label.into())]);
            run(vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(selected, allowed.then_some(expected));
        }
    }
}
