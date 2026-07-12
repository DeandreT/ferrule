use super::*;
use ir::ScalarType;
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
    move_canvas_node(&mut app.snarl, CanvasNode::Source, egui::pos2(73.0, 91.0));
    move_canvas_node(
        &mut app.snarl,
        CanvasNode::Graph(7),
        egui::pos2(517.0, 233.0),
    );
    app.project_path = project_path.display().to_string();
    app.save_project().expect("project and layout save");

    let project_json = std::fs::read_to_string(&project_path).expect("project was written");
    serde_json::from_str::<Project>(&project_json).expect("project JSON remains unchanged");
    assert!(!project_json.contains("\"layout\""));
    assert!(layout_path(&app.project_path).is_file());

    let mut loaded = FerruleApp {
        project_path: app.project_path.clone(),
        ..Default::default()
    };
    loaded.load_project();
    assert_eq!(
        canvas_position(&loaded.snarl, CanvasNode::Source),
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
    snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Source);
    let placeholder = snarl.insert_node(egui::pos2(180.0, 210.0), CanvasNode::Placeholder(0));
    let call = snarl.insert_node(egui::pos2(480.0, 210.0), CanvasNode::Graph(1));
    snarl.insert_node(egui::pos2(780.0, 0.0), CanvasNode::Target);
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
    app.project_path = project_path.display().to_string();
    app.save_project().expect("project and layout save");

    let mut loaded = FerruleApp {
        project_path: app.project_path.clone(),
        ..Default::default()
    };
    loaded.load_project();
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
fn stale_layout_cannot_reclassify_a_project_node_as_a_placeholder() {
    let project_path = temporary_project_path("stale-placeholder-layout");
    let mut app = FerruleApp::default();
    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::Null,
        },
    );
    app.snarl = build_snarl(&app.project);
    for node in app.snarl.nodes_mut() {
        if *node == CanvasNode::Graph(0) {
            *node = CanvasNode::Placeholder(0);
        }
    }
    app.project_path = project_path.display().to_string();
    app.save_project().expect("project and layout save");

    app.project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("intentional null replacement".into()),
        },
    );
    std::fs::write(
        &project_path,
        serde_json::to_string_pretty(&app.project).expect("project serializes"),
    )
    .expect("replacement project is written without touching its layout");

    let mut loaded = FerruleApp {
        project_path: app.project_path.clone(),
        ..Default::default()
    };
    loaded.load_project();
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
        project_path: project_path.display().to_string(),
        ..Default::default()
    };
    app.load_project();
    assert_eq!(
        canvas_position(&app.snarl, CanvasNode::Source),
        egui::pos2(0.0, 0.0)
    );
    assert_eq!(app.status, format!("loaded {}", app.project_path));
    assert!(!app.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn canvas_moves_and_arrange_roundtrip_through_history() {
    let mut app = FerruleApp::default();
    let arranged = canvas_position(&app.snarl, CanvasNode::Source);
    let custom = egui::pos2(123.0, 456.0);
    move_canvas_node(&mut app.snarl, CanvasNode::Source, custom);
    app.mark_clean();
    app.rebase_history();

    app.snarl = arrange_snarl(&app.project, &app.snarl);
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(canvas_position(&app.snarl, CanvasNode::Source), arranged);
    assert!(app.is_dirty());

    app.undo_project();
    assert_eq!(canvas_position(&app.snarl, CanvasNode::Source), custom);
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(canvas_position(&app.snarl, CanvasNode::Source), arranged);
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

    let arranged = arrange_snarl(&project, &current);
    assert!(
        arranged
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(0))
    );
    assert_eq!(
        arranged
            .wires()
            .map(|(from, to)| (arranged[from.node], arranged[to.node]))
            .collect::<Vec<_>>(),
        vec![(CanvasNode::Placeholder(0), CanvasNode::Graph(1))]
    );
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

    app.saved_editor = None;
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

    assert!(app.undo_history.is_empty());
    assert!(app.pending_history.is_some());
    app.observe_editor_history(
        start + HISTORY_COALESCE_DELAY + std::time::Duration::from_millis(100),
        true,
    );
    assert_eq!(app.undo_history.len(), 1);

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
    assert_eq!(app.undo_history.len(), 2);

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
    assert_eq!(app.undo_history.len(), 1);

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
    assert!(app.redo_history.is_empty());
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
        graph,
        root: Scope {
            target_field: String::new(),
            source: Some(vec![]),
            sequence: None,
            filter: None,
            group_by: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
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
            children: vec![],
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
    assert!(kinds.contains(&CanvasNode::Source));
    assert!(kinds.contains(&CanvasNode::Target));
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
        (CanvasNode::Source, 0, CanvasNode::Graph(1), 0),
        (CanvasNode::Graph(1), 0, CanvasNode::Target, 0),
        (CanvasNode::Source, 1, CanvasNode::Target, 1),
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
    assert_eq!(snarl.nodes().count(), 2, "both source fields stay hidden");
    let mut wires: Vec<_> = snarl
        .wires()
        .map(|(output, input)| (output.output, input.input))
        .collect();
    wires.sort_unstable();
    assert_eq!(wires, vec![(0, 0), (1, 1)]);
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
