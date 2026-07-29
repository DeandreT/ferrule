use super::*;
use crate::canvas_layout::arrange_snarl;
use crate::layout_store::layout_path;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, FunctionId, NamedTarget, Scope, UserFunction};

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

fn named_target(name: &str) -> NamedTarget {
    NamedTarget {
        name: name.to_owned(),
        path: Some(format!("{name}.json")),
        schema: SchemaNode::group(name, vec![SchemaNode::scalar("value", ScalarType::String)]),
        options: FormatOptions::default(),
        root: Scope::default(),
    }
}

fn user_function(name: &str) -> UserFunction {
    let mut body = Graph::default();
    body.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("result".to_owned()),
        },
    );
    UserFunction {
        library: "local".to_owned(),
        name: name.to_owned(),
        description: None,
        parameters: Vec::new(),
        output_name: "result".to_owned(),
        output_type: ScalarType::String,
        body,
        output: 0,
    }
}

#[test]
fn extra_target_create_and_rename_roundtrip_through_history() {
    let mut app = FerruleApp {
        extra_target_draft: Some(ExtraTargetDraft {
            name: "audit".to_owned(),
            output_path: "audit.json".to_owned(),
            schema: Some(named_target("audit").schema),
            ..ExtraTargetDraft::default()
        }),
        ..FerruleApp::default()
    };
    app.finish_extra_target();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));
    assert!(app.is_dirty());

    app.undo_project();
    assert!(app.project.extra_targets.is_empty());
    assert_eq!(app.mapping_workspace.active, MappingDocument::Main);
    assert!(!app.is_dirty());

    app.redo_project();
    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));

    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(203.0, 407.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("audit target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    app.observe_editor_history(std::time::Instant::now(), false);

    app.edit_extra_target(0);
    app.extra_target_draft
        .as_mut()
        .expect("edit draft exists")
        .name = "archive".to_owned();
    app.finish_extra_target();
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.project.extra_targets[0].name, "archive");
    assert_eq!(app.mapping_workspace.tabs[1], MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.undo_project();
    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert_eq!(app.mapping_workspace.tabs[1], MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].name, "archive");
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
}

#[test]
fn duplicate_extra_target_name_does_not_mutate_project() {
    let mut app = FerruleApp::default();
    app.project.extra_targets.push(named_target("audit"));
    app.extra_target_draft = Some(ExtraTargetDraft {
        name: " audit ".to_owned(),
        schema: Some(named_target("duplicate").schema),
        ..ExtraTargetDraft::default()
    });

    app.finish_extra_target();

    assert_eq!(app.project.extra_targets.len(), 1);
    assert_eq!(app.project.extra_targets[0].name, "audit");
    assert!(app.extra_target_draft.is_some());
    assert_eq!(app.status, "target is incomplete");
}

#[test]
fn primary_auto_connect_is_one_undoable_position_preserving_mutation() {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "source",
        vec![
            SchemaNode::scalar("order_id", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Int),
        ],
    );
    app.project.target = SchemaNode::group(
        "target",
        vec![
            SchemaNode::scalar("OrderId", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Float),
        ],
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    let custom = egui::pos2(117.0, 259.0);
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.begin_auto_connect();
    let pending = app
        .pending_auto_connect
        .as_ref()
        .expect("confirmation is staged");
    assert_eq!(pending.plan.connections.len(), 2);
    assert_eq!(pending.plan.skipped_ambiguous, 0);
    assert_eq!(pending.plan.skipped_incompatible, 0);
    app.apply_pending_auto_connect();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert_eq!(app.project.root.bindings.len(), 2);
    assert_eq!(app.project.graph.nodes.len(), 2);
    assert!(
        app.project
            .graph
            .nodes
            .values()
            .all(|node| matches!(node, Node::SourceField { .. }))
    );
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    assert_eq!(app.history.undo_len(), 1);

    app.undo_project();
    assert!(app.project.root.bindings.is_empty());
    assert!(app.project.graph.nodes.is_empty());
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    app.redo_project();
    assert_eq!(app.project.root.bindings.len(), 2);
    assert_eq!(app.project.graph.nodes.len(), 2);
}

#[test]
fn ambiguous_auto_connect_plan_leaves_the_project_unchanged() {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "source",
        vec![
            SchemaNode::group(
                "left",
                vec![SchemaNode::scalar("customer_id", ScalarType::String)],
            ),
            SchemaNode::group(
                "right",
                vec![SchemaNode::scalar("Customer-Id", ScalarType::String)],
            ),
        ],
    );
    app.project.target = SchemaNode::group(
        "target",
        vec![SchemaNode::scalar("CustomerId", ScalarType::String)],
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    app.mark_clean();
    app.rebase_history();

    app.begin_auto_connect();
    let pending = app
        .pending_auto_connect
        .as_ref()
        .expect("confirmation is staged");
    assert!(pending.plan.connections.is_empty());
    assert_eq!(pending.plan.skipped_ambiguous, 1);
    app.apply_pending_auto_connect();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert!(app.project.graph.nodes.is_empty());
    assert!(app.project.root.bindings.is_empty());
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
}

#[test]
fn named_target_auto_connect_uses_its_scope_and_preserves_its_canvas() {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "source",
        vec![SchemaNode::scalar("status", ScalarType::String)],
    );
    app.project.extra_targets.push(NamedTarget {
        name: "audit".to_owned(),
        path: Some("audit.json".to_owned()),
        schema: SchemaNode::group(
            "audit",
            vec![SchemaNode::scalar("Status", ScalarType::String)],
        ),
        options: FormatOptions::default(),
        root: Scope::default(),
    });
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(151.0, 313.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("named target canvas exists")
            .snarl,
        CanvasNode::SourceBlock(0),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.begin_auto_connect();
    app.apply_pending_auto_connect();
    app.observe_editor_history(std::time::Instant::now(), false);

    assert!(app.project.root.bindings.is_empty());
    assert_eq!(app.project.extra_targets[0].root.bindings.len(), 1);
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::SourceBlock(0)
        ),
        custom
    );

    app.undo_project();
    assert!(app.project.extra_targets[0].root.bindings.is_empty());
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::SourceBlock(0)
        ),
        custom
    );
    app.redo_project();
    assert_eq!(app.project.extra_targets[0].root.bindings.len(), 1);
}

#[test]
fn target_removal_rekeys_tabs_and_canvases_without_removing_graph_nodes() {
    let mut app = FerruleApp::default();
    app.project.extra_targets = vec![
        named_target("first"),
        named_target("second"),
        named_target("third"),
    ];
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);
    app.open_target_tab(2);
    assert!(app.ensure_target_canvas(2));
    let custom = egui::pos2(321.0, 654.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&2)
            .expect("third target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    app.remove_extra_target_now(0);
    app.observe_editor_history(std::time::Instant::now(), false);

    assert_eq!(
        app.project
            .extra_targets
            .iter()
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "third"]
    );
    assert!(app.project.graph.nodes.contains_key(&7));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(1));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&1].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.undo_project();
    assert_eq!(app.project.extra_targets.len(), 3);
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(2));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&2].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );

    app.redo_project();
    assert_eq!(app.project.extra_targets.len(), 2);
    assert!(app.project.graph.nodes.contains_key(&7));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(1));
    assert_eq!(
        canvas_position(
            &app.mapping_workspace.target_canvases[&1].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
}

#[test]
fn save_and_reopen_preserve_named_targets_and_mixed_tab_order() {
    let project_path = temporary_project_path("extra-target-roundtrip");
    let mut app = FerruleApp {
        document: DocumentLocation::untitled(project_path.clone()),
        ..Default::default()
    };
    let mut audit = named_target("audit");
    audit.path = Some("outputs/audit.jsonl".to_owned());
    audit.options.json_lines = true;
    audit.root.target_field = "auditRoot".to_owned();
    app.project.extra_targets = vec![audit, named_target("archive")];
    let function = FunctionId::new(42);
    app.project
        .user_functions
        .insert(function, user_function("normalize"));
    app.project.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String("shared".to_owned()),
        },
    );
    app.main_canvas.snarl = build_snarl(&app.project);

    app.open_target_tab(1);
    app.open_function_tab(function);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    let custom = egui::pos2(419.0, 287.0);
    move_canvas_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .expect("audit target canvas exists")
            .snarl,
        CanvasNode::Graph(7),
        custom,
    );
    let expected_tabs = vec![
        MappingDocument::Main,
        MappingDocument::Target(1),
        MappingDocument::Function(function),
        MappingDocument::Target(0),
    ];
    assert_eq!(app.mapping_workspace.tabs, expected_tabs);

    app.save_document_to(&project_path)
        .expect("project and layout save");
    let mut loaded = FerruleApp::default();
    loaded.load_project_from(&project_path);

    assert_eq!(loaded.project.extra_targets.len(), 2);
    let loaded_audit = &loaded.project.extra_targets[0];
    assert_eq!(loaded_audit.name, "audit");
    assert_eq!(loaded_audit.path.as_deref(), Some("outputs/audit.jsonl"));
    assert!(loaded_audit.options.json_lines);
    assert_eq!(loaded_audit.root.target_field, "auditRoot");
    assert_eq!(loaded.mapping_workspace.tabs, expected_tabs);
    assert_eq!(loaded.mapping_workspace.active, MappingDocument::Target(0));
    assert_eq!(
        canvas_position(
            &loaded.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::Graph(7)
        ),
        custom
    );
    assert!(
        loaded.mapping_workspace.target_canvases[&0]
            .snarl
            .nodes()
            .all(|node| !matches!(node, CanvasNode::Placeholder(_)))
    );
    assert!(!loaded.is_dirty());

    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
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
    app.main_canvas.snarl = build_snarl(&app.project);
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(73.0, 91.0),
    );
    move_canvas_node(
        &mut app.main_canvas.snarl,
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
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        egui::pos2(73.0, 91.0)
    );
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::Graph(7)),
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
    app.main_canvas.snarl = snarl;
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path)
        .expect("project and layout save");

    let mut loaded = FerruleApp {
        document: DocumentLocation::saved(project_path.clone()),
        ..Default::default()
    };
    loaded.load_project_from(&project_path);
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::Placeholder(0)),
        egui::pos2(180.0, 210.0)
    );
    let wires: Vec<_> = loaded
        .main_canvas
        .snarl
        .wires()
        .map(|(from, to)| {
            (
                loaded.main_canvas.snarl[from.node],
                loaded.main_canvas.snarl[to.node],
            )
        })
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
    app.main_canvas.snarl = build_snarl(&app.project);
    for node in app.main_canvas.snarl.nodes_mut() {
        if *node == CanvasNode::Graph(0) {
            *node = CanvasNode::Placeholder(0);
        }
    }
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(901.0, 733.0),
    );
    move_canvas_node(
        &mut app.main_canvas.snarl,
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
            .main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Graph(0))
    );
    assert!(
        !loaded
            .main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(0))
    );
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        expected_source
    );
    assert_eq!(
        canvas_position(&loaded.main_canvas.snarl, CanvasNode::Graph(1)),
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
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
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
    let arranged = canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0));
    let custom = egui::pos2(123.0, 456.0);
    move_canvas_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        custom,
    );
    app.mark_clean();
    app.rebase_history();

    arrange_snarl(
        &mut app.main_canvas.snarl,
        &app.main_canvas.node_sizes,
        crate::appearance::WireAppearance::default(),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        arranged
    );
    assert!(app.is_dirty());

    app.undo_project();
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        custom
    );
    assert!(!app.is_dirty());
    app.redo_project();
    assert_eq!(
        canvas_position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
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
    let mut app = FerruleApp {
        document: DocumentLocation::untitled(project_path.clone()),
        ..Default::default()
    };
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
fn first_save_rebases_relative_paths_from_the_untitled_document_base() {
    let project_path = temporary_project_path("first-save-rebase");
    let mut app = FerruleApp::default();
    app.project.source_path = Some("Cargo.toml".into());

    app.save_document_to(&project_path)
        .expect("untitled project saves");

    let stored = app
        .project
        .source_path
        .as_deref()
        .expect("source path remains configured");
    let resolved = project_path
        .parent()
        .expect("project has a parent")
        .join(stored);
    assert_eq!(
        std::fs::canonicalize(resolved).expect("rebased source exists"),
        std::fs::canonicalize("Cargo.toml").expect("workspace manifest exists")
    );
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn layout_failure_keeps_saved_project_and_editor_on_the_same_base() {
    let project_path = temporary_project_path("layout-failure");
    std::fs::create_dir_all(layout_path(&project_path))
        .expect("layout destination is blocked by a directory");
    let mut app = FerruleApp::default();
    app.project.source_path = Some("Cargo.toml".into());

    let outcome = app
        .save_document_to(&project_path)
        .expect("project save succeeds even when layout fails");

    assert!(outcome.layout_warning.is_some());
    assert_eq!(app.document, DocumentLocation::saved(project_path.clone()));
    let saved: Project =
        serde_json::from_slice(&std::fs::read(&project_path).expect("saved project is readable"))
            .expect("saved project parses");
    assert_eq!(saved.source_path, app.project.source_path);
    assert!(!app.is_dirty());
    std::fs::remove_dir_all(project_path.parent().expect("project has parent"))
        .expect("temporary test directory is removed");
}

#[test]
fn preview_executes_an_unsaved_project_without_writing_its_logical_output() -> anyhow::Result<()> {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-gui-preview-unsaved-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory)?;
    let logical_output = directory.join("must-not-be-written.xml");
    let mut app = FerruleApp {
        preview_draft: Some(crate::preview::PreviewDraft {
            target: crate::preview::PreviewTarget::Primary,
            input_identity: "input.xml".into(),
            output_identity: logical_output.display().to_string(),
            input_text: "<root/>".into(),
        }),
        ..FerruleApp::default()
    };

    app.execute_preview();

    assert!(
        app.preview_draft.is_none(),
        "successful preview closes setup"
    );
    assert!(app.show_run_report);
    assert!(!logical_output.exists());
    assert!(app.document.saved_path().is_none());
    assert!(!app.is_dirty());
    let Some(report) = app.run_report.as_mut() else {
        anyhow::bail!("successful preview has no report");
    };
    assert_eq!(
        report.report.kind,
        crate::run_report::RunReportKind::Preview
    );
    assert_eq!(report.report.outputs.len(), 1);
    assert!(matches!(
        report.report.outputs[0].preview(),
        crate::run_report::OutputPreview::Text { content, .. }
            if content.contains("<root")
    ));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn preview_uses_the_active_named_target_only() -> anyhow::Result<()> {
    let mut app = FerruleApp::default();
    app.project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.json".into()),
        schema: SchemaNode::group("audit", Vec::new()),
        options: FormatOptions::default(),
        root: Scope::default(),
    });
    app.mapping_workspace.active = MappingDocument::Target(0);
    app.begin_preview();
    let Some(draft) = app.preview_draft.as_mut() else {
        anyhow::bail!("named-target preview setup did not open");
    };
    draft.input_identity = "input.xml".into();
    draft.input_text = "<root/>".into();

    app.execute_preview();

    let Some(report) = app.run_report.as_ref() else {
        anyhow::bail!("successful preview has no report");
    };
    assert_eq!(report.report.outputs.len(), 1);
    assert_eq!(report.report.outputs[0].name, "audit");
    assert_eq!(report.report.outputs[0].path, PathBuf::from("audit.json"));
    Ok(())
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
        user_functions: Default::default(),
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
        user_functions: Default::default(),
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
        .map(|(output, input)| (snarl[output.node], output.output, input.input))
        .collect();
    wires.sort_by_key(|wire| format!("{wire:?}"));
    assert_eq!(
        wires,
        vec![
            (CanvasNode::SourceBlock(0), 0, 0),
            (CanvasNode::SourceBlock(0), 1, 1),
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
            user_functions: Default::default(),
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
    assert_eq!(app.main_canvas.snarl.nodes().count(), 2);

    std::fs::remove_dir_all(directory).expect("temporary test directory is removed");
}
