use super::*;
use crate::layout_store::layout_path;
use ir::{ScalarType, SchemaNode};
use mapping::Binding;

fn canvas_position(snarl: &Snarl<CanvasNode>, wanted: CanvasNode) -> egui::Pos2 {
    snarl
        .nodes_pos()
        .find_map(|(pos, &node)| (node == wanted).then_some(pos))
        .expect("canvas node exists")
}

fn move_canvas_node(snarl: &mut Snarl<CanvasNode>, wanted: CanvasNode, pos: egui::Pos2) {
    let id = snarl
        .node_ids()
        .find_map(|(id, &node)| (node == wanted).then_some(id))
        .expect("canvas node exists");
    snarl.get_node_info_mut(id).expect("canvas node exists").pos = pos;
}

fn temporary_project_path(test_name: &str) -> PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ferrule-gui-{test_name}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temporary test directory is created");
    dir.join("project.json")
}

#[test]
fn legacy_endpoint_layout_entries_migrate_to_the_first_block() {
    let source: PersistedCanvasNode =
        serde_json::from_str(r#"{"kind":"source"}"#).expect("legacy source entry parses");
    let target: PersistedCanvasNode =
        serde_json::from_str(r#"{"kind":"target"}"#).expect("legacy target entry parses");

    assert_eq!(source, PersistedCanvasNode::Source { block: 0 });
    assert_eq!(target, PersistedCanvasNode::Target { block: 0 });
    assert_eq!(
        PersistedCanvasNode::from(CanvasNode::SourceBlock(3)),
        PersistedCanvasNode::Source { block: 3 }
    );
}

#[test]
fn canvas_layout_saves_alongside_backward_compatible_project_json() {
    let project_path = temporary_project_path("layout-roundtrip");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.snarl = build_snarl(&app.project);
    move_canvas_node(
        &mut app.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(73.0, 91.0),
    );
    move_canvas_node(
        &mut app.snarl,
        CanvasNode::Graph(7),
        egui::pos2(517.0, 233.0),
    );
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    let project_json = std::fs::read_to_string(&project_path).expect("project was written");
    serde_json::from_str::<Project>(&project_json).expect("project JSON remains unchanged");
    assert!(!project_json.contains("\"layout\""));
    assert!(layout_path(&project_path).is_file());

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&loaded.snarl, CanvasNode::SourceBlock(0)),
        egui::pos2(73.0, 91.0)
    );
    assert_eq!(
        canvas_position(&loaded.snarl, CanvasNode::Graph(7)),
        egui::pos2(517.0, 233.0)
    );
    assert!(!loaded.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn layout_sidecar_restores_placeholder_identity_and_wiring() {
    let project_path = temporary_project_path("placeholder-roundtrip");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let mut snarl = Snarl::new();
    snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
    let placeholder = snarl.insert_node(egui::pos2(180.0, 210.0), CanvasNode::Placeholder(0));
    let call = snarl.insert_node(egui::pos2(480.0, 210.0), CanvasNode::Graph(1));
    snarl.insert_node(egui::pos2(780.0, 0.0), CanvasNode::TargetBlock(0));
    snarl.connect(
        OutPinId {
            node: placeholder,
            output: 0,
        },
        InPinId {
            node: call,
            input: 0,
        },
    );
    app.snarl = snarl;
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&loaded.snarl, CanvasNode::Placeholder(0)),
        egui::pos2(180.0, 210.0)
    );
    let wires: Vec<_> = loaded
        .snarl
        .wires()
        .map(|(from, to)| (loaded.snarl[from.node], loaded.snarl[to.node]))
        .collect();
    assert_eq!(
        wires,
        vec![(CanvasNode::Placeholder(0), CanvasNode::Graph(1))]
    );

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn stale_layout_cannot_reclassify_or_reposition_nodes() {
    let project_path = temporary_project_path("stale-placeholder-layout");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Int(1),
        },
    );
    app.snarl = build_snarl(&app.project);
    for node in app.snarl.nodes_mut() {
        if *node == CanvasNode::Graph(0) {
            *node = CanvasNode::Placeholder(0);
        }
    }
    move_canvas_node(
        &mut app.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(901.0, 733.0),
    );
    move_canvas_node(
        &mut app.snarl,
        CanvasNode::Graph(1),
        egui::pos2(1201.0, 833.0),
    );
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("intentional null replacement".into()),
        },
    );
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Int(2),
        },
    );
    let default_layout = build_snarl(&app.project);
    let expected_source = canvas_position(&default_layout, CanvasNode::SourceBlock(0));
    let expected_graph = canvas_position(&default_layout, CanvasNode::Graph(1));
    std::fs::write(
        &project_path,
        serde_json::to_string_pretty(&app.project).expect("project serializes"),
    )
    .expect("replacement project is written without touching its layout");

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert!(
        loaded
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Graph(0))
    );
    assert!(
        !loaded
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(0))
    );
    assert_eq!(
        canvas_position(&loaded.snarl, CanvasNode::SourceBlock(0)),
        expected_source
    );
    assert_eq!(
        canvas_position(&loaded.snarl, CanvasNode::Graph(1)),
        expected_graph
    );

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn project_without_layout_sidecar_uses_default_layout() {
    let project_path = temporary_project_path("legacy-project");
    let project = blank_project();
    std::fs::write(
        &project_path,
        serde_json::to_string_pretty(&project).expect("project serializes"),
    )
    .expect("legacy project is written");

    let mut app = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    app.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&app.snarl, CanvasNode::SourceBlock(0)),
        egui::pos2(0.0, 0.0)
    );
    assert_eq!(app.status, format!("loaded {}", project_path.display()));
    assert!(!app.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn canvas_moves_and_arrange_roundtrip_through_history() {
    let mut app = FerruleApp::default();
    let arranged = canvas_position(&app.snarl, CanvasNode::SourceBlock(0));
    let custom = egui::pos2(123.0, 456.0);
    move_canvas_node(&mut app.snarl, CanvasNode::SourceBlock(0), custom);
    app.mark_clean();
    app.rebase_history();

    arrange_snarl(
        &mut app.snarl,
        &app.canvas_node_sizes,
        crate::appearance::WireAppearance::default(),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(
        canvas_position(&app.snarl, CanvasNode::SourceBlock(0)),
        arranged
    );
    assert!(app.is_dirty());

    app.undo_project();
    assert_eq!(
        canvas_position(&app.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(
        canvas_position(&app.snarl, CanvasNode::SourceBlock(0)),
        arranged
    );
    assert!(app.is_dirty());
}

#[test]
fn arrange_preserves_placeholder_identity_and_wiring() {
    let mut project = blank_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let mut current = build_snarl(&project);
    for node in current.nodes_mut() {
        if *node == CanvasNode::Graph(0) {
            *node = CanvasNode::Placeholder(0);
        }
    }

    let identities_before: Vec<_> = current.node_ids().map(|(id, node)| (id, *node)).collect();
    let wires_before: Vec<_> = current.wires().collect();
    arrange_snarl(
        &mut current,
        &std::collections::BTreeMap::new(),
        crate::appearance::WireAppearance::default(),
    );
    assert!(
        current
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(0))
    );
    assert_eq!(
        current
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<Vec<_>>(),
        identities_before
    );
    assert_eq!(current.wires().collect::<Vec<_>>(), wires_before);
}

#[test]
fn project_dirty_state_tracks_saved_content() {
    let mut app = FerruleApp::default();
    assert!(!app.is_dirty());

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("changed".into()),
        },
    );
    assert!(app.is_dirty());

    app.project.graph.nodes.clear();
    assert!(
        !app.is_dirty(),
        "restoring saved content clears dirty state"
    );
}

#[test]
fn destructive_actions_wait_for_confirmation_when_dirty() {
    let mut app = FerruleApp::default();
    assert_eq!(
        app.request_destructive_action(DestructiveAction::NewProject),
        Some(DestructiveAction::NewProject)
    );

    app.history.mark_unsaved();
    assert_eq!(
        app.request_destructive_action(DestructiveAction::OpenProject),
        None
    );
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::OpenProject)
    );
}

#[test]
fn failed_open_preserves_the_current_document_and_dirty_state() {
    let old_path = temporary_project_path("failed-open-current");
    let invalid_path = old_path.with_file_name("invalid.json");
    std::fs::write(&invalid_path, "not json").expect("invalid project is written");
    let mut app = FerruleApp {
        document: DocumentLocation::saved(old_path.clone()),
        ..Default::default()
    };
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("unsaved".into()),
        },
    );
    assert!(app.is_dirty());

    app.load_project_from(&invalid_path);

    assert_eq!(app.document, DocumentLocation::saved(old_path.clone()));
    assert!(app.project.graph.nodes.contains_key(&7));
    assert!(app.is_dirty());
    assert_eq!(app.diagnostics.items().len(), 1);
    std::fs::remove_dir_all(old_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn failed_save_does_not_change_the_document_association() {
    let old_path = temporary_project_path("failed-save-current");
    let directory = old_path.parent().expect("project has parent").to_path_buf();
    let mut app = FerruleApp {
        document: DocumentLocation::saved(old_path.clone()),
        ..Default::default()
    };
    app.project.graph.nodes.insert(
        8,
        Node::Const {
            value: ir::Value::Null,
        },
    );

    assert!(app.save_document_to(&directory).is_err());
    assert_eq!(app.document, DocumentLocation::saved(old_path.clone()));
    assert!(app.is_dirty());
    std::fs::remove_dir_all(&directory).expect("temporary test directory is removed");
}

#[test]
fn invalid_run_does_not_save_or_clear_dirty_state() {
    let project_path = temporary_project_path("invalid-run");
    let mut app = FerruleApp::default();
    app.save_document_to(&project_path)
        .expect("baseline project is saved");
    let saved = std::fs::read_to_string(&project_path).expect("baseline project is readable");
    app.project.root.bindings.push(Binding {
        target_field: "missing".into(),
        node: 999,
    });

    app.run(&egui::Context::default());

    assert_eq!(
        std::fs::read_to_string(&project_path).expect("project remains readable"),
        saved
    );
    assert!(app.is_dirty());
    assert!(!app.diagnostics.items().is_empty());
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn blank_run_paths_fall_back_to_stored_project_paths() {
    let project_path = temporary_project_path("stored-run-paths");
    let directory = project_path.parent().expect("project has parent");
    std::fs::write(directory.join("input.xml"), "<root/>").expect("input instance is written");
    let mut app = FerruleApp::default();
    app.project.source_path = Some("input.xml".into());
    app.project.target_path = Some("output.xml".into());
    app.save_document_to(&project_path)
        .expect("project with stored paths is saved");
    app.input_path.clear();
    app.output_path.clear();

    app.run(&egui::Context::default());

    assert!(directory.join("output.xml").is_file(), "{}", app.status);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert!(app.show_run_report);
    let report = app
        .run_report
        .as_ref()
        .expect("successful run has a report");
    assert_eq!(report.selected_output(), 0);
    assert_eq!(report.report.outputs.len(), 1);
    assert_eq!(report.report.outputs[0].path, directory.join("output.xml"));
    assert!(!app.is_dirty());
    std::fs::remove_dir_all(directory).expect("temporary test directory is removed");
}

#[test]
fn successful_save_resumes_a_destructive_continuation() {
    let project_path = temporary_project_path("save-continuation");
    let mut app = FerruleApp::default();
    app.save_document_to(&project_path)
        .expect("baseline project is saved");
    app.project.graph.nodes.insert(
        9,
        Node::Const {
            value: ir::Value::Null,
        },
    );

    app.save_with_continuation(
        Some(SaveContinuation::Destructive(DestructiveAction::NewProject)),
        &egui::Context::default(),
    );

    assert!(app.project.graph.nodes.is_empty());
    assert!(app.document.saved_path().is_none());
    let saved: Project = serde_json::from_str(
        &std::fs::read_to_string(&project_path).expect("saved project is readable"),
    )
    .expect("saved project parses");
    assert!(saved.graph.nodes.contains_key(&9));
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn history_coalesces_keyboard_edits_and_roundtrips_undo_redo() {
    let mut app = FerruleApp::default();
    let start = std::time::Instant::now();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("a".into()),
        },
    );
    app.observe_editor_history(start, true);
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("ab".into()),
        },
    );
    app.observe_editor_history(start + std::time::Duration::from_millis(100), true);

    assert_eq!(app.history.undo_len(), 0);
    assert!(app.pending_history.is_some());
    app.observe_editor_history(
        start + HISTORY_COALESCE_DELAY + std::time::Duration::from_millis(100),
        true,
    );
    assert_eq!(app.history.undo_len(), 1);

    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
    app.redo_project();
    assert!(matches!(
        app.project.graph.nodes.get(&0),
        Some(Node::Const {
            value: ir::Value::String(value)
        }) if value == "ab"
    ));
}

#[test]
fn pointer_edits_are_distinct_history_steps() {
    let mut app = FerruleApp::default();
    let start = std::time::Instant::now();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.observe_editor_history(start, false);
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.observe_editor_history(start, false);
    assert_eq!(app.history.undo_len(), 2);

    app.undo_project();
    assert!(app.project.graph.nodes.contains_key(&0));
    assert!(!app.project.graph.nodes.contains_key(&1));
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
}

#[test]
fn keyboard_edits_after_the_quiet_period_start_a_new_history_step() {
    let mut app = FerruleApp::default();
    let start = std::time::Instant::now();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("first".into()),
        },
    );
    app.observe_editor_history(start, true);

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("second".into()),
        },
    );
    app.observe_editor_history(start + HISTORY_COALESCE_DELAY, true);
    assert_eq!(app.history.undo_len(), 1);

    app.undo_project();
    assert!(matches!(
        app.project.graph.nodes.get(&0),
        Some(Node::Const {
            value: ir::Value::String(value)
        }) if value == "first"
    ));
    app.undo_project();
    assert!(app.project.graph.nodes.is_empty());
}

#[test]
fn undo_and_redo_update_dirty_state_against_saved_baseline() {
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(app.is_dirty());

    app.undo_project();
    assert!(!app.is_dirty());
    app.redo_project();
    assert!(app.is_dirty());

    app.rebase_history();
    assert!(!app.can_undo());
    assert!(!app.history.can_redo());
}

/// Loading the orders-style project must recreate the whole picture:
/// hidden SourceFields become wires from the Source endpoint, function
/// inputs become node-to-node wires, and bindings become wires into
/// the Target endpoint's leaf pins.
#[test]
fn build_snarl_recreates_endpoint_and_binding_wires() {
    let mut graph = Graph::default();
    // 0: hidden SourceField (matches leaf "name"), 1: upper(0)
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["name".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let project = Project {
        source: SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        ),
        target: SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("loud_name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(vec![]),
            bindings: vec![
                Binding {
                    target_field: "loud_name".into(),
                    node: 1,
                },
                // Bound straight from the hidden SourceField? Use a
                // second field to prove Source->Target wires too.
                Binding {
                    target_field: "age".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        },
    };
    // 2: hidden SourceField for "age", bound directly to the target.
    let mut project = project;
    project.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["age".into()],
            frame: None,
        },
    );

    let snarl = build_snarl(&project);

    // Only Source, Target, and the Call node should be on the canvas.
    let kinds: Vec<CanvasNode> = snarl.nodes().copied().collect();
    assert_eq!(kinds.len(), 3);
    assert!(kinds.contains(&CanvasNode::SourceBlock(0)));
    assert!(kinds.contains(&CanvasNode::TargetBlock(0)));
    assert!(kinds.contains(&CanvasNode::Graph(1)));

    // Wires: Source(name)->Call arg0, Call->Target(loud_name),
    // Source(age)->Target(age).
    let mut wires: Vec<(CanvasNode, usize, CanvasNode, usize)> = snarl
        .wires()
        .map(|(o, i)| (snarl[o.node], o.output, snarl[i.node], i.input))
        .collect();
    // Wire iteration order is not deterministic; compare as a set.
    wires.sort_by_key(|w| format!("{w:?}"));
    let mut expected = vec![
        (CanvasNode::SourceBlock(0), 0, CanvasNode::Graph(1), 0),
        (CanvasNode::Graph(1), 0, CanvasNode::TargetBlock(0), 0),
        (CanvasNode::SourceBlock(0), 1, CanvasNode::TargetBlock(0), 1),
    ];
    expected.sort_by_key(|w| format!("{w:?}"));
    assert_eq!(wires, expected);
}

#[test]
fn build_snarl_matches_hidden_source_fields_by_frame_and_path() {
    let source = SchemaNode::group(
        "root",
        vec![
            SchemaNode::group("A", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
        ],
    );
    let target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("AId", ScalarType::String),
            SchemaNode::scalar("BId", ScalarType::String),
        ],
    );
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            frame: Some(vec!["A".into()]),
            path: vec!["Id".into()],
        },
    );
    graph.nodes.insert(
        1,
        Node::SourceField {
            frame: Some(vec!["B".into()]),
            path: vec!["Id".into()],
        },
    );
    let project = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "AId".into(),
                    node: 0,
                },
                Binding {
                    target_field: "BId".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    };

    let snarl = build_snarl(&project);
    assert_eq!(snarl.nodes().count(), 3, "both source fields stay hidden");
    let mut wires: Vec<_> = snarl
        .wires()
        .map(|(output, input)| (snarl[output.node], output.output, input.input))
        .collect();
    wires.sort_by_key(|wire| format!("{wire:?}"));
    assert_eq!(
        wires,
        vec![
            (CanvasNode::SourceBlock(0), 0, 0),
            (CanvasNode::SourceBlock(1), 0, 1),
        ]
    );
}

#[test]
fn build_snarl_only_hides_legacy_frameless_fields_with_unique_suffixes() {
    let project = |source| {
        let target = SchemaNode::group("root", vec![SchemaNode::scalar("out", ScalarType::String)]);
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["Id".into()],
            },
        );
        Project {
            source,
            target,
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            graph,
            root: Scope {
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        }
    };
    let group = |name| {
        SchemaNode::group(name, vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating()
    };

    let unique = build_snarl(&project(SchemaNode::group("root", vec![group("A")])));
    assert_eq!(unique.nodes().count(), 2);

    let ambiguous = build_snarl(&project(SchemaNode::group(
        "root",
        vec![group("A"), group("B")],
    )));
    assert!(ambiguous.nodes().any(|node| *node == CanvasNode::Graph(0)));
}

#[test]
fn new_mapping_stages_both_schemas_before_replacing_the_project() {
    let project_path = temporary_project_path("new-mapping");
    let directory = project_path.parent().expect("project has parent");
    let source_path = directory.join("source.xsd");
    let target_path = directory.join("target.schema.json");
    std::fs::write(
        &source_path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="SourceRoot">
    <xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#,
    )
    .expect("source schema is written");
    std::fs::write(
        &target_path,
        r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "TargetRoot",
  "type": "object",
  "properties": { "Label": { "type": "string" } }
}"#,
    )
    .expect("target schema is written");

    let mut app = FerruleApp::default();
    app.begin_new_mapping();
    app.stage_mapping_schema(SchemaSide::Source, source_path);
    assert_eq!(app.project.source.name, "root");
    app.stage_mapping_schema(SchemaSide::Target, target_path);
    assert_eq!(app.project.target.name, "root");

    app.finish_new_mapping();

    assert_eq!(app.project.source.name, "SourceRoot");
    assert_eq!(app.project.target.name, "TargetRoot");
    assert!(app.new_mapping_setup.is_none());
    assert!(app.is_dirty());
    assert_eq!(app.snarl.nodes().count(), 2);

    std::fs::remove_dir_all(directory).expect("temporary test directory is removed");
}
