use super::*;

fn prepared() -> FerruleApp {
    let mut app = fixture();
    for graph in [
        &mut app.project.graph,
        &mut app.project.user_functions.get_mut(&FUNCTION).unwrap().body,
    ] {
        graph.nodes.insert(
            0,
            Node::Const {
                value: decimal("7.25"),
            },
        );
    }
    app.rebase_history();
    app
}

fn activate(harness: &mut Harness<'_, Editor>, document: MappingDocument) {
    harness.state_mut().app.mapping_workspace.active = document;
    harness.run();
    if harness.query_by_label("Exact decimal value").is_none() {
        open_constant(harness);
    }
}

#[test]
fn graph_decimal_draft_survives_source_add_remove_typed_and_snapshot_history() {
    for snapshots in [false, true] {
        for document in DOCUMENTS {
            let mut app = prepared();
            app.snapshot_oracle = snapshots;
            let mut harness = editor(app);
            activate(&mut harness, document);
            replace(&mut harness, "Exact decimal value", "-");
            click(&mut harness, "Apply");
            let baseline = constant(&harness.state().app, document);
            escape(&mut harness);
            let app = &mut harness.state_mut().app;
            let mut source = ExtraSourceDraft::default();
            source.name = "unrelated".into();
            source.instance_path = "unrelated.json".into();
            source.schema = Some(SchemaNode::group("Lookup", Vec::new()));
            source.options.json_document = true;
            app.extra_source_draft = Some(source);
            app.finish_extra_source();
            harness.run();
            for step in 0..6 {
                match step {
                    1 => harness.state_mut().app.remove_project_extra_source(0),
                    2 | 3 => harness.state_mut().app.undo_project(),
                    4 | 5 => harness.state_mut().app.redo_project(),
                    _ => {}
                }
                activate(&mut harness, document);
                assert_eq!(
                    draft(&harness),
                    "-",
                    "{document:?}, snapshot={snapshots}, step={step}"
                );
                assert_eq!(constant(&harness.state().app, document), baseline);
                harness.get_by_label(&ir::DecimalError::InvalidLexical.to_string());
                escape(&mut harness);
            }
        }
    }
}

#[test]
fn graph_decimal_named_reorder_and_same_name_recreation_have_distinct_ownership() {
    let mut app = prepared();
    let mut second = app.project.extra_targets[0].clone();
    second.name = "receipt".into();
    app.project.extra_targets.push(second);
    app.open_target_tab(1);
    app.ensure_target_canvas(1);
    let mut harness = editor(app);
    activate(&mut harness, MappingDocument::Target(0));
    replace(&mut harness, "Exact decimal value", "audit-draft");
    escape(&mut harness);
    activate(&mut harness, MappingDocument::Target(1));
    replace(&mut harness, "Exact decimal value", "receipt-draft");
    escape(&mut harness);
    harness.state_mut().app.project.extra_targets.swap(0, 1);
    for (index, expected) in [(1, "audit-draft"), (0, "receipt-draft")] {
        activate(&mut harness, MappingDocument::Target(index));
        assert_eq!(draft(&harness), expected);
        escape(&mut harness);
    }
    let replacement = harness.state().app.project.extra_targets[1].clone();
    harness.state_mut().app.remove_extra_target_now(1);
    harness
        .state_mut()
        .app
        .project
        .extra_targets
        .push(replacement);
    harness.state_mut().app.open_target_tab(1);
    activate(&mut harness, MappingDocument::Target(1));
    assert_eq!(draft(&harness), "7.25");
    escape(&mut harness);
    activate(&mut harness, MappingDocument::Target(0));
    assert_eq!(draft(&harness), "receipt-draft");
}

#[test]
fn graph_decimal_real_stage_sessions_do_not_inherit_parent_or_previous_stage_drafts() {
    let directory = Directory::new();
    let mut parent = prepared();
    let mut pipeline =
        crate::pipeline_edit::PipelineEditorDocument::create(&directory.0.join("pipeline.json"))
            .unwrap();
    pipeline.pipeline.stages = ["first", "second"]
        .into_iter()
        .map(|name| mapping::PipelineStage {
            id: name.into(),
            mapping_path: None,
            project: parent.project.clone(),
            source: mapping::PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: Vec::new(),
        })
        .collect();
    parent.pipeline_editor = Some(pipeline_editor_ui::PipelineEditorUi::new(pipeline));
    let mut harness = editor(parent);
    activate(&mut harness, MappingDocument::Main);
    replace(&mut harness, "Exact decimal value", "parent-draft");
    escape(&mut harness);
    let mut container_bound = None;
    for index in [0, 1, 0] {
        let parent = &mut harness.state_mut().app;
        let mut pipeline = parent.pipeline_editor.take().unwrap().document;
        pipeline
            .pipeline
            .stages
            .sort_by_key(|stage| stage.id.clone());
        pipeline.pipeline.stages.swap(0, index);
        parent.pipeline_editor = Some(pipeline_editor_ui::PipelineEditorUi::new(pipeline));
        parent.begin_pipeline_stage_canvas(0);
        let stage = *parent.pipeline_stage_canvas.take().unwrap().editor;
        let parent = std::mem::replace(&mut harness.state_mut().app, stage);
        activate(&mut harness, MappingDocument::Main);
        assert_eq!(draft(&harness), "7.25");
        replace(&mut harness, "Exact decimal value", "stage-draft");
        escape(&mut harness);
        harness.state_mut().app = parent;
        activate(&mut harness, MappingDocument::Main);
        assert_eq!(draft(&harness), "parent-draft");
        escape(&mut harness);
        let counts = crate::graph_draft_ui::container_counts(&harness.ctx);
        if let Some(bound) = container_bound {
            assert_eq!(counts, bound);
        } else {
            container_bound = Some(counts);
        }
    }
}

#[test]
fn graph_decimal_changed_values_function_replacement_and_document_replacement_reset_drafts() {
    for document in DOCUMENTS {
        let mut harness = editor(prepared());
        activate(&mut harness, document);
        replace(&mut harness, "Exact decimal value", "stale");
        escape(&mut harness);
        let app = &mut harness.state_mut().app;
        let graph = match document {
            MappingDocument::Function(id) => {
                &mut app.project.user_functions.get_mut(&id).unwrap().body
            }
            _ => &mut app.project.graph,
        };
        graph.nodes.insert(
            0,
            Node::Const {
                value: decimal("8.5"),
            },
        );
        activate(&mut harness, document);
        assert_eq!(draft(&harness), "8.5");
        replace(&mut harness, "Exact decimal value", "retired");
        escape(&mut harness);
    }
    let mut harness = editor(prepared());
    for document in [MappingDocument::Main, MappingDocument::Function(FUNCTION)] {
        activate(&mut harness, document);
        replace(&mut harness, "Exact decimal value", "retired");
        escape(&mut harness);
    }
    let app = &mut harness.state_mut().app;
    let before = app.project.clone();
    app.project.user_functions.remove(&FUNCTION);
    app.reconcile_graph_drafts();
    let replacement = before.user_functions[&FUNCTION].clone();
    app.project.user_functions.insert(FUNCTION, replacement);
    activate(&mut harness, MappingDocument::Function(FUNCTION));
    assert_eq!(draft(&harness), "7.25");
    escape(&mut harness);
    activate(&mut harness, MappingDocument::Main);
    assert_eq!(draft(&harness), "retired");
    escape(&mut harness);
    harness.state_mut().app.document_ui_id = new_document_ui_id();
    activate(&mut harness, MappingDocument::Main);
    assert_eq!(draft(&harness), "7.25");
}

#[test]
fn graph_decimal_equal_node_recreation_does_not_restore_draft_or_text_undo() {
    fn graph(app: &mut FerruleApp, document: MappingDocument) -> &mut Graph {
        match document {
            MappingDocument::Function(id) => {
                &mut app.project.user_functions.get_mut(&id).unwrap().body
            }
            _ => &mut app.project.graph,
        }
    }
    for document in DOCUMENTS {
        let mut harness = editor(prepared());
        activate(&mut harness, document);
        replace(&mut harness, "Exact decimal value", "deleted-draft");
        escape(&mut harness);
        let app = &mut harness.state_mut().app;
        let before = graph(app, document).clone();
        let node = graph(app, document).nodes.remove(&0).unwrap();
        app.queue_node_edit(
            document,
            &before,
            editor_ui::GraphAction::RemoveNodes {
                requested: [0].into(),
            },
        );
        let before = graph(app, document).clone();
        graph(app, document).nodes.insert(0, node);
        app.queue_node_edit(
            document,
            &before,
            editor_ui::GraphAction::InsertNode {
                node: 0,
                position: editor_ui::CanvasPosition { x: 230.0, y: 40.0 },
            },
        );
        activate(&mut harness, document);
        assert_eq!(draft(&harness), "7.25");
        replace(&mut harness, "Exact decimal value", "new-draft");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_ne!(draft(&harness), "deleted-draft");
    }
}

#[test]
fn graph_decimal_target_output_control_and_history_keep_draft() {
    let mut harness = editor(prepared());
    activate(&mut harness, MappingDocument::Target(0));
    replace(&mut harness, "Exact decimal value", "target-draft");
    escape(&mut harness);
    let app = &mut harness.state_mut().app;
    app.extra_target_draft = Some(crate::extra_targets::ExtraTargetDraft::from_target(
        0,
        &app.project.extra_targets[0],
    ));
    harness.run();
    replace(&mut harness, "Target output file", "changed.json");
    click(&mut harness, "Save target");
    for step in 0..3 {
        if step == 1 {
            harness.state_mut().app.undo_project();
        }
        if step == 2 {
            harness.state_mut().app.redo_project();
        }
        activate(&mut harness, MappingDocument::Target(0));
        assert_eq!(draft(&harness), "target-draft");
        escape(&mut harness);
    }
}

#[test]
fn graph_decimal_repeated_history_bounds_state_and_keeps_function_draft() {
    let mut harness = editor(prepared());
    activate(&mut harness, MappingDocument::Function(FUNCTION));
    replace(
        &mut harness,
        "Exact decimal value",
        "unrelated-function-draft",
    );
    escape(&mut harness);
    activate(&mut harness, MappingDocument::Main);
    replace(&mut harness, "Exact decimal value", "8.5");
    click(&mut harness, "Apply");
    let baseline = crate::value_editor::graph_draft_counts(&harness.ctx);
    let registered = crate::graph_draft_ui::retained_counts(&harness.ctx);
    escape(&mut harness);
    for _ in 0..16 {
        harness.state_mut().app.undo_project();
        activate(&mut harness, MappingDocument::Main);
        assert_eq!(draft(&harness), "7.25");
        replace(&mut harness, "Exact decimal value", "discard on redo");
        escape(&mut harness);
        harness.state_mut().app.redo_project();
        activate(&mut harness, MappingDocument::Main);
        assert_eq!(draft(&harness), "8.5");
        assert_eq!(
            crate::value_editor::graph_draft_counts(&harness.ctx),
            baseline
        );
        assert_eq!(
            crate::graph_draft_ui::retained_counts(&harness.ctx),
            registered
        );
        escape(&mut harness);
        activate(&mut harness, MappingDocument::Function(FUNCTION));
        assert_eq!(draft(&harness), "unrelated-function-draft");
        escape(&mut harness);
    }
}
