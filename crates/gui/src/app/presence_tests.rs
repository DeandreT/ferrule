//! Actual Inspector controls, typed history, durable layout, and public Preview.

use super::*;
use crate::test_ui::ContextExt as _;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget, ScopeConstruction, ScopeIteration};

mod context_regressions;

const INPUT: &str = r#"{"Rows":[{"Keep":true,"Name":"first"},{"Keep":false,"Name":"second"},{"Keep":true,"Name":"third"}]}"#;

fn fixture(document: MappingDocument, presence: bool) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Name", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    app.project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![
                            SchemaNode::scalar("Position", ScalarType::Int),
                            SchemaNode::group(
                                "Detail",
                                vec![SchemaNode::scalar("Name", ScalarType::String)],
                            ),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: Some(vec!["Rows".into()]),
    };
    app.project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (2, field("Keep")),
        (3, field("Name")),
        (
            4,
            Node::Position {
                collection: vec!["Rows".into()],
            },
        ),
        (
            5,
            Node::Call {
                function: "exists".into(),
                args: vec![3],
            },
        ),
    ]
    .into_iter()
    .collect();
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            children: vec![Scope {
                target_field: "Item".into(),
                iteration: if presence {
                    ScopeIteration::OptionalPresence { condition: 0 }
                } else {
                    ScopeIteration::None
                },
                bindings: vec![Binding {
                    target_field: "Position".into(),
                    node: 4,
                }],
                children: vec![Scope {
                    target_field: "Detail".into(),
                    bindings: vec![Binding {
                        target_field: "Name".into(),
                        node: 3,
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    app.project.extra_targets = vec![NamedTarget {
        name: "Other".into(),
        path: Some("other.json".into()),
        schema: app.project.target.clone(),
        options: app.project.target_options.clone(),
        root: app.project.root.clone(),
    }];
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    let ids = app
        .main_canvas
        .snarl
        .node_ids()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    for id in ids {
        app.main_canvas.snarl.get_node_info_mut(id).unwrap().pos += egui::vec2(31.0, 47.0);
    }
    app.mapping_workspace.active = document;
    app.mapping_workspace.focused = document;
    app.selected_scope = vec![0, 0];
    app.mark_clean();
    app.rebase_history();
    assert!(engine::validate(&app.project).is_empty());
    app
}

fn active_root(app: &FerruleApp) -> &Scope {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    }
}

fn active_scope(app: &FerruleApp) -> &Scope {
    &active_root(app).children[0].children[0]
}

fn key(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).unwrap()
}

fn layout(app: &FerruleApp) -> String {
    format!(
        "{:?} {:?}",
        CanvasLayout::capture_nodes(&app.main_canvas.snarl),
        app.mapping_workspace
            .target_canvases
            .iter()
            .map(|(index, canvas)| (*index, CanvasLayout::capture_nodes(&canvas.snarl)))
            .collect::<Vec<_>>()
    )
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context.all_styles_mut(|style| {
        style.animation_time = 0.0;
    });
    context
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.handle_history_shortcuts(ui.ctx(), enabled);
            app.show_inspector(ui, enabled);
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    fn visit(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(text) if clip.contains_rect(text.visual_bounding_rect()) => {
                out.push((
                    text.galley.text().into(),
                    text.visual_bounding_rect().center(),
                ));
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut out);
    }
    out
}

fn click(app: &mut FerruleApp, context: &egui::Context, enabled: bool, label: &str, last: bool) {
    let mut output = frame(app, context, enabled, vec![]);
    for _ in 0..3 {
        output = frame(app, context, enabled, vec![]);
    }
    let candidates = texts(&output)
        .into_iter()
        .filter(|(text, _)| crate::test_ui::inspector_label_matches(text, label))
        .collect::<Vec<_>>();
    let position = if last {
        candidates.last()
    } else {
        candidates.first()
    }
    .unwrap_or_else(|| panic!("missing {label}: {:?}", texts(&output)))
    .1;
    for pressed in [true, false] {
        frame(
            app,
            context,
            enabled,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn choose(app: &mut FerruleApp, context: &egui::Context, from: NodeId, to: NodeId) {
    click(app, context, true, &format!("{from}: expression"), false);
    click(app, context, true, &format!("{to}: expression"), true);
}

fn outputs(app: &FerruleApp) -> engine::ExecutionOutputs {
    let input = format_json::from_str(INPUT, &app.project.source).unwrap();
    engine::run_outputs(&app.project, &input).unwrap()
}

fn active_output<'a>(app: &FerruleApp, outputs: &'a engine::ExecutionOutputs) -> &'a Instance {
    match app.mapping_workspace.active {
        MappingDocument::Target(_) => &outputs.extras[0].instance,
        _ => &outputs.primary,
    }
}

fn item_positions(instance: &Instance) -> Vec<Vec<i64>> {
    instance
        .field("Rows")
        .unwrap()
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            row.field("Item")
                .unwrap()
                .as_repeated()
                .unwrap()
                .iter()
                .map(
                    |item| match item.field("Position").unwrap().as_scalar().unwrap() {
                        Value::Int(value) => *value,
                        other => panic!("position: {other:?}"),
                    },
                )
                .collect()
        })
        .collect()
}

#[test]
fn presence_real_controls_create_replace_remove_and_preserve_content_on_both_targets() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = fixture(document, false);
        let original = key(&app);
        let original_scope = active_scope(&app).clone();
        let original_layout = layout(&app);
        let context = context();
        click(&mut app, &context, true, "Optional presence", false);
        assert_eq!(active_scope(&app).presence(), Some(0));
        choose(&mut app, &context, 0, 2);
        assert_eq!(active_scope(&app).presence(), Some(2));
        let output = outputs(&app);
        assert_eq!(
            item_positions(active_output(&app, &output)),
            vec![vec![1], vec![], vec![3]]
        );
        let untouched = match document {
            MappingDocument::Main => &output.extras[0].instance,
            _ => &output.primary,
        };
        assert_eq!(item_positions(untouched), vec![vec![1], vec![2], vec![3]]);
        assert_eq!(active_scope(&app).bindings, original_scope.bindings);
        assert_eq!(active_scope(&app).children, original_scope.children);
        assert_eq!(layout(&app), original_layout);
        assert!(matches!(
            app.history.next_undo(),
            Some(HistoryRecord::Command(_))
        ));
        let edited = key(&app);
        app.undo_project();
        assert_eq!(active_scope(&app).presence(), Some(0));
        app.redo_project();
        assert_eq!(key(&app), edited);
        app.selected_scope = vec![0, 0];
        choose(&mut app, &context, 2, 1);
        assert_eq!(
            item_positions(active_output(&app, &outputs(&app))),
            vec![Vec::<i64>::new(); 3]
        );
        click(&mut app, &context, true, "Optional presence", false);
        assert_eq!(key(&app), original);
        assert_eq!(layout(&app), original_layout);
    }
}

#[test]
fn presence_locked_controls_leave_project_and_history_unchanged() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for presence in [false, true] {
            let mut app = fixture(document, presence);
            app.begin_preview();
            assert!(app.preview_draft.is_some());
            let before = (key(&app), layout(&app));
            let context = context();
            click(&mut app, &context, false, "Optional presence", false);
            if presence {
                click(&mut app, &context, false, "0: expression", false);
            }
            assert_eq!((key(&app), layout(&app)), before);
            assert!(!app.is_dirty() && !app.can_undo() && !app.history.can_redo());
        }
    }
}

#[test]
fn presence_typed_edits_match_snapshot_history() {
    use super::typed_edit_oracle::{assert_typed_edit_matches_snapshots, newest_command};
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for present in [false, true] {
            let app = assert_typed_edit_matches_snapshots(
                || fixture(document, present),
                |app| {
                    let context = context();
                    if present {
                        choose(app, &context, 0, 2);
                    } else {
                        click(app, &context, true, "Optional presence", false);
                    }
                },
                "Change scope iteration",
            );
            assert!(matches!(
                newest_command(&app).action(),
                editor_ui::GraphAction::EditScope {
                    setting: editor_ui::ScopeSetting::Iteration,
                    ..
                }
            ));
        }
    }
}

fn profile(app: &FerruleApp) -> crate::scope_editor::presence_controls::Profile {
    crate::scope_editor::presence_controls::profile(
        &app.project,
        active_root(app),
        &app.project.target,
        &[0, 0],
        true,
    )
}

#[test]
fn presence_incompatible_routes_controls_and_constructions_are_not_offered() {
    let base = fixture(MappingDocument::Main, false);
    for edit in [
        |scope: &mut Scope| {
            scope.filter = Some(0);
        },
        |scope: &mut Scope| {
            scope.sort_by = Some(4);
        },
        |scope: &mut Scope| {
            scope.group_by = Some(4);
        },
        |scope: &mut Scope| {
            scope.post_group_filter = Some(0);
        },
        |scope: &mut Scope| {
            scope
                .windows
                .push(mapping::SequenceWindow::First { count: 4 });
        },
        |scope: &mut Scope| {
            scope
                .dynamic_bindings
                .push(mapping::DynamicBinding { key: 3, value: 3 });
        },
        |scope: &mut Scope| {
            scope.merge_dynamic_fields = true;
        },
        |scope: &mut Scope| {
            scope.construction = ScopeConstruction::CopyCurrentSource;
        },
        |scope: &mut Scope| {
            scope.iteration_output = mapping::IterationOutput::First;
        },
        |scope: &mut Scope| {
            scope.iteration = ScopeIteration::Source(vec!["Rows".into()]);
        },
        |scope: &mut Scope| {
            scope.iteration =
                ScopeIteration::Concatenate(mapping::ScopeSequence::new(Scope::default(), vec![]));
        },
    ] {
        let mut app = fixture(MappingDocument::Main, false);
        edit(&mut app.project.root.children[0].children[0]);
        let before = key(&app);
        let mut scope = active_scope(&app).clone();
        let context = context();
        context.run_test_ui(Default::default(), |ui| {
            crate::scope_editor::presence_controls::show(
                ui,
                &mut scope,
                &app.project.graph,
                &profile(&app),
            )
        });
        assert_eq!(scope, *active_scope(&app));
        assert_eq!(key(&app), before);
        assert!(!profile(&app).is_editable());
    }
    let mut app = base;
    app.project.root.children[0].iteration = ScopeIteration::OwnedSource {
        source: mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: vec!["Rows".into()],
        },
        output_path: None,
    };
    assert!(!profile(&app).is_editable());
    assert!(
        !crate::scope_editor::presence_controls::profile(
            &app.project,
            active_root(&app),
            &app.project.target,
            &[0, 0],
            false
        )
        .is_editable()
    );
}

#[test]
fn presence_picker_retains_but_does_not_offer_missing_non_boolean_or_private_conditions() {
    let mut app = fixture(MappingDocument::Main, true);
    app.project.graph.nodes.insert(6, Node::Unconnected);
    app.project.graph.nodes.insert(
        7,
        Node::Call {
            function: "exists".into(),
            args: vec![6],
        },
    );
    app.project.graph.nodes.insert(
        8,
        Node::Call {
            function: "not".into(),
            args: vec![8],
        },
    );
    app.project.graph.nodes.insert(
        9,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        10,
        Node::Call {
            function: "exists".into(),
            args: vec![9],
        },
    );
    assert_eq!(profile(&app).condition_ids(), &[0, 1, 2, 5]);
    app.project.root.children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 99 };
    app.mark_clean();
    app.rebase_history();
    let before = key(&app);
    let context = context();
    frame(&mut app, &context, true, vec![]);
    assert_eq!(key(&app), before);
    assert!(!app.can_undo());
}

#[test]
fn presence_picker_keeps_multi_hop_parent_frames_and_refuses_oversized_graphs() {
    let mut app = fixture(MappingDocument::Main, true);
    if let ir::SchemaKind::Group { children, .. } = &mut app.project.source.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
    {
        children.push(SchemaNode::group("Sub", vec![]).repeating());
    }
    app.project.root.children[0].iteration =
        ScopeIteration::Source(vec!["Rows".into(), "Sub".into()]);
    assert!(
        profile(&app).condition_ids().contains(&2),
        "outer Rows/Keep remains in context"
    );
    for id in 100..4200 {
        app.project.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::Bool(true),
            },
        );
    }
    assert!(!profile(&app).is_editable());
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-presence-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn preview(
    app: &mut FerruleApp,
    context: &egui::Context,
    input: &str,
    output: &Path,
) -> Option<String> {
    app.run_report = None;
    app.preview_draft = Some(crate::preview::PreviewDraft {
        target: match app.mapping_workspace.active {
            MappingDocument::Target(_) => crate::preview::PreviewTarget::Named("Other".into()),
            _ => crate::preview::PreviewTarget::Primary,
        },
        input_identity: app
            .project
            .source_path
            .clone()
            .unwrap_or_else(|| "input.json".into()),
        output_identity: output.display().to_string(),
        input_text: input.into(),
        debug_breakpoint: None,
    });
    app.execute_preview();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while app.pending_preview.is_some() {
        assert!(std::time::Instant::now() < deadline, "Preview completes");
        app.poll_preview(context);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!output.exists(), "Preview does not publish files");
    app.run_report
        .as_mut()
        .map(|report| match report.report.outputs[0].preview() {
            crate::run_report::OutputPreview::Text { content, .. } => content.clone(),
            _ => panic!("JSON Preview"),
        })
}

#[test]
fn presence_save_reopen_preview_retains_parent_positions_children_and_exact_layout()
-> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let scratch = Scratch::new();
        let mut app = fixture(document, false);
        let context = context();
        click(&mut app, &context, true, "Optional presence", false);
        choose(&mut app, &context, 0, 2);
        let original_layout = layout(&app);
        let path = scratch.0.join("project.json");
        let outcome = app.save_document_to(&path)?;
        assert!(outcome.validation_issues.is_empty() && outcome.layout_warning.is_none());
        let saved = key(&app);
        let project_bytes = std::fs::read(&path)?;
        let layout_path = crate::layout_store::layout_path(&path);
        let layout_bytes = std::fs::read(&layout_path)?;
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        reopened.open_target_tab(0);
        assert!(reopened.ensure_target_canvas(0));
        reopened.mapping_workspace.active = document;
        reopened.selected_scope = vec![0, 0];
        assert_eq!(key(&reopened), saved);
        assert_eq!(layout(&reopened), original_layout);
        let expected =
            format_json::to_string(&app.project.target, active_output(&app, &outputs(&app)))?;
        let text = preview(
            &mut reopened,
            &context,
            INPUT,
            &scratch.0.join("not-written.json"),
        )
        .expect("successful public Preview");
        assert_eq!(text, expected);
        assert_eq!(std::fs::read(&path)?, project_bytes);
        assert_eq!(std::fs::read(&layout_path)?, layout_bytes);
        assert_eq!(layout(&reopened), original_layout);
        // A missing Boolean remains an execution error, not false/absent output.
        assert!(
            preview(
                &mut reopened,
                &context,
                r#"{"Rows":[{"Name":"missing"}]}"#,
                &scratch.0.join("missing.json")
            )
            .is_none()
        );
        assert!(
            reopened
                .diagnostics
                .items()
                .iter()
                .any(|item| format!("{item:?}").contains("null"))
        );
    }
    Ok(())
}

#[test]
fn presence_retains_existing_non_boolean_runtime_error() {
    let scratch = Scratch::new();
    let mut app = fixture(MappingDocument::Main, true);
    app.project.root.children[0].children[0].iteration =
        ScopeIteration::OptionalPresence { condition: 3 };
    let input = format_json::from_str(INPUT, &app.project.source).unwrap();
    assert!(matches!(
        engine::run(&app.project, &input),
        Err(engine::EngineError::NotABool {
            node: 3,
            found: "string"
        })
    ));
    let before = key(&app);
    let context = context();
    frame(&mut app, &context, true, vec![]);
    assert_eq!(key(&app), before);
    assert!(
        preview(
            &mut app,
            &context,
            INPUT,
            &scratch.0.join("not-written.json")
        )
        .is_none()
    );
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| format!("{item:?}").contains("string"))
    );
}

#[test]
fn presence_full_desktop_viewport_controls_and_preview_render() -> anyhow::Result<()> {
    use egui_kittest::{Harness, kittest::Queryable as _};
    let mut frame = eframe::Frame::_new_kittest();
    let mut initialized = false;
    let mut harness = Harness::builder()
        .with_size((1280.0, 800.0))
        .with_accessibility_check(false)
        .with_os(egui::os::OperatingSystem::Nix)
        .build_ui_state(
            move |ui, app: &mut FerruleApp| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.animation_time = 0.0;
                        style.scroll_animation = egui::style::ScrollAnimation::none();
                    });
                    initialized = true;
                }
                eframe::App::ui(app, ui, &mut frame);
            },
            fixture(MappingDocument::Main, false),
        );
    harness.run_steps(4);
    harness.get_by_label("Optional presence").click();
    harness.run_steps(4);
    assert_eq!(active_scope(harness.state()).presence(), Some(0));
    let image = harness.render().map_err(anyhow::Error::msg)?;
    assert_eq!(image.dimensions(), (1280, 800));
    if let Some(directory) = std::env::var_os("FERRULE_PRESENCE_SHOTS") {
        image.save(PathBuf::from(directory).join("optional-presence-1280x800.png"))?;
    }
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run_steps(3);
    assert_eq!(active_scope(harness.state()).presence(), None);
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    harness.run_steps(3);
    assert_eq!(active_scope(harness.state()).presence(), Some(0));
    let scratch = Scratch::new();
    let path = scratch.0.join("mapping.json");
    let saved = harness.state_mut().save_document_to(&path)?;
    assert!(saved.validation_issues.is_empty() && saved.layout_warning.is_none());
    harness.state_mut().load_project_from(&path);
    harness.state_mut().selected_scope = vec![0, 0];
    harness.run_steps(4);
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::P,
    );
    harness.run_steps(4);
    harness
        .state_mut()
        .preview_draft
        .as_mut()
        .expect("actual Preview shortcut")
        .input_text = INPUT.into();
    harness.run_steps(3);
    harness.get_by_label("Preview").click();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while harness.state().run_report.is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "actual Preview button completes: {}",
            harness.state().status
        );
        harness.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(harness.state().show_run_report);
    assert_eq!(
        harness.state().run_report.as_ref().unwrap().report.kind,
        crate::run_report::RunReportKind::Preview
    );
    Ok(())
}
