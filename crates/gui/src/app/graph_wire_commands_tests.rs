//! Wire connection and disconnection commands on primary and named target
//! canvases, mixed with removals and snapshots.

use super::*;
use editor_ui::WireSink;
use egui_snarl::ui::SnarlViewer as _;

const UPPER_PIN: usize = 0;
const JOINED_PIN: usize = 1;

fn primary() -> GraphDocument {
    GraphDocument::Target(TargetId::Primary)
}

fn root_binding(field: &str) -> WireSink {
    WireSink::Binding {
        scope: ScopeLocation::root(),
        field: field.into(),
    }
}

fn single(planned: Vec<PlannedGraphEdit>) -> PlannedGraphEdit {
    let [planned] = <[PlannedGraphEdit; 1]>::try_from(planned)
        .unwrap_or_else(|planned| panic!("one typed edit, got {}", planned.len()));
    planned
}

/// Drops a wire through the viewer's canvas connect entry point.
fn connect(
    app: &mut FerruleApp,
    document: MappingDocument,
    (from, output): (CanvasNode, usize),
    (to, input): (CanvasNode, usize),
) -> PlannedGraphEdit {
    let (planned, error) = edit_canvas(app, document, |viewer, snarl| {
        let from = snarl.out_pin(OutPinId {
            node: canvas_id(snarl, from),
            output,
        });
        let to = snarl.in_pin(InPinId {
            node: canvas_id(snarl, to),
            input,
        });
        viewer.connect(&from, &to, snarl);
    });
    assert_eq!(error, None);
    single(planned)
}

/// Removes a wire through the viewer's canvas disconnect entry point.
fn disconnect(
    app: &mut FerruleApp,
    document: MappingDocument,
    (from, output): (CanvasNode, usize),
    (to, input): (CanvasNode, usize),
) -> PlannedGraphEdit {
    let (planned, error) = edit_canvas(app, document, |viewer, snarl| {
        let from = snarl.out_pin(OutPinId {
            node: canvas_id(snarl, from),
            output,
        });
        let to = snarl.in_pin(InPinId {
            node: canvas_id(snarl, to),
            input,
        });
        viewer.disconnect(&from, &to, snarl);
    });
    assert_eq!(error, None);
    single(planned)
}

fn record(app: &mut FerruleApp, document: MappingDocument, planned: PlannedGraphEdit) {
    assert!(observe_planned(app, document, planned));
}

pub(super) fn args(app: &FerruleApp, node: NodeId) -> Vec<NodeId> {
    match app.project.graph.nodes.get(&node) {
        Some(Node::Call { args, .. }) => args.clone(),
        other => panic!("node {node} is a call, got {other:?}"),
    }
}

pub(super) fn ids(nodes: impl Iterator<Item = NodeId>) -> Vec<NodeId> {
    nodes.collect()
}

fn bindings(scope: &Scope) -> Vec<(String, NodeId)> {
    scope
        .bindings
        .iter()
        .map(|binding| (binding.target_field.clone(), binding.node))
        .collect()
}

#[test]
fn node_input_connection_is_one_command_with_complete_undo_and_redo() {
    let mut app = app();
    let saved = key(&app);
    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    record(&mut app, MappingDocument::Main, planned);
    let connected = key(&app);
    assert_eq!(args(&app, UPPER), [SEPARATOR]);
    assert!(!app.project.graph.nodes.contains_key(&SOURCE));
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Connect wire"));
    let assert_identities = |command: &GraphCommand| {
        assert_eq!(command.document(), &primary());
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
        let patch = command.patch();
        assert_eq!(ids(patch.changed_nodes()), [UPPER]);
        assert_eq!(
            ids(patch.removed_nodes()),
            [SOURCE],
            "the displaced source field is orphaned"
        );
        assert_eq!(patch.added_nodes().count(), 0);
        assert_eq!(patch.scopes().count(), 0);
    };
    assert_identities(next_undo_command(&app));
    assert!(app.observed_dirty() && app.revisions.is_semantically_dirty());

    app.undo_project();
    assert_eq!(app.status, "undid Connect wire");
    assert_eq!(key(&app), saved);
    assert_eq!(args(&app, UPPER), [SOURCE]);
    assert!(!app.is_dirty() && !app.observed_dirty());
    assert_identities(next_redo_command(&app));

    app.redo_project();
    assert_eq!(app.status, "redid Connect wire");
    assert_eq!(key(&app), connected);
    assert!(app.is_dirty() && app.observed_dirty());
    assert_identities(next_undo_command(&app));
}

#[test]
fn target_binding_wires_connect_and_disconnect_as_commands() {
    let mut app = app();
    let saved = key(&app);
    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::SourceBlock(0), 0),
        (CanvasNode::TargetBlock(0), JOINED_PIN),
    );
    record(&mut app, MappingDocument::Main, planned);
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::ConnectWire {
            source: SOURCE,
            sink: root_binding("Joined")
        },
        "the existing source field backs the source pin"
    );
    assert_eq!(command.patch().nodes().count(), 0);
    let scopes = command.patch().scopes().collect::<Vec<_>>();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].0, &ScopeLocation::root());
    assert_eq!(
        bindings(&app.project.root),
        [("Upper".into(), UPPER), ("Joined".into(), SOURCE)]
    );
    let rebound = key(&app);

    let planned = disconnect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(UPPER), 0),
        (CanvasNode::TargetBlock(0), UPPER_PIN),
    );
    record(&mut app, MappingDocument::Main, planned);
    assert_eq!(app.history.undo_label(), Some("Disconnect wire"));
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::DisconnectWire {
            sink: root_binding("Upper")
        }
    );
    assert_eq!(command.patch().nodes().count(), 0);
    assert_eq!(bindings(&app.project.root), [("Joined".into(), SOURCE)]);
    let unbound = key(&app);

    app.undo_project();
    assert_eq!(app.status, "undid Disconnect wire");
    assert_eq!(key(&app), rebound);
    app.undo_project();
    assert_eq!(app.status, "undid Connect wire");
    assert_eq!(key(&app), saved);
    assert!(!app.observed_dirty());
    app.redo_project();
    app.redo_project();
    assert_eq!(app.status, "redid Disconnect wire");
    assert_eq!(key(&app), unbound);
}

#[test]
fn node_input_disconnection_rewires_to_a_hidden_unconnected_input() {
    let mut app = app();
    let saved = key(&app);
    let planned = disconnect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(JOINED), 1),
    );
    record(&mut app, MappingDocument::Main, planned);
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
    assert_eq!(ids(command.patch().changed_nodes()), [JOINED]);
    assert_eq!(
        command.patch().removed_nodes().count(),
        0,
        "the shown constant is not orphaned"
    );
    assert_eq!(args(&app, JOINED), [UPPER, REPLACEMENT]);
    let disconnected = key(&app);

    app.undo_project();
    assert_eq!(app.status, "undid Disconnect wire");
    assert_eq!(key(&app), saved);
    assert!(!app.project.graph.nodes.contains_key(&REPLACEMENT));
    app.redo_project();
    assert_eq!(key(&app), disconnected);
}

#[test]
fn named_target_wires_record_the_target_identity() {
    let mut app = FerruleApp::default();
    app.project.source =
        SchemaNode::group("Source", vec![SchemaNode::scalar("A", ScalarType::String)]);
    app.project.target = target_schema("Target");
    let (graph, root) = graph_parts();
    app.project.graph = graph;
    app.project.root = root.clone();
    app.project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.xml".into()),
        schema: target_schema("Audit"),
        options: Default::default(),
        root,
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    assert!(app.ensure_target_canvas(0));
    app.mark_clean();
    app.rebase_history();
    let saved = key(&app);

    let planned = connect(
        &mut app,
        MappingDocument::Target(0),
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::TargetBlock(0), UPPER_PIN),
    );
    record(&mut app, MappingDocument::Target(0), planned);
    let named = GraphDocument::Target(TargetId::Named("audit".into()));
    let command = next_undo_command(&app);
    assert_eq!(command.document(), &named);
    assert_eq!(
        command.action(),
        &GraphAction::ConnectWire {
            source: SEPARATOR,
            sink: root_binding("Upper")
        }
    );
    assert_eq!(
        bindings(&app.project.extra_targets[0].root),
        [("Upper".into(), SEPARATOR), ("Joined".into(), JOINED)]
    );
    assert_eq!(
        bindings(&app.project.root),
        [("Upper".into(), UPPER), ("Joined".into(), JOINED)],
        "the primary target is untouched"
    );

    let planned = disconnect(
        &mut app,
        MappingDocument::Target(0),
        (CanvasNode::Graph(JOINED), 0),
        (CanvasNode::TargetBlock(0), JOINED_PIN),
    );
    record(&mut app, MappingDocument::Target(0), planned);
    assert_eq!(next_undo_command(&app).document(), &named);
    assert_eq!(
        bindings(&app.project.extra_targets[0].root),
        [("Upper".into(), SEPARATOR)]
    );

    app.undo_project();
    app.undo_project();
    assert_eq!(app.status, "undid Connect wire");
    assert_eq!(key(&app), saved);
    assert!(!app.observed_dirty());
}

#[test]
fn wire_commands_interleave_with_removals_and_snapshots() {
    let mut app = app();
    let now = std::time::Instant::now();
    let mut states = vec![key(&app)];

    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    app.observe_editor_history(now, false);
    states.push(key(&app));
    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    record(&mut app, MappingDocument::Main, planned);
    states.push(key(&app));
    record_removal(&mut app, MappingDocument::Main, &[JOINED]);
    states.push(key(&app));
    let moved = egui::pos2(64.0, 32.0);
    move_node(&mut app.main_canvas.snarl, CanvasNode::Graph(UPPER), moved);
    app.observe_editor_history(now, false);
    let planned = disconnect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(UPPER), 0),
        (CanvasNode::TargetBlock(0), UPPER_PIN),
    );
    record(&mut app, MappingDocument::Main, planned);
    states.push(key(&app));
    assert!(app.project.root.bindings.is_empty());

    let mut undone = Vec::new();
    while app.history.can_undo() {
        let kind = match app.history.next_undo() {
            Some(HistoryRecord::Command(_)) => "command",
            _ => "snapshot",
        };
        undone.push((kind, app.history.undo_label().map(str::to_owned)));
        app.undo_project();
        undone.push(("state", Some(key(&app))));
    }
    assert_eq!(
        undone,
        [
            ("command", Some("Disconnect wire".into())),
            ("state", Some(states[3].clone())),
            ("snapshot", Some("Edit layout".into())),
            ("state", Some(states[3].clone())),
            ("command", Some("Remove node".into())),
            ("state", Some(states[2].clone())),
            ("command", Some("Connect wire".into())),
            ("state", Some(states[1].clone())),
            ("snapshot", Some("Edit mapping".into())),
            ("state", Some(states[0].clone())),
        ]
    );
    assert!(!app.observed_dirty() && !app.is_dirty());

    let mut redone = Vec::new();
    while app.history.can_redo() {
        app.redo_project();
        redone.push((app.status.clone(), key(&app)));
    }
    assert_eq!(
        redone,
        [
            ("redid Edit mapping".into(), states[1].clone()),
            ("redid Connect wire".into(), states[2].clone()),
            ("redid Remove node".into(), states[3].clone()),
            ("redid Edit layout".into(), states[3].clone()),
            ("redid Disconnect wire".into(), states[4].clone()),
        ]
    );
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(UPPER)),
        Some(moved),
        "command steps restore the layout their state had"
    );

    // A new wire command after a partial undo discards the mixed redo branch.
    app.undo_project();
    app.undo_project();
    app.undo_project();
    assert_eq!(key(&app), states[2]);
    let planned = disconnect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(UPPER), 0),
        (CanvasNode::TargetBlock(0), UPPER_PIN),
    );
    record(&mut app, MappingDocument::Main, planned);
    assert!(!app.history.can_redo());
    app.undo_project();
    assert_eq!(key(&app), states[2]);
}

#[test]
fn wire_commands_sharing_a_frame_with_untyped_edits_stay_snapshots() {
    // An in-place edit in the same frame.
    let mut app = app();
    let saved = key(&app);
    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    assert!(!observe_planned(&mut app, MappingDocument::Main, planned));
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Edit mapping"));
    assert!(app.frame_commands.is_empty());
    app.undo_project();
    assert_eq!(key(&app), saved, "the snapshot restores both edits");

    // Two typed edits and an untyped one in the same frame.
    let mut app = self::app();
    let first = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    let second = disconnect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(JOINED), 0),
        (CanvasNode::TargetBlock(0), JOINED_PIN),
    );
    app.queue_canvas_edits(MappingDocument::Main, vec![first, second]);
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.history.undo_len(), 1);
    assert!(matches!(
        app.history.next_undo(),
        Some(HistoryRecord::Snapshot(_))
    ));
    app.undo_project();
    assert_eq!(key(&app), saved);

    // A queued edit whose action does not match its patch is dropped.
    let mut app = self::app();
    let mut planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    planned.action = GraphAction::DisconnectWire {
        sink: root_binding("Upper"),
    };
    assert!(!observe_planned(&mut app, MappingDocument::Main, planned));
    assert_eq!(app.history.undo_label(), Some("Edit mapping"));
}

#[test]
fn typed_edits_sharing_a_frame_form_one_grouped_step() {
    let mut app = app();
    let saved = key(&app);
    let first = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    let second = disconnect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(JOINED), 0),
        (CanvasNode::TargetBlock(0), JOINED_PIN),
    );
    app.queue_canvas_edits(MappingDocument::Main, vec![first, second]);
    app.observe_editor_history(std::time::Instant::now(), false);
    let edited = key(&app);
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("2 edits"));
    let actions = next_undo_commands(&app)
        .iter()
        .map(|command| command.action().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        [
            GraphAction::ConnectWire {
                source: SEPARATOR,
                sink: WireSink::NodeInput {
                    node: UPPER,
                    input: 0
                }
            },
            GraphAction::DisconnectWire {
                sink: root_binding("Joined")
            },
        ]
    );

    app.undo_project();
    assert_eq!(app.status, "undid 2 edits");
    assert_eq!(key(&app), saved);
    assert!(!app.observed_dirty() && !app.is_dirty());
    app.redo_project();
    assert_eq!(app.status, "redid 2 edits");
    assert_eq!(key(&app), edited);

    // A stale later command refuses the whole step without partial effects.
    app.observed_editor.project.root.bindings.clear();
    let project = key(&app);
    app.undo_project();
    assert_eq!(app.status, "Undo failed");
    assert_eq!(key(&app), project);
    assert_eq!((app.history.undo_len(), app.history.redo_len()), (1, 0));
    assert_eq!(args(&app, UPPER), [SEPARATOR]);
}

#[test]
fn stale_wire_commands_refuse_undo_and_redo_without_changes() {
    let mut app = app();
    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    record(&mut app, MappingDocument::Main, planned);
    // The connected node no longer has the content the command produced.
    let lowered = Node::Call {
        function: "lower".into(),
        args: vec![SEPARATOR],
    };
    app.observed_editor
        .project
        .graph
        .nodes
        .insert(UPPER, lowered.clone());
    let project = key(&app);

    app.undo_project();
    assert_eq!(app.status, "Undo failed");
    assert_eq!(key(&app), project);
    assert_eq!((app.history.undo_len(), app.history.redo_len()), (1, 0));
    let stale = format!("mapping node {UPPER} changed since the edit");
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains(&stale))
    );

    // Restore the recorded content: undo succeeds, then a stale redo fails.
    app.observed_editor.project.graph.nodes.insert(
        UPPER,
        Node::Call {
            function: "upper".into(),
            args: vec![SEPARATOR],
        },
    );
    app.undo_project();
    assert_eq!(app.status, "undid Connect wire");
    app.observed_editor
        .project
        .graph
        .nodes
        .insert(UPPER, lowered);
    let project = key(&app);
    app.redo_project();
    assert_eq!(app.status, "Redo failed");
    assert_eq!(key(&app), project);
    assert_eq!((app.history.undo_len(), app.history.redo_len()), (0, 1));
}

#[test]
fn wire_commands_advance_only_the_semantic_revision() {
    let mut app = app();
    let saved = app.revisions.current();
    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    record(&mut app, MappingDocument::Main, planned);
    let connected = app.revisions.current();
    assert_ne!(connected.semantic(), saved.semantic());
    assert_eq!(connected.layout(), saved.layout());
    assert!(app.revisions.is_semantically_dirty());
    let fingerprints = app.fingerprints.computations();

    app.undo_project();
    assert_eq!(app.revisions.current(), saved);
    assert!(!app.observed_dirty());
    assert_eq!(app.fingerprints.computations(), fingerprints + 1);
    app.redo_project();
    let redone = app.revisions.current();
    assert_ne!(redone.semantic(), saved.semantic());
    assert_eq!(redone.layout(), saved.layout());
    assert!(app.revisions.is_semantically_dirty() && !app.revisions.is_layout_dirty());
    assert_eq!(app.fingerprints.computations(), fingerprints + 2);

    // Observing the recorded state again reuses its revision's fingerprint.
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.revisions.current(), redone);
    assert_eq!(app.fingerprints.computations(), fingerprints + 2);
}

#[test]
fn a_command_frame_keys_the_project_once_more_than_an_idle_frame() {
    let mut app = app();
    let now = std::time::Instant::now();
    let keys = project_keys_computed();
    app.observe_editor_history(now, false);
    assert_eq!(project_keys_computed() - keys, 1, "an idle frame");

    let planned = connect(
        &mut app,
        MappingDocument::Main,
        (CanvasNode::Graph(SEPARATOR), 0),
        (CanvasNode::Graph(UPPER), 0),
    );
    assert_eq!(
        project_keys_computed() - keys,
        1,
        "canvas planning compares graph parts, never the whole project"
    );
    record(&mut app, MappingDocument::Main, planned);
    assert_eq!(
        project_keys_computed() - keys,
        3,
        "the observation key and one proof that the command explains it"
    );
    app.observe_editor_history(now, false);
    assert_eq!(project_keys_computed() - keys, 4);
    assert_eq!(app.history.undo_len(), 1);

    // A snapshot frame without commands keys the project once.
    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    app.observe_editor_history(now, false);
    assert_eq!(project_keys_computed() - keys, 5);
    assert_eq!(app.history.undo_len(), 2);
}
