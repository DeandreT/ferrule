use super::super::*;
use super::*;
use crate::canvas::{project_source_blocks, target_blocks};
use crate::graph_viewer::GraphViewer;
use crate::path_picker::SourcePathCatalog;
use crate::project_state::{project_keys_computed, project_snapshot_key};
use crate::test_ui::ContextExt as _;
use editor_ui::{GraphAction, ScopeLocation};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget, Node};
use std::collections::BTreeSet;

const SOURCE: NodeId = 0;
const SEPARATOR: NodeId = 1;
const UPPER: NodeId = 2;
const JOINED: NodeId = 3;
/// The first identity the removal reserves for the rewired `concat` input.
const REPLACEMENT: NodeId = 4;

fn binding(field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}

fn separator(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}

/// Source `A` feeds `upper` (2), whose result feeds `concat` (3) and the
/// `Upper` binding. Removing `upper` drops that binding, rewires `concat` to
/// a hidden unconnected input, and removes the orphaned source field.
fn graph_parts() -> (mapping::Graph, Scope) {
    let mut graph = mapping::Graph::default();
    graph.nodes.insert(
        SOURCE,
        Node::SourceField {
            path: vec!["A".into()],
            frame: None,
        },
    );
    graph.nodes.insert(SEPARATOR, separator("-"));
    graph.nodes.insert(
        UPPER,
        Node::Call {
            function: "upper".into(),
            args: vec![SOURCE],
        },
    );
    graph.nodes.insert(
        JOINED,
        Node::Call {
            function: "concat".into(),
            args: vec![UPPER, SEPARATOR],
        },
    );
    let root = Scope {
        bindings: vec![binding("Upper", UPPER), binding("Joined", JOINED)],
        ..Scope::default()
    };
    (graph, root)
}

fn target_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Upper", ScalarType::String),
            SchemaNode::scalar("Joined", ScalarType::String),
        ],
    )
}

fn app() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source =
        SchemaNode::group("Source", vec![SchemaNode::scalar("A", ScalarType::String)]);
    app.project.target = target_schema("Target");
    app.project.source_path = Some("source.xml".into());
    let (graph, root) = graph_parts();
    app.project.graph = graph;
    app.project.root = root;
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    app
}

fn canvas_id(snarl: &Snarl<CanvasNode>, node: CanvasNode) -> egui_snarl::NodeId {
    snarl
        .node_ids()
        .find_map(|(id, &candidate)| (candidate == node).then_some(id))
        .expect("canvas node exists")
}

fn position(snarl: &Snarl<CanvasNode>, node: CanvasNode) -> Option<egui::Pos2> {
    snarl
        .nodes_pos()
        .find_map(|(pos, &candidate)| (candidate == node).then_some(pos))
}

fn move_node(snarl: &mut Snarl<CanvasNode>, node: CanvasNode, pos: egui::Pos2) {
    let id = canvas_id(snarl, node);
    snarl.get_node_info_mut(id).expect("canvas node exists").pos = pos;
}

fn key(app: &FerruleApp) -> String {
    project_snapshot_key(&app.project)
}

/// Runs one edit through the viewer of the main canvas or one named target
/// canvas, returning the typed edits it planned and any edit error.
fn edit_canvas(
    app: &mut FerruleApp,
    document: MappingDocument,
    edit: impl FnOnce(&mut GraphViewer<'_>, &mut Snarl<CanvasNode>),
) -> (Vec<PlannedGraphEdit>, Option<String>) {
    let source_blocks = project_source_blocks(&app.project);
    let source_paths = SourcePathCatalog::new(&app.project.source, &app.project.extra_sources)
        .with_primary_source_options(&app.project.source_options)
        .with_project_ownership(&app.project);
    let (root_scope, extra_targets, schema, snarl): (_, &[NamedTarget], _, _) = match document {
        MappingDocument::Main => (
            &mut app.project.root,
            &app.project.extra_targets[..],
            &app.project.target,
            &mut app.main_canvas.snarl,
        ),
        MappingDocument::Target(index) => {
            let target = &mut app.project.extra_targets[index];
            (
                &mut target.root,
                &[][..],
                &target.schema,
                &mut app
                    .mapping_workspace
                    .target_canvases
                    .get_mut(&index)
                    .expect("target canvas exists")
                    .snarl,
            )
        }
        MappingDocument::Function(_) => unreachable!("functions are not used here"),
    };
    let target_blocks = target_blocks(schema);
    let mut scroll = crate::canvas_endpoints::EndpointScrollState::default();
    let mut viewer = GraphViewer {
        graph: &mut app.project.graph,
        root_scope,
        primary_root_authoring: false,
        extra_targets,
        inactive_target_scopes: &[],
        project_references: crate::graph_viewer::ProjectGraphReferences::new(
            &app.project.failure_rules,
            &app.project.extra_sources,
        )
        .with_user_functions(&app.project.user_functions)
        .with_primary_source(&app.project.source),
        source_blocks: &source_blocks,
        target_blocks: &target_blocks,
        source_x12: false,
        target_x12: false,
        source_paths: &source_paths,
        function_names: Default::default(),
        function_inputs: Default::default(),
        parameter_names: Default::default(),
        protected_output: None,
        function_output: None,
        requested_function_open: None,
        colors: crate::appearance::SemanticThemeColors::default(),
        wire_color_mode: crate::appearance::WireColorMode::Theme,
        endpoint_scroll: &mut scroll,
        value_map_wheel: None,
        endpoint_search_match: None,
        node_sizes: None,
        hovered_node: None,
        hovered_node_this_frame: None,
        camera_pan: egui::Vec2::ZERO,
        camera_focus: None,
        canvas_transform: None,
        pin_interaction_ids: Vec::new(),
        planned_edits: Vec::new(),
        error: None,
    };
    edit(&mut viewer, snarl);
    (viewer.planned_edits, viewer.error)
}

/// Removes graph nodes through the same viewer entry point the canvas uses.
fn remove_nodes(
    app: &mut FerruleApp,
    document: MappingDocument,
    nodes: &[NodeId],
) -> Option<PlannedGraphEdit> {
    let (mut planned, error) = edit_canvas(app, document, |viewer, snarl| {
        let selected = nodes
            .iter()
            .map(|&node| canvas_id(snarl, CanvasNode::Graph(node)))
            .collect::<Vec<_>>();
        viewer.remove_snarl_nodes_recorded(&selected, snarl);
    });
    assert_eq!(error, None);
    assert!(planned.len() <= 1);
    planned.pop()
}

/// Queues one planned edit as its canvas host does and observes the frame,
/// returning whether the observer recorded it as a command.
pub(super) fn observe_planned(
    app: &mut FerruleApp,
    document: MappingDocument,
    planned: PlannedGraphEdit,
) -> bool {
    let undo = app.history.undo_len();
    app.queue_canvas_edits(document, vec![planned]);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.history.undo_len() == undo + 1
        && matches!(app.history.next_undo(), Some(HistoryRecord::Command(_)))
}

fn record_removal(app: &mut FerruleApp, document: MappingDocument, nodes: &[NodeId]) {
    let planned = remove_nodes(app, document, nodes).expect("removal forms a typed edit");
    assert!(observe_planned(app, document, planned));
}

pub(super) fn next_undo_commands(app: &FerruleApp) -> &[GraphCommand] {
    match app.history.next_undo() {
        Some(HistoryRecord::Command(record)) => record.commands(),
        _ => panic!("the next undo step is a command"),
    }
}

pub(super) fn next_undo_command(app: &FerruleApp) -> &GraphCommand {
    match next_undo_commands(app) {
        [command] => command,
        commands => panic!("one command, got {}", commands.len()),
    }
}

pub(super) fn next_redo_command(app: &FerruleApp) -> &GraphCommand {
    match app.history.next_redo() {
        Some(HistoryRecord::Command(record)) => match record.commands() {
            [command] => command,
            commands => panic!("one command, got {}", commands.len()),
        },
        _ => panic!("the next redo step is a command"),
    }
}

fn assert_removal_identities(removal: &GraphCommand, document: GraphDocument) {
    assert_eq!(removal.document(), &document);
    assert_eq!(
        removal.action(),
        &GraphAction::RemoveNodes {
            requested: BTreeSet::from([UPPER])
        }
    );
    let patch = removal.patch();
    assert_eq!(
        patch.removed_nodes().collect::<Vec<_>>(),
        [SOURCE, UPPER],
        "the requested node and its orphaned input"
    );
    assert_eq!(patch.added_nodes().collect::<Vec<_>>(), [REPLACEMENT]);
    assert_eq!(patch.changed_nodes().collect::<Vec<_>>(), [JOINED]);
    let scopes = patch.scopes().collect::<Vec<_>>();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].0, &ScopeLocation::root());
    assert_eq!(scopes[0].1.before().len(), 2);
    assert_eq!(
        scopes[0]
            .1
            .after()
            .iter()
            .map(|binding| (binding.target_field.as_str(), binding.node))
            .collect::<Vec<_>>(),
        [("Joined", JOINED)]
    );
}

#[test]
fn node_removal_is_one_command_with_complete_undo_and_redo() {
    let mut app = app();
    let custom = egui::pos2(321.0, 123.0);
    move_node(&mut app.main_canvas.snarl, CanvasNode::Graph(UPPER), custom);
    app.observe_editor_history(std::time::Instant::now(), false);
    app.mark_clean();
    let saved = key(&app);

    record_removal(&mut app, MappingDocument::Main, &[UPPER]);
    let removed = key(&app);
    assert_eq!(app.history.undo_len(), 2);
    assert_eq!(app.history.undo_label(), Some("Remove node"));
    assert_removal_identities(
        next_undo_command(&app),
        GraphDocument::Target(TargetId::Primary),
    );
    assert!(matches!(
        app.project.graph.nodes.get(&JOINED),
        Some(Node::Call { args, .. }) if args == &[REPLACEMENT, SEPARATOR]
    ));
    assert!(matches!(
        app.project.graph.nodes.get(&REPLACEMENT),
        Some(Node::Unconnected)
    ));
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(UPPER)),
        None
    );
    assert!(app.is_dirty() && app.observed_dirty());
    assert!(app.revisions.is_semantically_dirty());

    // The observer sees the recorded state and adds no snapshot of its own.
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.history.undo_len(), 2);

    app.undo_project();
    assert_eq!(app.status, "undid Remove node");
    assert_eq!(key(&app), saved);
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(UPPER)),
        Some(custom),
        "the retained layout restores the removed node's position"
    );
    assert!(!app.is_dirty() && !app.observed_dirty());
    assert_eq!(app.history.redo_label(), Some("Remove node"));
    assert_removal_identities(
        next_redo_command(&app),
        GraphDocument::Target(TargetId::Primary),
    );

    app.redo_project();
    assert_eq!(app.status, "redid Remove node");
    assert_eq!(key(&app), removed);
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(UPPER)),
        None
    );
    assert!(app.is_dirty() && app.observed_dirty());

    app.undo_project();
    assert_eq!(key(&app), saved);
    assert!(!app.observed_dirty());
}

#[test]
fn named_target_removals_record_the_target_identity() {
    let mut app = FerruleApp::default();
    app.project.source =
        SchemaNode::group("Source", vec![SchemaNode::scalar("A", ScalarType::String)]);
    app.project.target = target_schema("Target");
    let (graph, root) = graph_parts();
    app.project.graph = graph;
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

    record_removal(&mut app, MappingDocument::Target(0), &[UPPER]);
    assert_removal_identities(
        next_undo_command(&app),
        GraphDocument::Target(TargetId::Named("audit".into())),
    );
    assert!(app.project.root.bindings.is_empty());
    assert_eq!(app.project.extra_targets[0].root.bindings.len(), 1);

    app.undo_project();
    assert_eq!(key(&app), saved);
    assert_eq!(app.project.extra_targets[0].root.bindings.len(), 2);
    assert!(!app.observed_dirty());
}

#[test]
fn snapshot_and_command_steps_interleave_through_undo_and_redo() {
    let mut app = app();
    let now = std::time::Instant::now();
    let mut states = vec![key(&app)];

    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    app.observe_editor_history(now, false);
    states.push(key(&app));
    record_removal(&mut app, MappingDocument::Main, &[UPPER]);
    states.push(key(&app));
    app.project.graph.nodes.insert(SEPARATOR, separator("*"));
    app.observe_editor_history(now, false);
    states.push(key(&app));
    let moved = egui::pos2(77.0, 55.0);
    move_node(&mut app.main_canvas.snarl, CanvasNode::Graph(JOINED), moved);
    app.observe_editor_history(now, false);
    assert_eq!(key(&app), states[3], "a layout move keeps project content");

    let kinds = |app: &FerruleApp| match app.history.next_undo() {
        Some(HistoryRecord::Snapshot(_)) => "snapshot",
        Some(HistoryRecord::Command(_)) => "command",
        None => "none",
    };
    let mut undone = Vec::new();
    while app.history.can_undo() {
        undone.push((kinds(&app), app.history.undo_label().map(str::to_owned)));
        app.undo_project();
        undone.push(("state", Some(key(&app))));
    }
    assert_eq!(
        undone,
        [
            ("snapshot", Some("Edit layout".into())),
            ("state", Some(states[3].clone())),
            ("snapshot", Some("Edit mapping".into())),
            ("state", Some(states[2].clone())),
            ("command", Some("Remove node".into())),
            ("state", Some(states[1].clone())),
            ("snapshot", Some("Edit mapping".into())),
            ("state", Some(states[0].clone())),
        ]
    );
    assert!(!app.observed_dirty() && !app.is_dirty());

    for expected in &states[1..] {
        app.redo_project();
        assert_eq!(&key(&app), expected);
    }
    app.redo_project();
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(JOINED)),
        Some(moved)
    );
    assert!(!app.history.can_redo());

    // A new command after a partial undo discards the mixed redo branch.
    app.undo_project();
    app.undo_project();
    app.undo_project();
    assert_eq!(key(&app), states[1]);
    app.redo_project();
    assert_eq!(key(&app), states[2]);
    app.undo_project();
    assert!(matches!(
        app.history.next_redo(),
        Some(HistoryRecord::Command(_))
    ));
    record_removal(&mut app, MappingDocument::Main, &[JOINED]);
    assert!(!app.history.can_redo());
    assert_eq!(app.history.undo_len(), 2);
    app.undo_project();
    assert_eq!(key(&app), states[1]);
}

#[test]
fn removals_sharing_a_frame_with_unobserved_edits_stay_snapshots() {
    let mut app = app();
    let saved = key(&app);
    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    let planned = remove_nodes(&mut app, MappingDocument::Main, &[UPPER])
        .expect("removal forms a typed edit");

    assert!(!observe_planned(&mut app, MappingDocument::Main, planned));
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.undo_label(), Some("Edit mapping"));
    assert!(matches!(
        app.history.next_undo(),
        Some(HistoryRecord::Snapshot(_))
    ));
    app.undo_project();
    assert_eq!(key(&app), saved, "the snapshot restores both edits");
}

#[test]
fn a_stale_command_fails_undo_without_changing_state_or_history() {
    let mut app = app();
    record_removal(&mut app, MappingDocument::Main, &[UPPER]);
    app.observed_editor.project.graph.nodes.remove(&REPLACEMENT);
    let project = key(&app);

    app.undo_project();
    assert_eq!(app.status, "Undo failed");
    assert_eq!(key(&app), project);
    assert_eq!(app.history.undo_len(), 1);
    assert_eq!(app.history.redo_len(), 0);
    assert!(app.diagnostics.items().iter().any(|item| {
        item.message
            .contains(&format!("mapping node {REPLACEMENT} is not present"))
    }));
}

#[test]
fn command_layouts_survive_a_relocating_save() -> anyhow::Result<()> {
    let mut app = app();
    let custom = egui::pos2(222.0, 111.0);
    move_node(&mut app.main_canvas.snarl, CanvasNode::Graph(UPPER), custom);
    app.observe_editor_history(std::time::Instant::now(), false);
    record_removal(&mut app, MappingDocument::Main, &[UPPER]);

    let directory =
        std::env::temp_dir().join(format!("ferrule-gui-graph-commands-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("nested"))?;
    let path = directory.join("nested").join("project.json");
    let saved = app.save_document_to(&path);
    let undone = saved.map(|_| {
        assert!(!app.observed_dirty());
        assert_ne!(app.project.source_path.as_deref(), Some("source.xml"));
        app.undo_project();
        (
            position(&app.main_canvas.snarl, CanvasNode::Graph(UPPER)),
            app.project.source_path.clone(),
        )
    });
    std::fs::remove_dir_all(&directory)?;
    let (restored, source_path) = undone?;
    assert_eq!(restored, Some(custom));
    assert_ne!(
        source_path.as_deref(),
        Some("source.xml"),
        "the reverted command keeps the rebased paths"
    );
    assert!(app.is_dirty() && app.observed_dirty());
    Ok(())
}

#[test]
fn revisions_separate_semantic_and_layout_changes() {
    let mut app = app();
    let now = std::time::Instant::now();
    let saved = app.revisions.current();
    let original =
        position(&app.main_canvas.snarl, CanvasNode::Graph(JOINED)).expect("canvas node exists");
    let fingerprints = app.fingerprints.computations();

    // A layout-only change dirties the layout sidecar, not the project.
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::Graph(JOINED),
        original + egui::vec2(40.0, 0.0),
    );
    app.observe_editor_history(now, false);
    let moved = app.revisions.current();
    assert_eq!(moved.semantic(), saved.semantic());
    assert_ne!(moved.layout(), saved.layout());
    assert!(app.observed_dirty() && !app.revisions.is_semantically_dirty());
    assert_eq!(app.fingerprints.computations(), fingerprints);

    // Returning the layout to its saved content adopts the saved revision.
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::Graph(JOINED),
        original,
    );
    app.observe_editor_history(now, false);
    assert_eq!(app.revisions.current(), saved);
    assert!(!app.observed_dirty() && !app.is_dirty());

    // A semantic edit bumps only the semantic revision and dirties the project.
    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    app.observe_editor_history(now, false);
    let edited = app.revisions.current();
    assert_ne!(edited.semantic(), saved.semantic());
    assert_eq!(edited.layout(), saved.layout());
    assert!(app.revisions.is_semantically_dirty() && app.observed_dirty());
    assert_eq!(app.fingerprints.computations(), fingerprints + 1);

    // Undo back to the saved content is clean at the saved revision.
    app.undo_project();
    assert_eq!(app.revisions.current(), saved);
    assert!(!app.observed_dirty() && !app.is_dirty());

    // Editing back to the saved content by hand is clean too.
    app.project.graph.nodes.insert(SEPARATOR, separator("*"));
    app.observe_editor_history(now, false);
    assert!(app.observed_dirty());
    app.project.graph.nodes.insert(SEPARATOR, separator("-"));
    app.observe_editor_history(now, false);
    assert_eq!(app.revisions.current().semantic(), saved.semantic());
    assert!(!app.observed_dirty() && !app.is_dirty());

    // Commands follow the same save point.
    record_removal(&mut app, MappingDocument::Main, &[UPPER]);
    assert!(app.revisions.is_semantically_dirty());
    app.undo_project();
    assert_eq!(app.revisions.current().semantic(), saved.semantic());
    assert!(!app.observed_dirty());
}

#[test]
fn saving_unobserved_edits_reserves_the_revision_they_will_adopt() {
    let mut app = app();
    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    app.mark_clean();
    assert!(
        app.observed_dirty(),
        "the observation still predates the edit"
    );
    assert!(!app.is_dirty());

    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(!app.observed_dirty());
    assert_eq!(
        app.fingerprints
            .get(app.revisions.current().semantic())
            .map(String::as_str),
        app.observed_editor
            .state
            .layout
            .project_fingerprint
            .as_deref()
    );
}

#[test]
fn idle_observation_keys_the_project_once_and_never_refingerprints() {
    let mut app = app();
    let context = egui::Context::default();
    let now = std::time::Instant::now();
    let keys = project_keys_computed();
    let fingerprints = app.fingerprints.computations();

    for frame in 1..=5 {
        let _ = context.run_test_ui(egui::RawInput::default(), |ui| app.show_status_bar(ui));
        app.observe_editor_history(now, false);
        assert!(!app.observed_dirty());
        assert_eq!(project_keys_computed() - keys, frame);
    }
    assert_eq!(app.fingerprints.computations(), fingerprints);
    assert_eq!(app.history.undo_len(), 0);

    app.project.graph.nodes.insert(SEPARATOR, separator("+"));
    app.observe_editor_history(now, false);
    assert_eq!(app.fingerprints.computations(), fingerprints + 1);
    for _ in 0..5 {
        app.observe_editor_history(now, false);
    }
    assert_eq!(app.fingerprints.computations(), fingerprints + 1);
    assert_eq!(project_keys_computed() - keys, 11);
}

#[path = "graph_wire_commands_tests.rs"]
mod wires;

#[path = "owned_source_authoring_tests.rs"]
mod owned_sources;

#[path = "graph_commands_canvas_tests.rs"]
mod canvas;

#[path = "graph_property_commands_tests.rs"]
mod properties;
