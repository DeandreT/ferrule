//! Node property, binding panel, and scope setting edits recorded as typed
//! commands through the property editor and the Inspector.

use super::canvas::{pointer, press_key, text_position};
use super::wires::{args, ids};
use super::*;
use editor_ui::{Iteration, NodeProperty, ScopeSetting, ScopeSettingValue, Sorting, TargetBinding};
use mapping::{ScopeIteration, SequenceWindow};
use std::time::{Duration, Instant};

fn screen() -> Option<egui::Rect> {
    Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(1100.0, 1500.0),
    ))
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn shapes_at(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn collect(shape: &egui::epaint::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
        match shape {
            egui::epaint::Shape::Text(text)
                if crate::test_ui::inspector_label_matches(text.galley.text(), label) =>
            {
                positions.push(text.visual_bounding_rect().center());
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, label, positions);
                }
            }
            _ => {}
        }
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, label, &mut positions);
    }
    positions
}

fn press_and_release(position: egui::Pos2) -> [Vec<egui::Event>; 2] {
    [true, false].map(|pressed| {
        vec![
            egui::Event::PointerMoved(position),
            pointer(position, egui::PointerButton::Primary, pressed),
        ]
    })
}

/// Runs one frame of `node`'s canvas property editor, queues its typed edits
/// as the canvas host does, and observes the frame.
fn property_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    node: NodeId,
    events: Vec<egui::Event>,
    (now, coalesce): (Instant, bool),
) -> egui::FullOutput {
    let mut output = None;
    let (planned, error) = edit_canvas(app, MappingDocument::Main, |viewer, snarl| {
        let pin = snarl.out_pin(OutPinId {
            node: canvas_id(snarl, CanvasNode::Graph(node)),
            output: 0,
        });
        output = Some(context.run_test_ui(
            egui::RawInput {
                screen_rect: screen(),
                events,
                ..Default::default()
            },
            |ui| viewer.show_properties_for_test(&pin, ui, snarl),
        ));
    });
    assert_eq!(error, None);
    app.queue_canvas_edits(MappingDocument::Main, planned);
    app.observe_editor_history(now, coalesce);
    output.expect("the property editor ran")
}

fn click_property(
    app: &mut FerruleApp,
    context: &egui::Context,
    node: NodeId,
    label: &str,
    now: Instant,
) {
    let mut output = property_frame(app, context, node, Vec::new(), (now, false));
    for _ in 0..2 {
        output = property_frame(app, context, node, Vec::new(), (now, false));
    }
    let position = *shapes_at(&output, label)
        .first()
        .unwrap_or_else(|| panic!("rendered property control {label:?}"));
    for events in press_and_release(position) {
        property_frame(app, context, node, events, (now, false));
    }
}

fn constant(app: &FerruleApp, node: NodeId) -> String {
    match app.project.graph.nodes.get(&node) {
        Some(Node::Const {
            value: Value::String(text),
        }) => text.clone(),
        other => panic!("node {node} is a string constant, got {other:?}"),
    }
}

#[test]
fn typing_a_constant_coalesces_into_one_typed_edit_per_pause() {
    let mut app = app();
    let context = context();
    let saved = key(&app);
    let start = Instant::now();
    let mut output = Vec::new();
    for _ in 0..3 {
        output = vec![property_frame(
            &mut app,
            &context,
            SEPARATOR,
            Vec::new(),
            (start, false),
        )];
    }
    let field = output[0]
        .shapes
        .iter()
        .find_map(|shape| text_position(&shape.shape, "-"))
        .expect("the constant's text is rendered");
    for events in press_and_release(field) {
        property_frame(&mut app, &context, SEPARATOR, events, (start, false));
    }
    assert_eq!(app.history.undo_len(), 0, "focusing a field is not an edit");

    let typing = |app: &mut FerruleApp, events, after: u64| {
        property_frame(
            app,
            &context,
            SEPARATOR,
            events,
            (start + Duration::from_millis(after), true),
        );
    };
    typing(
        &mut app,
        vec![press_key(egui::Key::End), egui::Event::Text("x".into())],
        0,
    );
    typing(&mut app, vec![egui::Event::Text("y".into())], 100);
    assert_eq!(constant(&app, SEPARATOR), "-xy");
    assert_eq!(app.history.undo_len(), 1, "continued typing is one step");
    assert_eq!(app.history.undo_label(), Some("Edit constant"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.document(),
        &GraphDocument::Target(TargetId::Primary)
    );
    assert_eq!(
        command.action(),
        &GraphAction::EditNode {
            node: SEPARATOR,
            property: NodeProperty::Constant
        }
    );
    assert_eq!(ids(command.patch().changed_nodes()), [SEPARATOR]);
    assert_eq!(command.patch().nodes().count(), 1);
    let typed = key(&app);

    // Typing after a pause starts a new step.
    typing(&mut app, vec![egui::Event::Text("z".into())], 700);
    assert_eq!(app.history.undo_len(), 2);
    let retyped = key(&app);

    app.undo_project();
    assert_eq!(app.status, "undid Edit constant");
    assert_eq!(key(&app), typed);
    app.undo_project();
    assert_eq!(key(&app), saved);
    assert!(!app.observed_dirty() && !app.is_dirty());
    app.redo_project();
    app.redo_project();
    assert_eq!(app.status, "redid Edit constant");
    assert_eq!(key(&app), retyped);
}

#[test]
fn function_call_inputs_are_typed_edits_naming_their_hidden_inputs() {
    let mut app = app();
    let context = context();
    let now = Instant::now();
    let saved = key(&app);

    click_property(&mut app, &context, JOINED, "+arg", now);
    assert_eq!(args(&app, JOINED), [UPPER, SEPARATOR, REPLACEMENT]);
    assert!(matches!(
        app.project.graph.nodes.get(&REPLACEMENT),
        Some(Node::Unconnected)
    ));
    assert_eq!(app.history.undo_label(), Some("Edit function call"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::EditNode {
            node: JOINED,
            property: NodeProperty::FunctionCall
        }
    );
    assert_eq!(ids(command.patch().changed_nodes()), [JOINED]);
    assert_eq!(ids(command.patch().added_nodes()), [REPLACEMENT]);
    let added = key(&app);

    click_property(&mut app, &context, JOINED, "-arg", now);
    assert_eq!(args(&app, JOINED), [UPPER, SEPARATOR]);
    assert_eq!(app.history.undo_len(), 2);
    let command = next_undo_command(&app);
    assert_eq!(ids(command.patch().removed_nodes()), [REPLACEMENT]);
    let removed = key(&app);
    assert_eq!(removed, saved);

    app.undo_project();
    assert_eq!(app.status, "undid Edit function call");
    assert_eq!(key(&app), added);
    app.undo_project();
    assert_eq!(key(&app), saved);
    app.redo_project();
    assert_eq!(key(&app), added);
    app.redo_project();
    assert_eq!(key(&app), removed);
}

const SCORE: NodeId = 0;
const NAME: NodeId = 1;
const TWO: NodeId = 2;
const LABEL: NodeId = 3;

/// A `Rows` scope iterating source rows and binding `Name`, selected in the
/// Inspector, with an integer and a string constant.
fn scope_app() -> FerruleApp {
    let rows = |fields: [&str; 2]| {
        SchemaNode::group(
            "Rows",
            fields
                .into_iter()
                .map(|field| SchemaNode::scalar(field, ScalarType::String))
                .collect(),
        )
        .repeating()
    };
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Source", vec![rows(["Score", "Name"])]);
    app.project.target = SchemaNode::group("Target", vec![rows(["Name", "Score"])]);
    app.project.source_path = Some("input.xml".into());
    for (id, field) in [(SCORE, "Score"), (NAME, "Name")] {
        app.project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec!["Rows".into(), field.into()],
                frame: None,
            },
        );
    }
    app.project.graph.nodes.insert(
        TWO,
        Node::Const {
            value: Value::Int(2),
        },
    );
    app.project.graph.nodes.insert(LABEL, separator("-"));
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![binding("Name", NAME)],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    app
}

fn rows() -> ScopeLocation {
    ScopeLocation::of_children(&[0])
}

fn inspector_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_test_ui(
        egui::RawInput {
            screen_rect: screen(),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, true),
    );
    app.observe_editor_history(Instant::now(), false);
    output
}

/// Clicks the `occurrence`-th rendered Inspector label, or the last one.
fn click(app: &mut FerruleApp, context: &egui::Context, label: &str, occurrence: Option<usize>) {
    // Undo clears the scope selection with the rebuilt canvases.
    app.selected_scope = vec![0];
    let mut output = inspector_frame(app, context, Vec::new());
    for _ in 0..3 {
        output = inspector_frame(app, context, Vec::new());
    }
    let positions = shapes_at(&output, label);
    let position = match occurrence {
        Some(index) => positions.get(index),
        None => positions.last(),
    }
    .copied()
    .unwrap_or_else(|| panic!("rendered Inspector control {label:?}"));
    for events in press_and_release(position) {
        inspector_frame(app, context, events);
    }
}

fn rows_scope(app: &FerruleApp) -> &Scope {
    &app.project.root.children[0]
}

fn target_binding(field: &str, node: NodeId) -> TargetBinding {
    TargetBinding {
        field: field.into(),
        node,
    }
}

fn assert_only_bindings_of_rows(command: &GraphCommand) {
    let patch = command.patch();
    assert_eq!(patch.nodes().count(), 0);
    assert_eq!(patch.settings().count(), 0);
    let scopes = patch.scopes().map(|(scope, _)| scope).collect::<Vec<_>>();
    assert_eq!(scopes, [&rows()]);
}

/// Undoes every step back to `states[0]` and redoes them all, checking each
/// state and status label.
fn assert_history(app: &mut FerruleApp, states: &[String], labels: &[&str]) {
    for (state, label) in states.iter().rev().skip(1).zip(labels.iter().rev()) {
        app.undo_project();
        assert_eq!(app.status, format!("undid {label}"));
        assert_eq!(&key(app), state);
    }
    assert!(!app.observed_dirty() && !app.is_dirty());
    for (state, label) in states.iter().skip(1).zip(labels) {
        app.redo_project();
        assert_eq!(app.status, format!("redid {label}"));
        assert_eq!(&key(app), state);
    }
}

#[test]
fn binding_panel_additions_retargets_and_removals_are_typed_commands() {
    let mut app = scope_app();
    let context = context();
    let mut states = vec![key(&app)];

    click(&mut app, &context, "+ binding", Some(0));
    assert_eq!(app.history.undo_label(), Some("Add binding"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::AddBinding {
            scope: rows(),
            binding: target_binding("Score", SCORE),
        }
    );
    assert_only_bindings_of_rows(command);
    states.push(key(&app));

    click(&mut app, &context, "0: field Rows/Score", Some(0));
    click(&mut app, &context, "1: field Rows/Name", None);
    assert_eq!(app.history.undo_label(), Some("Retarget binding"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::RetargetBinding {
            scope: rows(),
            from: target_binding("Score", SCORE),
            to: target_binding("Score", NAME),
        }
    );
    assert_only_bindings_of_rows(command);
    states.push(key(&app));

    click(&mut app, &context, "x", Some(0));
    assert_eq!(app.history.undo_label(), Some("Remove binding"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::RemoveBinding {
            scope: rows(),
            binding: target_binding("Name", NAME),
        }
    );
    assert_only_bindings_of_rows(command);
    assert_eq!(
        rows_scope(&app)
            .bindings
            .iter()
            .map(TargetBinding::of)
            .collect::<Vec<_>>(),
        [target_binding("Score", NAME)]
    );
    states.push(key(&app));
    assert_eq!(app.history.undo_len(), 3);

    assert_history(
        &mut app,
        &states,
        &["Add binding", "Retarget binding", "Remove binding"],
    );
}

fn setting_change(
    app: &FerruleApp,
    setting: ScopeSetting,
) -> (ScopeSettingValue, ScopeSettingValue) {
    let command = next_undo_command(app);
    assert_eq!(
        command.action(),
        &GraphAction::EditScope {
            scope: rows(),
            setting
        }
    );
    let patch = command.patch();
    assert_eq!(patch.nodes().count() + patch.scopes().count(), 0);
    assert_eq!(patch.settings().count(), 1);
    let change = patch.setting(&rows(), setting).expect("the named setting");
    (change.before().clone(), change.after().clone())
}

#[test]
fn scope_setting_edits_are_typed_commands_naming_scope_and_setting() {
    let mut app = scope_app();
    let context = context();
    let mut states = vec![key(&app)];

    click(&mut app, &context, "filtered", Some(0));
    assert_eq!(app.history.undo_label(), Some("Change scope filter"));
    assert_eq!(
        setting_change(&app, ScopeSetting::Filter),
        (
            ScopeSettingValue::Filter(None),
            ScopeSettingValue::Filter(Some(SCORE))
        )
    );
    states.push(key(&app));

    click(&mut app, &context, "sorted", Some(0));
    assert_eq!(app.history.undo_label(), Some("Change scope sort"));
    assert_eq!(
        setting_change(&app, ScopeSetting::Sort),
        (
            ScopeSettingValue::Sort(Sorting::default()),
            ScopeSettingValue::Sort(Sorting {
                by: Some(SCORE),
                ..Sorting::default()
            })
        )
    );
    states.push(key(&app));

    click(&mut app, &context, "+", Some(0));
    assert_eq!(app.history.undo_label(), Some("Change scope windows"));
    assert_eq!(
        setting_change(&app, ScopeSetting::Windows),
        (
            ScopeSettingValue::Windows(Vec::new()),
            ScopeSettingValue::Windows(vec![SequenceWindow::First { count: SCORE }])
        )
    );
    states.push(key(&app));

    click(&mut app, &context, "iterates", Some(0));
    assert_eq!(app.history.undo_label(), Some("Change scope iteration"));
    assert_eq!(
        setting_change(&app, ScopeSetting::Iteration),
        (
            ScopeSettingValue::Iteration(Iteration::Source(vec!["Rows".into()])),
            ScopeSettingValue::Iteration(Iteration::None)
        )
    );
    states.push(key(&app));

    assert_history(
        &mut app,
        &states,
        &[
            "Change scope filter",
            "Change scope sort",
            "Change scope windows",
            "Change scope iteration",
        ],
    );
}

#[test]
fn edit_ops_parity_for_scope_controls_and_binding_retarget() {
    use edit_ops::{
        BindingRef, EditOperation, Expression, Index, ScopeRef, SourceCollection, Target,
    };
    let at = || ScopeRef {
        target: Target::Primary {},
        children: vec![Index(0)],
    };
    let compare = |before: mapping::Project, app: &FerruleApp, operation| {
        let mut session = edit_ops::EditSession::new(before.clone());
        session.apply(operation).unwrap();
        assert_eq!(
            crate::project_state::model_key(session.project()),
            crate::project_state::model_key(&app.project)
        );
        session.undo().unwrap();
        assert_eq!(
            crate::project_state::model_key(session.project()),
            crate::project_state::model_key(&before)
        );
    };
    let mut app = scope_app();
    let ctx = context();
    let before = app.project.clone();
    click(&mut app, &ctx, "filtered", Some(0));
    compare(
        before,
        &app,
        EditOperation::SetFilter {
            scope: at(),
            node: Some(edit_ops::NodeId(SCORE)),
        },
    );
    let before = app.project.clone();
    click(&mut app, &ctx, "sorted", Some(0));
    compare(
        before,
        &app,
        EditOperation::SetSort {
            scope: at(),
            keys: vec![edit_ops::SortKey {
                node: edit_ops::NodeId(SCORE),
                descending: false,
            }],
        },
    );
    let before = app.project.clone();
    click(&mut app, &ctx, "iterates", Some(0));
    compare(
        before,
        &app,
        EditOperation::SetIteration {
            scope: at(),
            source: None,
        },
    );
    let before = app.project.clone();
    click(&mut app, &ctx, "iterates", Some(0));
    compare(
        before,
        &app,
        EditOperation::SetIteration {
            scope: at(),
            source: Some(SourceCollection::Legacy { path: vec![] }),
        },
    );
    let mut app = scope_app();
    let ctx = context();
    let before = app.project.clone();
    click(&mut app, &ctx, "+ binding", Some(0));
    compare(
        before,
        &app,
        EditOperation::Bind {
            binding: BindingRef {
                scope: at(),
                field: "Score".into(),
            },
            expression: Expression::Existing {
                node: edit_ops::NodeId(SCORE),
            },
        },
    );
    let before = app.project.clone();
    click(&mut app, &ctx, "0: field Rows/Score", Some(0));
    click(&mut app, &ctx, "1: field Rows/Name", None);
    compare(
        before,
        &app,
        EditOperation::RetargetBinding {
            binding: BindingRef {
                scope: at(),
                field: "Score".into(),
            },
            to: BindingRef {
                scope: at(),
                field: "Score".into(),
            },
            node: edit_ops::NodeId(NAME),
        },
    );
    let mut app = scope_app();
    let ctx = context();
    let before = app.project.clone();
    click(&mut app, &ctx, "none", Some(0));
    click(&mut app, &ctx, "key", None);
    compare(
        before,
        &app,
        EditOperation::SetGrouping {
            scope: at(),
            grouping: edit_ops::Grouping::By {
                node: edit_ops::NodeId(SCORE),
            },
        },
    );
    let mut app = scope_app();
    let ctx = context();
    let before = app.project.clone();
    click(&mut app, &ctx, "x", Some(0));
    compare(
        before,
        &app,
        EditOperation::RemoveBinding {
            binding: BindingRef {
                scope: at(),
                field: "Name".into(),
            },
        },
    );
}

#[test]
fn property_binding_and_scope_commands_interleave_with_earlier_commands_and_snapshots() {
    let mut app = scope_app();
    let context = context();
    let now = Instant::now();
    let mut states = vec![key(&app)];
    let kind = |app: &FerruleApp| match app.history.next_undo() {
        Some(HistoryRecord::Snapshot(_)) => "snapshot",
        Some(HistoryRecord::Command(_)) => "command",
        None => "none",
    };
    let mut kinds = Vec::new();

    click(&mut app, &context, "filtered", Some(0));
    kinds.push(kind(&app));
    states.push(key(&app));

    // An in-place edit outside the migrated families stays a snapshot.
    app.project.root.children[0].merge_dynamic_fields = true;
    app.observe_editor_history(now, false);
    kinds.push(kind(&app));
    states.push(key(&app));

    let planned =
        remove_nodes(&mut app, MappingDocument::Main, &[TWO]).expect("removal forms a typed edit");
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    kinds.push(kind(&app));
    states.push(key(&app));

    click(&mut app, &context, "+ binding", Some(0));
    kinds.push(kind(&app));
    states.push(key(&app));

    let output = property_frame(&mut app, &context, LABEL, Vec::new(), (now, false));
    let field = output
        .shapes
        .iter()
        .find_map(|shape| text_position(&shape.shape, "-"))
        .expect("the constant's text is rendered");
    for events in press_and_release(field) {
        property_frame(&mut app, &context, LABEL, events, (now, false));
    }
    property_frame(
        &mut app,
        &context,
        LABEL,
        vec![press_key(egui::Key::End), egui::Event::Text("X".into())],
        (now, true),
    );
    assert_eq!(constant(&app, LABEL), "-X");
    kinds.push(kind(&app));
    states.push(key(&app));

    assert_eq!(
        kinds,
        ["command", "snapshot", "command", "command", "command"]
    );
    assert_history(
        &mut app,
        &states,
        &[
            "Change scope filter",
            "Edit mapping",
            "Remove node",
            "Add binding",
            "Edit constant",
        ],
    );
}

#[test]
fn scope_edits_sharing_a_frame_with_untyped_edits_stay_snapshots() {
    let mut app = scope_app();
    let context = context();
    let saved = key(&app);
    // Rendering selects the scope; the edit then shares the click frame.
    for _ in 0..3 {
        inspector_frame(&mut app, &context, Vec::new());
    }
    let output = inspector_frame(&mut app, &context, Vec::new());
    let filtered = shapes_at(&output, "filtered")[0];
    let [press, release] = press_and_release(filtered);
    inspector_frame(&mut app, &context, press);
    app.project.root.children[0].merge_dynamic_fields = true;
    inspector_frame(&mut app, &context, release);

    assert_eq!(rows_scope(&app).filter, Some(SCORE));
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Edit mapping"));
    app.undo_project();
    assert_eq!(key(&app), saved, "the snapshot restores both edits");
}

#[test]
fn stale_scope_commands_refuse_undo_without_changes() {
    let mut app = scope_app();
    let context = context();
    click(&mut app, &context, "filtered", Some(0));
    app.observed_editor.project.root.children[0].filter = Some(NAME);
    let project = key(&app);

    app.undo_project();
    assert_eq!(app.status, "Undo failed");
    assert_eq!(key(&app), project);
    assert_eq!((app.history.undo_len(), app.history.redo_len()), (1, 0));
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("Filter of scope"))
    );
}

#[test]
fn scope_commands_follow_semantic_revisions_and_key_the_project_once_more() {
    let mut app = scope_app();
    let context = context();
    let saved = app.revisions.current();
    for _ in 0..3 {
        inspector_frame(&mut app, &context, Vec::new());
    }
    let output = inspector_frame(&mut app, &context, Vec::new());
    let filtered = shapes_at(&output, "filtered")[0];
    let [press, release] = press_and_release(filtered);

    // Input without an edit keys the project once, in the observer.
    let keys = project_keys_computed();
    inspector_frame(&mut app, &context, press);
    assert_eq!(project_keys_computed() - keys, 1);
    // The edit frame adds one key proving the command explains it.
    let keys = project_keys_computed();
    inspector_frame(&mut app, &context, release);
    assert_eq!(project_keys_computed() - keys, 2);
    assert_eq!(app.history.undo_label(), Some("Change scope filter"));

    let edited = app.revisions.current();
    assert_ne!(edited.semantic(), saved.semantic());
    assert_eq!(edited.layout(), saved.layout());
    assert!(app.revisions.is_semantically_dirty());
    app.undo_project();
    assert_eq!(app.revisions.current().semantic(), saved.semantic());
    assert!(!app.observed_dirty() && !app.is_dirty());
    app.redo_project();
    assert!(app.revisions.is_semantically_dirty());
    assert_eq!(app.revisions.current().layout(), saved.layout());
}
