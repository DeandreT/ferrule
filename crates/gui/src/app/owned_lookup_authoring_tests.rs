//! Actual Lookup controls, exact-owner execution and durable typed history.
use super::*;
use crate::test_ui::ContextExt as _;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedSource, NamedTarget, OwnedLookup, OwnedSourceRef, SourceOwner};

#[path = "owned_lookup_context_regressions.rs"]
pub(super) mod context_regressions;
#[path = "owned_lookup_scope_regressions.rs"]
pub(super) mod scope_regressions;

const INPUT: &str = r#"{"Rows":[{"Code":"hit","PrimaryValue":"primary-first"},{"Code":"hit","PrimaryValue":"primary-later"},{"Code":"only-primary","PrimaryValue":"primary-only"}],"Alt":[{"AltKey":"hit","AltValue":"primary-alt"}],"Ref":{"Rows":[{"Code":"hit","PrimaryValue":"alias-shadow"}]}}"#;
const NAMED: &str = r#"{"Rows":[{"Code":"hit","NamedValue":"named-first"},{"Code":"hit","NamedValue":"named-later"},{"Code":"only-named","NamedValue":"named-only"}],"Alt":[{"OtherKey":"hit","OtherValue":"named-alt"}]}"#;

fn rows(name: &str, key: &str, value: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar(key, ScalarType::String),
            SchemaNode::scalar(value, ScalarType::String),
        ],
    )
    .repeating()
}
fn source(owner: SourceOwner, path: &str) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: vec![path.into()],
    }
}
fn named_owner() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
pub(super) fn fixture(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Input",
        vec![
            rows("Rows", "Code", "PrimaryValue"),
            rows("Alt", "AltKey", "AltValue"),
            SchemaNode::group("Ref", vec![rows("Rows", "Code", "PrimaryValue")]),
        ],
    );
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    let hints = std::env::temp_dir().join("owned-lookup-hints");
    app.project.source_path = Some(hints.join("input.json").display().to_string());
    app.project.target_path = Some(hints.join("output.json").display().to_string());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.extra_sources = vec![NamedSource {
        name: "Ref".into(),
        path: hints.join("named.json").display().to_string(),
        schema: SchemaNode::group(
            "Reference",
            vec![
                rows("Rows", "Code", "NamedValue"),
                rows("Alt", "OtherKey", "OtherValue"),
            ],
        ),
        options: mapping::FormatOptions {
            json_document: true,
            ..Default::default()
        },
        dynamic_path: None,
    }];
    app.project.graph.nodes = [
        (
            3,
            Node::Const {
                value: Value::String("hit".into()),
            },
        ),
        (
            7,
            Node::OwnedLookup(OwnedLookup {
                collection: source(SourceOwner::Primary, "Rows"),
                key: vec!["Code".into()],
                matches: 3,
                value: vec!["PrimaryValue".into()],
            }),
        ),
    ]
    .into();
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "Result".into(),
            node: 7,
        }],
        ..Default::default()
    };
    app.project.extra_targets = vec![NamedTarget {
        name: "Audit".into(),
        path: Some(hints.join("audit.json").display().to_string()),
        schema: app.project.target.clone(),
        options: app.project.target_options.clone(),
        root: app.project.root.clone(),
    }];
    rebuild(&mut app, document);
    app
}
pub(super) fn rebuild(app: &mut FerruleApp, document: MappingDocument) {
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mapping_workspace = Default::default();
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    for doc in [MappingDocument::Main, MappingDocument::Target(0)] {
        let state = canvas_mut(app, doc);
        let nodes = state
            .snarl
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<Vec<_>>();
        for (id, node) in nodes {
            state.snarl.get_node_info_mut(id).unwrap().pos = match node {
                CanvasNode::SourceBlock(i) => egui::pos2(0.0, i as f32 * 250.0),
                CanvasNode::TargetBlock(_) => egui::pos2(1050.0, 0.0),
                CanvasNode::Graph(3) => egui::pos2(350.0, 30.0),
                CanvasNode::Graph(9) => egui::pos2(600.0, 400.0),
                _ => egui::pos2(600.0, 30.0),
            };
        }
        state.reset_view();
    }
    app.mapping_workspace.active = document;
    app.mapping_workspace.focused = document;
    app.mark_clean();
    app.rebase_history();
}
pub(super) fn canvas_mut(
    app: &mut FerruleApp,
    document: MappingDocument,
) -> &mut CanvasDocumentState {
    match document {
        MappingDocument::Main => &mut app.main_canvas,
        MappingDocument::Target(index) => app
            .mapping_workspace
            .target_canvases
            .get_mut(&index)
            .unwrap(),
        _ => panic!("mapping canvas"),
    }
}
pub(super) fn snapshot(app: &FerruleApp) -> serde_json::Value {
    let mut layout =
        editor_state(&app.project, &app.main_canvas.snarl, &app.mapping_workspace).layout;
    layout.project_fingerprint = None;
    serde_json::json!({"project": app.project, "layout": layout})
}
pub(super) fn creation_snapshot(app: &FerruleApp, document: MappingDocument) -> serde_json::Value {
    let state = snapshot(app);
    // An unbound new node has no position on an inactive canvas until that
    // canvas is rebuilt. Its authored position belongs to the active canvas.
    let nodes = match document {
        MappingDocument::Main => &state["layout"]["nodes"],
        MappingDocument::Target(_) => &state["layout"]["target_nodes"]["0"],
        _ => panic!("mapping canvas"),
    };
    serde_json::json!({"project": state["project"], "nodes": nodes})
}
fn lookup(app: &FerruleApp) -> &OwnedLookup {
    let Node::OwnedLookup(lookup) = &app.project.graph.nodes[&7] else {
        panic!("owned lookup")
    };
    lookup
}
pub(super) fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}
pub(super) fn frame(
    app: &mut FerruleApp,
    ctx: &egui::Context,
    enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_test_ui(
        egui::RawInput {
            time: Some(ctx.cumulative_frame_nr() as f64 / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_mapping_workspace_canvas(ui, enabled),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}
pub(super) fn settle(app: &mut FerruleApp, ctx: &egui::Context, enabled: bool) -> egui::FullOutput {
    let mut output = frame(app, ctx, enabled, vec![]);
    for _ in 0..4 {
        output = frame(app, ctx, enabled, vec![]);
    }
    output
}
pub(super) fn settle_all(app: &mut FerruleApp, ctx: &egui::Context) {
    let active = app.mapping_workspace.active;
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        app.mapping_workspace.active = document;
        settle(app, ctx, true);
    }
    app.mapping_workspace.active = active;
    settle(app, ctx, true);
}
pub(super) fn visible(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        label: &str,
        found: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text)
                if text.galley.text() == label
                    && clip.contains(text.visual_bounding_rect().center()) =>
            {
                found.push(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, label, found);
                }
            }
            _ => {}
        }
    }
    let mut found = vec![];
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, label, &mut found);
    }
    found
}
pub(super) fn pointer(
    app: &mut FerruleApp,
    ctx: &egui::Context,
    enabled: bool,
    pos: egui::Pos2,
    button: egui::PointerButton,
) {
    frame(app, ctx, enabled, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            enabled,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}
pub(super) fn click_nth(
    app: &mut FerruleApp,
    ctx: &egui::Context,
    enabled: bool,
    label: &str,
    index: usize,
) {
    let output = settle(app, ctx, enabled);
    let found = visible(&output, label);
    assert!(
        found.len() > index,
        "missing widget {label:?}; found {found:?}; shapes={:?}",
        output.shapes
    );
    pointer(
        app,
        ctx,
        enabled,
        found[index],
        egui::PointerButton::Primary,
    );
}
fn choose(app: &mut FerruleApp, ctx: &egui::Context, current: &str, selected: &str) {
    let output = settle(app, ctx, true);
    if visible(&output, "Return field").is_empty() {
        open_lookup(app, ctx);
    }
    for label in [current, selected] {
        let output = settle(app, ctx, true);
        let pos = *visible(&output, label)
            .last()
            .unwrap_or_else(|| panic!("missing choice {label:?}"));
        pointer(app, ctx, true, pos, egui::PointerButton::Primary);
    }
}
pub(super) fn open_lookup(app: &mut FerruleApp, ctx: &egui::Context) {
    let label = crate::graph_viewer::readable_node_label(&app.project.graph.nodes[&7]);
    let output = settle(app, ctx, true);
    let title = visible(&output, &label)[0];
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let pos = visible(&output, &pencil)
        .into_iter()
        .min_by(|a, b| a.distance(title).total_cmp(&b.distance(title)))
        .expect("lookup pencil");
    pointer(app, ctx, true, pos, egui::PointerButton::Primary);
    settle(app, ctx, true);
}
pub(super) fn assert_typed_edit(app: &FerruleApp) {
    let Some(HistoryRecord::Command(record)) = app.history.next_undo() else {
        panic!("typed command")
    };
    let [command] = record.commands() else {
        panic!("one command")
    };
    assert_eq!(
        command.action(),
        &editor_ui::GraphAction::EditNode {
            node: 7,
            property: editor_ui::NodeProperty::Other
        }
    );
    assert_eq!(command.patch().changed_nodes().collect::<Vec<_>>(), [7]);
}

/// Drive the same SnarlViewer connection action used by native pin drags.
fn connect_needle(app: &mut FerruleApp, node: NodeId) {
    wire_action(
        app,
        CanvasNode::Graph(3),
        0,
        CanvasNode::Graph(node),
        0,
        false,
    )
    .unwrap();
}

pub(super) fn wire_action(
    app: &mut FerruleApp,
    from_node: CanvasNode,
    output: usize,
    to_node: CanvasNode,
    input: usize,
    disconnect: bool,
) -> Result<(), String> {
    use egui_snarl::ui::SnarlViewer as _;
    let document = app.mapping_workspace.active;
    let paths = SourcePathCatalog::new(&app.project.source, &app.project.extra_sources)
        .with_project_ownership(&app.project);
    let sources = project_source_blocks(&app.project);
    let schema = match document {
        MappingDocument::Target(index) => &app.project.extra_targets[index].schema,
        _ => &app.project.target,
    };
    let targets = target_blocks(schema);
    let mut root = match document {
        MappingDocument::Target(index) => app.project.extra_targets[index].root.clone(),
        _ => app.project.root.clone(),
    };
    let mut snarl = std::mem::take(&mut canvas_mut(app, document).snarl);
    let id = |node| {
        snarl
            .node_ids()
            .find_map(|(id, candidate)| (*candidate == node).then_some(id))
            .unwrap()
    };
    let from = snarl.out_pin(OutPinId {
        node: id(from_node),
        output,
    });
    let to = snarl.in_pin(InPinId {
        node: id(to_node),
        input,
    });
    let mut scroll = Default::default();
    let mut viewer = GraphViewer {
        graph: &mut app.project.graph,
        root_scope: &mut root,
        primary_root_authoring: false,
        extra_targets: &app.project.extra_targets,
        inactive_target_scopes: &[],
        project_references: crate::graph_viewer::ProjectGraphReferences::new(
            &app.project.failure_rules,
            &app.project.extra_sources,
        ),
        source_blocks: &sources,
        target_blocks: &targets,
        source_x12: false,
        target_x12: false,
        source_paths: &paths,
        function_names: Default::default(),
        function_inputs: Default::default(),
        parameter_names: Default::default(),
        protected_output: None,
        function_output: None,
        requested_function_open: None,
        colors: Default::default(),
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
        pin_interaction_ids: vec![],
        planned_edits: vec![],
        error: None,
    };
    if disconnect {
        viewer.disconnect(&from, &to, &mut snarl);
    } else {
        viewer.connect(&from, &to, &mut snarl);
    }
    let error = viewer.error.take();
    let edits = std::mem::take(&mut viewer.planned_edits);
    drop(viewer);
    match document {
        MappingDocument::Target(index) => app.project.extra_targets[index].root = root,
        _ => app.project.root = root,
    }
    canvas_mut(app, document).snarl = snarl;
    app.queue_canvas_edits(document, edits);
    app.observe_editor_history(std::time::Instant::now(), false);
    error.map_or(Ok(()), Err)
}
fn assert_output(app: &FerruleApp, expected: Option<&str>) {
    let input = format_json::from_str(INPUT, &app.project.source).unwrap();
    let named = format_json::from_str(NAMED, &app.project.extra_sources[0].schema).unwrap();
    let validation = engine::validate(&app.project);
    assert!(validation.is_empty(), "{validation:?}");
    let result = engine::run_outputs_with_sources_and_context(
        &app.project,
        &input,
        vec![("Ref".into(), named)],
        &engine::ExecutionContext::new(Path::new("lookup-project.json")),
    )
    .unwrap();
    let expected = ir::Instance::Group(
        vec![(
            "Result".into(),
            ir::Instance::Scalar(expected.map_or(Value::Null, |value| Value::String(value.into()))),
        )]
        .into(),
    );
    assert_eq!(result.primary, expected);
    assert_eq!(result.extras[0].instance, expected);
}

#[test]
fn actual_owner_and_collection_pickers_recompute_only_local_fields_and_keep_wiring() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = fixture(document);
        let ctx = context();
        settle(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        let before = snapshot(&app);
        assert_output(&app, Some("primary-first"));
        choose(&mut app, &ctx, "Primary input", "Named input: Ref");
        assert_eq!(lookup(&app).collection, source(named_owner(), "Rows"));
        assert_eq!(lookup(&app).key, ["Code"]);
        assert_eq!(lookup(&app).value, ["Code"]);
        assert_eq!(lookup(&app).matches, 3);
        assert_typed_edit(&app);
        // The return selector is the second Code after the owner change.
        click_nth(&mut app, &ctx, true, "Code", 1);
        let choices = settle(&mut app, &ctx, true);
        assert!(visible(&choices, "PrimaryValue").is_empty());
        click_nth(&mut app, &ctx, true, "NamedValue", 0);
        assert_output(&app, Some("named-first"));
        choose(&mut app, &ctx, "Rows", "Alt");
        assert_eq!(lookup(&app).key, ["OtherKey"]);
        assert_eq!(lookup(&app).value, ["OtherKey"]);
        choose(&mut app, &ctx, "Named input: Ref", "Primary input");
        assert_eq!(lookup(&app).collection, source(SourceOwner::Primary, "Alt"));
        assert_eq!(lookup(&app).key, ["AltKey"]);
        assert_eq!(lookup(&app).value, ["AltKey"]);
        let changed = snapshot(&app);
        assert_eq!(changed["layout"], before["layout"]);
        let count = app.history.undo_len();
        for _ in 0..count {
            app.undo_project();
        }
        assert_eq!(snapshot(&app), before);
        for _ in 0..count {
            app.redo_project();
        }
        assert_eq!(snapshot(&app), changed);
        for doc in [MappingDocument::Main, MappingDocument::Target(0)] {
            let snarl = &canvas_mut(&mut app, doc).snarl;
            let incoming = snarl
                .wires()
                .filter(|(_, to)| snarl[to.node] == CanvasNode::Graph(7))
                .map(|(from, to)| (snarl[from.node], to.input))
                .collect::<Vec<_>>();
            assert_eq!(incoming, [(CanvasNode::Graph(3), 0)]);
        }
    }
}

#[test]
fn actual_owned_creation_is_explicit_typed_and_legacy_creation_stays_available() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = fixture(document);
        let ctx = context();
        settle(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        let before = creation_snapshot(&app, document);
        pointer(
            &mut app,
            &ctx,
            true,
            egui::pos2(1100.0, 850.0),
            egui::PointerButton::Secondary,
        );
        click_nth(&mut app, &ctx, true, "Lookup from one input", 0);
        assert_eq!(app.project.graph.nodes.len(), 4);
        let (&id, Node::OwnedLookup(created)) = app
            .project
            .graph
            .nodes
            .iter()
            .find(|(id, node)| **id != 7 && matches!(node, Node::OwnedLookup(_)))
            .unwrap()
        else {
            panic!("created lookup")
        };
        assert_eq!(created.collection.owner, SourceOwner::Primary);
        assert!(matches!(
            app.project.graph.nodes[&created.matches],
            Node::Unconnected
        ));
        let Some(HistoryRecord::Command(record)) = app.history.next_undo() else {
            panic!("typed insertion")
        };
        assert!(
            matches!(record.commands()[0].action(), editor_ui::GraphAction::InsertNode { node, .. } if *node == id)
        );
        settle_all(&mut app, &ctx);
        let after = creation_snapshot(&app, document);
        app.undo_project();
        assert_eq!(creation_snapshot(&app, document), before);
        app.redo_project();
        assert_eq!(creation_snapshot(&app, document), after);
        connect_needle(&mut app, id);
        let Node::OwnedLookup(wired) = &app.project.graph.nodes[&id] else {
            panic!("owned lookup")
        };
        assert_eq!(wired.matches, 3);
        let Some(HistoryRecord::Command(record)) = app.history.next_undo() else {
            panic!("typed wire")
        };
        assert!(matches!(
            record.commands()[0].action(),
            editor_ui::GraphAction::ConnectWire { source: 3, .. }
        ));
        app.undo_project();
        assert_eq!(creation_snapshot(&app, document), after);
        app.redo_project();
        // Opening a menu never changes an existing legacy lookup.
        app.project.graph.nodes.insert(
            7,
            Node::Lookup {
                collection: vec!["Rows".into()],
                key: vec!["Code".into()],
                matches: 3,
                value: vec!["PrimaryValue".into()],
            },
        );
        rebuild(&mut app, document);
        let before = snapshot(&app);
        settle(&mut app, &ctx, true);
        assert_eq!(snapshot(&app), before);
        pointer(
            &mut app,
            &ctx,
            true,
            egui::pos2(1100.0, 850.0),
            egui::PointerButton::Secondary,
        );
        frame(
            &mut app,
            &ctx,
            true,
            vec![egui::Event::Text("Lookup".into())],
        );
        let output = settle(&mut app, &ctx, true);
        assert!(
            !visible(&output, "Lookup").is_empty(),
            "legacy palette entry remains"
        );
    }
}

#[test]
fn named_primary_has_a_distinct_owner_control_label() {
    let mut app = fixture(MappingDocument::Main);
    app.project.extra_sources[0].name = "primary".into();
    rebuild(&mut app, MappingDocument::Main);
    let ctx = context();
    choose(&mut app, &ctx, "Primary input", "Named input: primary");
    assert_eq!(
        lookup(&app).collection.owner,
        SourceOwner::Named {
            name: "primary".into()
        }
    );
    assert_eq!(lookup(&app).matches, 3);
}

#[test]
fn display_invalid_metadata_and_locked_or_unavailable_owners_are_noops() {
    for unavailable in ["missing", "dynamic", "duplicate", "locked", "invalid-path"] {
        let mut app = fixture(MappingDocument::Main);
        match unavailable {
            "missing" => {
                if let Node::OwnedLookup(node) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    node.collection.owner = SourceOwner::Named {
                        name: "Missing".into(),
                    };
                }
            }
            "dynamic" => {
                app.project.extra_sources[0].dynamic_path =
                    Some(mapping::DynamicSourcePath::legacy(3, vec!["Rows".into()]));
                if let Node::OwnedLookup(node) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    node.collection.owner = named_owner();
                }
            }
            "duplicate" => {
                app.project
                    .extra_sources
                    .push(app.project.extra_sources[0].clone());
                if let Node::OwnedLookup(node) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    node.collection.owner = named_owner();
                }
            }
            "invalid-path" => {
                if let Node::OwnedLookup(node) = app.project.graph.nodes.get_mut(&7).unwrap() {
                    node.collection.path = vec!["SavedMissingCollection".into()];
                    node.key = vec!["SavedKey".into()];
                    node.value = vec!["SavedValue".into()];
                }
            }
            _ => {}
        }
        rebuild(&mut app, MappingDocument::Main);
        let ctx = context();
        let before = snapshot(&app);
        open_lookup(&mut app, &ctx);
        let output = settle(&mut app, &ctx, unavailable != "locked");
        if unavailable == "locked" {
            click_nth(&mut app, &ctx, false, "Primary input", 0);
        }
        if ["missing", "dynamic", "duplicate"].contains(&unavailable) {
            assert!(
                !visible(
                    &output,
                    "This lookup owner is unavailable. Stored paths and wiring are unchanged."
                )
                .is_empty()
            );
        }
        assert_eq!(snapshot(&app), before, "{unavailable}");
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
    }
}

#[test]
fn legacy_settings_only_convert_on_explicit_action_and_preserve_needle() {
    let mut app = fixture(MappingDocument::Main);
    app.project.graph.nodes.insert(
        7,
        Node::Lookup {
            collection: vec!["Rows".into()],
            key: vec!["SavedKey".into()],
            matches: 3,
            value: vec!["SavedValue".into()],
        },
    );
    rebuild(&mut app, MappingDocument::Main);
    let ctx = context();
    let before = snapshot(&app);
    let output = settle(&mut app, &ctx, true);
    let title = visible(&output, "Rows")[0];
    frame(&mut app, &ctx, true, vec![egui::Event::PointerMoved(title)]);
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    click_nth(&mut app, &ctx, true, &pencil, 0);
    settle(&mut app, &ctx, true);
    assert_eq!(snapshot(&app), before);
    assert!(!app.can_undo());
    click_nth(&mut app, &ctx, true, "Use an exact input", 0);
    assert_eq!(lookup(&app).matches, 3);
    assert_eq!(lookup(&app).collection.owner, SourceOwner::Primary);
    assert_typed_edit(&app);
    app.undo_project();
    assert_eq!(snapshot(&app), before);
}

#[test]
fn expressions_shared_with_specialized_item_contexts_are_read_only() {
    let mut app = fixture(MappingDocument::Main);
    app.project.graph.nodes.insert(
        9,
        Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection: vec!["Rows".into()],
            value: vec![],
            expression: Some(7),
            arg: None,
        },
    );
    rebuild(&mut app, MappingDocument::Main);
    let ctx = context();
    let before = snapshot(&app);
    open_lookup(&mut app, &ctx);
    settle(&mut app, &ctx, true);
    click_nth(&mut app, &ctx, true, "Primary input", 0);
    assert_eq!(snapshot(&app), before);
    assert!(!app.can_undo());
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-owned-lookup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn owner_specific_miss_and_saved_reopened_lookup_execute_in_preview_and_public_run()
-> anyhow::Result<()> {
    let scratch = Scratch::new();
    std::fs::write(scratch.0.join("named.json"), NAMED)?;
    let mut app = fixture(MappingDocument::Target(0));
    app.project.extra_sources[0].path = scratch.0.join("named.json").display().to_string();
    app.project.extra_targets[0].path = Some(scratch.0.join("audit.json").display().to_string());
    let ctx = context();
    settle(&mut app, &ctx, true);
    choose(&mut app, &ctx, "Primary input", "Named input: Ref");
    click_nth(&mut app, &ctx, true, "Code", 1);
    click_nth(&mut app, &ctx, true, "NamedValue", 0);
    assert_output(&app, Some("named-first"));
    app.project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("only-primary".into()),
        },
    );
    assert_output(&app, None); // Primary has a match; the chosen named input does not.
    let path = scratch.0.join("project.json");
    let before = snapshot(&app);
    let saved = app.save_document_to(&path)?;
    assert!(saved.validation_issues.is_empty());
    assert!(saved.layout_warning.is_none());
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    reopened.open_target_tab(0);
    assert!(reopened.ensure_target_canvas(0));
    assert_eq!(snapshot(&reopened), before);
    assert_output(&reopened, None);
    let input_path = scratch.0.join("input.json");
    let output_path = scratch.0.join("result.json");
    std::fs::write(&input_path, INPUT)?;
    let run = cli::run_project_with_paths(&path, Some(&input_path), Some(&output_path))?;
    assert_eq!(run.artifacts.len(), 2);
    for output in [&output_path, &scratch.0.join("audit.json")] {
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(output)?)?,
            serde_json::json!({})
        );
    }
    reopened.preview_draft = Some(crate::preview::PreviewDraft {
        target: crate::preview::PreviewTarget::Primary,
        input_identity: "input.json".into(),
        output_identity: scratch.0.join("not-written.json").display().to_string(),
        input_text: INPUT.into(),
        debug_breakpoint: None,
    });
    reopened.execute_preview();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while reopened.pending_preview.is_some() && std::time::Instant::now() < deadline {
        reopened.poll_preview(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(reopened.pending_preview.is_none());
    let report = reopened.run_report.as_mut().expect("public preview report");
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("JSON preview")
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(content)?,
        serde_json::json!({})
    );
    assert!(!scratch.0.join("not-written.json").exists());
    Ok(())
}
