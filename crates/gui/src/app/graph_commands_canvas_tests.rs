//! Typed edit commands driven through the real rendered main canvas.

use super::wires::{args, ids};
use super::*;
use editor_ui::WireSink;

fn canvas_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    modifiers: egui::Modifiers,
    mut events: Vec<egui::Event>,
) -> egui::FullOutput {
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let output = context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            events,
            ..Default::default()
        },
        |ui| app.show_mapping_workspace_canvas(ui, true),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn text_position(shape: &egui::epaint::Shape, wanted: &str) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text().contains(wanted) => {
            Some(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            shapes.iter().find_map(|shape| text_position(shape, wanted))
        }
        _ => None,
    }
}

#[test]
fn deleting_a_selected_canvas_node_records_one_command_step() {
    let mut app = app();
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut output = canvas_frame(&mut app, &context, egui::Modifiers::NONE, Vec::new());
    for _ in 0..3 {
        output = canvas_frame(&mut app, &context, egui::Modifiers::NONE, Vec::new());
    }
    assert_eq!(app.history.undo_len(), 0, "rendering alone is not an edit");
    let title = output
        .shapes
        .iter()
        .find_map(|shape| text_position(&shape.shape, "Uppercase"))
        .expect("the upper node title is rendered");
    for pressed in [true, false] {
        let _ = canvas_frame(
            &mut app,
            &context,
            egui::Modifiers::SHIFT,
            vec![
                egui::Event::PointerMoved(title),
                egui::Event::PointerButton {
                    pos: title,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
        );
    }
    let saved = key(&app);
    let _ = canvas_frame(
        &mut app,
        &context,
        egui::Modifiers::NONE,
        vec![egui::Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    assert!(!app.project.graph.nodes.contains_key(&UPPER));
    assert_eq!(app.history.undo_len(), 1);
    assert_removal_identities(
        next_undo_command(&app),
        GraphDocument::Target(TargetId::Primary),
    );
    app.undo_project();
    assert_eq!(key(&app), saved);
    assert!(!app.observed_dirty());
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    canvas_frame(app, context, egui::Modifiers::NONE, events)
}

fn settle(app: &mut FerruleApp, context: &egui::Context) -> egui::FullOutput {
    let mut output = frame(app, context, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, Vec::new());
    }
    output
}

fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn press_key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// The fixture laid out without overlap, rendered, and saved.
fn laid_out() -> (FerruleApp, egui::Context) {
    let mut app = app();
    for (node, pos) in [
        (CanvasNode::SourceBlock(0), egui::pos2(20.0, 40.0)),
        (CanvasNode::Graph(UPPER), egui::pos2(320.0, 40.0)),
        (CanvasNode::Graph(SEPARATOR), egui::pos2(320.0, 320.0)),
        (CanvasNode::Graph(JOINED), egui::pos2(620.0, 220.0)),
        (CanvasNode::TargetBlock(0), egui::pos2(900.0, 40.0)),
    ] {
        move_node(&mut app.main_canvas.snarl, node, pos);
    }
    let context = egui::Context::default();
    crate::icons::install(&context);
    settle(&mut app, &context);
    app.mark_clean();
    app.rebase_history();
    (app, context)
}

/// The screen center of one rendered pin: the `index`-th from the top on
/// the input or output edge of `wanted`.
fn pin(
    app: &FerruleApp,
    context: &egui::Context,
    wanted: CanvasNode,
    output: bool,
    index: usize,
) -> egui::Pos2 {
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        context,
        app.embedded_canvas_id(egui::Id::unique("main_mapping_canvas")),
        app.main_canvas.view_generation,
    );
    let responses = ids
        .into_iter()
        .filter_map(|id| context.read_response(id))
        .collect::<Vec<_>>();
    let node = canvas_id(&app.main_canvas.snarl, wanted);
    let frame = responses
        .iter()
        .find_map(|pin| {
            context.read_response(pin.layer_id.id.with(("snarl-node", node)).with("frame"))
        })
        .expect("the node frame is rendered");
    let bounds = context
        .layer_transform_to_global(frame.layer_id)
        .unwrap_or_default()
        * frame.rect;
    let edge = if output {
        bounds.right()
    } else {
        bounds.left()
    };
    let mut pins = responses
        .iter()
        .filter(|pin| pin.sense.senses_drag())
        .map(|pin| {
            context
                .layer_transform_to_global(pin.layer_id)
                .unwrap_or_default()
                * pin.rect.center()
        })
        .filter(|center| bounds.expand(16.0).contains(*center) && (center.x - edge).abs() < 20.0)
        .collect::<Vec<_>>();
    pins.sort_by(|left, right| left.y.total_cmp(&right.y));
    *pins
        .get(index)
        .unwrap_or_else(|| panic!("pin {index} of {wanted:?} among {pins:?}"))
}

fn drag(app: &mut FerruleApp, context: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    let primary = egui::PointerButton::Primary;
    frame(app, context, vec![egui::Event::PointerMoved(from)]);
    frame(app, context, vec![pointer(from, primary, true)]);
    frame(
        app,
        context,
        vec![egui::Event::PointerMoved(from.lerp(to, 0.5))],
    );
    frame(app, context, vec![egui::Event::PointerMoved(to)]);
    frame(app, context, vec![pointer(to, primary, false)]);
}

fn assert_round_trip(app: &mut FerruleApp, label: &str, before: &str, after: &str) {
    app.undo_project();
    assert_eq!(app.status, format!("undid {label}"));
    assert_eq!(key(app), before);
    assert!(!app.observed_dirty() && !app.is_dirty());
    assert_eq!(app.history.redo_label(), Some(label));
    app.redo_project();
    assert_eq!(app.status, format!("redid {label}"));
    assert_eq!(key(app), after);
    assert!(app.observed_dirty());
}

#[test]
fn dragging_a_canvas_wire_records_one_connect_command() {
    let (mut app, context) = laid_out();
    let saved = key(&app);
    let from = pin(&app, &context, CanvasNode::Graph(SEPARATOR), true, 0);
    let to = pin(&app, &context, CanvasNode::Graph(UPPER), false, 0);
    drag(&mut app, &context, from, to);
    settle(&mut app, &context);

    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Connect wire"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::ConnectWire {
            source: SEPARATOR,
            sink: WireSink::NodeInput {
                node: UPPER,
                input: 0
            }
        }
    );
    assert_eq!(ids(command.patch().changed_nodes()), [UPPER]);
    assert_eq!(ids(command.patch().removed_nodes()), [SOURCE]);
    assert_eq!(args(&app, UPPER), [SEPARATOR]);
    let connected = key(&app);
    assert_round_trip(&mut app, "Connect wire", &saved, &connected);
}

#[test]
fn right_clicking_a_canvas_wire_records_one_disconnect_command() {
    let (mut app, context) = laid_out();
    let saved = key(&app);
    let from = pin(&app, &context, CanvasNode::Graph(SEPARATOR), true, 0);
    let to = pin(&app, &context, CanvasNode::Graph(JOINED), false, 1);
    let wire = from.lerp(to, 0.5);
    let secondary = egui::PointerButton::Secondary;
    frame(&mut app, &context, vec![egui::Event::PointerMoved(wire)]);
    frame(&mut app, &context, vec![pointer(wire, secondary, true)]);
    frame(&mut app, &context, vec![pointer(wire, secondary, false)]);
    settle(&mut app, &context);

    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Disconnect wire"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::DisconnectWire {
            sink: WireSink::NodeInput {
                node: JOINED,
                input: 1
            }
        }
    );
    assert_eq!(ids(command.patch().added_nodes()), [REPLACEMENT]);
    assert_eq!(args(&app, JOINED), [UPPER, REPLACEMENT]);
    let disconnected = key(&app);
    assert_round_trip(&mut app, "Disconnect wire", &saved, &disconnected);
}

#[test]
fn inserting_a_palette_node_records_one_insert_command_with_its_position() {
    let (mut app, context) = laid_out();
    let saved = key(&app);
    let empty = egui::pos2(600.0, 700.0);
    let secondary = egui::PointerButton::Secondary;
    frame(&mut app, &context, vec![egui::Event::PointerMoved(empty)]);
    frame(&mut app, &context, vec![pointer(empty, secondary, true)]);
    frame(&mut app, &context, vec![pointer(empty, secondary, false)]);
    frame(&mut app, &context, Vec::new());
    frame(
        &mut app,
        &context,
        vec![egui::Event::Text("Constant".into())],
    );
    frame(&mut app, &context, Vec::new());
    frame(&mut app, &context, vec![press_key(egui::Key::Enter)]);
    settle(&mut app, &context);

    const INSERTED: NodeId = REPLACEMENT;
    assert!(matches!(
        app.project.graph.nodes.get(&INSERTED),
        Some(Node::Const { value: Value::Null })
    ));
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Insert node"));
    let command = next_undo_command(&app);
    let GraphAction::InsertNode {
        node,
        position: placed,
    } = *command.action()
    else {
        panic!("an insertion, got {:?}", command.action());
    };
    assert_eq!(node, INSERTED);
    assert_eq!(ids(command.patch().added_nodes()), [INSERTED]);
    assert_eq!(command.patch().scopes().count(), 0);
    let placed = egui::pos2(placed.x, placed.y);
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(INSERTED)),
        Some(placed),
        "the command names the inserted node's canvas position"
    );
    let inserted = key(&app);

    app.undo_project();
    assert_eq!(app.status, "undid Insert node");
    assert_eq!(key(&app), saved);
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(INSERTED)),
        None
    );
    app.redo_project();
    assert_eq!(app.status, "redid Insert node");
    assert_eq!(key(&app), inserted);
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(INSERTED)),
        Some(placed)
    );
}
