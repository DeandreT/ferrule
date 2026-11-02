use super::*;

fn text(output: &egui::FullOutput, label: &str) -> String {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::TextInput
                && node.label() == Some(&format!("{label} value")))
            .then(|| node.value().unwrap_or_default().to_owned())
        })
        .unwrap()
}

#[test]
fn graph_draft_survives_source_refresh_and_history() {
    let mut retained = Retained::new();
    for snapshots in [false, true] {
        for document in DOCUMENTS {
            let (mut app, ctx) = setup(
                &retained,
                Value::Int(7),
                vec![(Value::Int(7), Value::Int(9))],
                Some(Value::Int(11)),
                ScalarType::Int,
            );
            app.snapshot_oracle = snapshots;
            app.mapping_workspace.active = document;
            open(&mut app, &ctx, &retained);
            for label in ["Entry 1 key", "Entry 1 value", "Default"] {
                enter_text(&mut app, &ctx, &retained, label, "123");
            }
            close(&mut app, &ctx, &retained);
            let mut source = ExtraSourceDraft::default();
            source.name = "unrelated".into();
            source.instance_path = "unrelated.json".into();
            source.schema = Some(SchemaNode::group("Lookup", Vec::new()));
            source.options.json_document = true;
            app.extra_source_draft = Some(source);
            app.finish_extra_source();
            settle(&mut app, &ctx, &retained, true);
            for action in 0..6 {
                match action {
                    1 => app.remove_project_extra_source(0),
                    2 | 3 => app.undo_project(),
                    4 | 5 => app.redo_project(),
                    _ => {}
                }
                app.mapping_workspace.active = document;
                open(&mut app, &ctx, &retained);
                let output = settle(&mut app, &ctx, &retained, true);
                for label in ["Entry 1 key", "Entry 1 value", "Default"] {
                    assert_eq!(
                        text(&output, label),
                        "123",
                        "{document:?}, snapshot={snapshots}, step {action}, {label}"
                    );
                }
                assert_eq!(
                    cells(&app, document),
                    (vec![(Value::Int(7), Value::Int(9))], Some(Value::Int(11)))
                );
                close(&mut app, &ctx, &retained);
            }
        }
    }
    retained.complete = true;
}

#[test]
fn graph_draft_same_id_replacement_starts_fresh() {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let (mut app, ctx) = setup(
            &retained,
            Value::Int(7),
            vec![(Value::String(String::new()), Value::String(String::new()))],
            None,
            ScalarType::String,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &ctx, &retained);
        choose_kind(&mut app, &ctx, &retained, "Entry 1 key", "int");
        enter_text(&mut app, &ctx, &retained, "Entry 1 key", "123");
        close(&mut app, &ctx, &retained);
        // Two committed mutations before another redraw: an equal replacement must
        // not resurrect the deleted node's private draft or TextEdit state.
        let before = selected_graph(&app, document).clone();
        let node = selected_graph_mut(&mut app, document)
            .nodes
            .remove(&1)
            .unwrap();
        app.queue_node_edit(
            document,
            &before,
            editor_ui::GraphAction::RemoveNodes {
                requested: [1].into(),
            },
        );
        let before = selected_graph(&app, document).clone();
        selected_graph_mut(&mut app, document).nodes.insert(1, node);
        app.queue_node_edit(
            document,
            &before,
            editor_ui::GraphAction::InsertNode {
                node: 1,
                position: editor_ui::CanvasPosition { x: 350.0, y: 0.0 },
            },
        );
        open(&mut app, &ctx, &retained);
        let output = settle(&mut app, &ctx, &retained, true);
        assert_eq!(text(&output, "Entry 1 key"), "");
        enter_text(&mut app, &ctx, &retained, "Entry 1 key", "replacement");
        // Deliver TextEdit undo directly: the app's global Command+Z shortcut
        // otherwise takes this event before the focused editor sees it.
        let _ = ctx.run_test_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                time: Some(ctx.cumulative_frame_nr() as f64 / 10.0),
                events: vec![
                    key_event(egui::Key::Z, egui::Modifiers::COMMAND, true),
                    key_event(egui::Key::Z, egui::Modifiers::COMMAND, false),
                ],
                ..Default::default()
            },
            |ui| app.show_mapping_workspace_canvas(ui, true),
        );
        let output = settle(&mut app, &ctx, &retained, true);
        assert_ne!(text(&output, "Entry 1 key"), "123");
    }
    retained.complete = true;
}

#[test]
fn graph_draft_changed_value_resets_only_its_own_cell() {
    let mut retained = Retained::new();
    for document in DOCUMENTS {
        let (mut app, ctx) = setup(
            &retained,
            Value::Int(7),
            vec![(Value::Int(7), Value::Int(9))],
            Some(Value::Int(11)),
            ScalarType::Int,
        );
        app.mapping_workspace.active = document;
        open(&mut app, &ctx, &retained);
        for label in ["Entry 1 key", "Entry 1 value", "Default"] {
            enter_text(&mut app, &ctx, &retained, label, "123");
        }
        close(&mut app, &ctx, &retained);
        let Node::ValueMap { table, .. } = selected_graph_mut(&mut app, document)
            .nodes
            .get_mut(&1)
            .unwrap()
        else {
            panic!("ValueMap")
        };
        table[0].0 = Value::Int(8);
        open(&mut app, &ctx, &retained);
        let output = settle(&mut app, &ctx, &retained, true);
        assert_eq!(text(&output, "Entry 1 key"), "8");
        for label in ["Entry 1 value", "Default"] {
            assert_eq!(text(&output, label), "123");
        }
    }
    retained.complete = true;
}

#[test]
fn graph_draft_target_output_control_and_history_keep_cells() {
    let mut retained = Retained::new();
    let (mut app, ctx) = setup(
        &retained,
        Value::Int(7),
        vec![(Value::Int(7), Value::Int(9))],
        Some(Value::Int(11)),
        ScalarType::Int,
    );
    app.mapping_workspace.active = MappingDocument::Target(0);
    open(&mut app, &ctx, &retained);
    for label in ["Entry 1 key", "Entry 1 value", "Default"] {
        enter_text(&mut app, &ctx, &retained, label, "123");
    }
    close(&mut app, &ctx, &retained);
    app.extra_target_draft = Some(crate::extra_targets::ExtraTargetDraft::from_target(
        0,
        &app.project.extra_targets[0],
    ));
    let output = settle(&mut app, &ctx, &retained, true);
    let point = control(
        &output,
        egui::accesskit::Role::TextInput,
        "Target output file",
        true,
    );
    click(&mut app, &ctx, &retained, true, point);
    frame(
        &mut app,
        &ctx,
        &retained,
        true,
        vec![
            key_event(egui::Key::A, egui::Modifiers::COMMAND, true),
            key_event(egui::Key::A, egui::Modifiers::COMMAND, false),
        ],
    );
    frame(
        &mut app,
        &ctx,
        &retained,
        true,
        vec![egui::Event::Text("changed.json".into())],
    );
    let output = settle(&mut app, &ctx, &retained, true);
    let point = control(&output, egui::accesskit::Role::Button, "Save target", true);
    click(&mut app, &ctx, &retained, true, point);
    for step in 0..3 {
        if step == 1 {
            app.undo_project();
        }
        if step == 2 {
            app.redo_project();
        }
        app.mapping_workspace.active = MappingDocument::Target(0);
        open(&mut app, &ctx, &retained);
        let output = settle(&mut app, &ctx, &retained, true);
        for label in ["Entry 1 key", "Entry 1 value", "Default"] {
            assert_eq!(text(&output, label), "123");
        }
        close(&mut app, &ctx, &retained);
    }
    retained.complete = true;
}

#[test]
fn graph_draft_repeated_cell_changes_and_actual_type_menus_keep_state_bounded() {
    let mut retained = Retained::new();
    let (mut app, ctx) = setup(
        &retained,
        Value::Int(7),
        vec![(Value::Int(7), Value::Int(9))],
        Some(Value::Int(11)),
        ScalarType::Int,
    );
    app.mapping_workspace.active = MappingDocument::Main;
    open(&mut app, &ctx, &retained);
    choose_kind(&mut app, &ctx, &retained, "Entry 1 key", "int");
    let baseline = crate::value_editor::graph_draft_counts(&ctx);
    let registrations = crate::graph_draft_ui::retained_counts(&ctx);
    let menus = ctx.data(|data| data.count::<egui::containers::menu::MenuState>());
    let containers = crate::graph_draft_ui::container_counts(&ctx);
    assert!(containers.0 > 0 && containers.1 > 0);
    for index in 0..24 {
        let Node::ValueMap { table, .. } = app.project.graph.nodes.get_mut(&1).unwrap() else {
            unreachable!()
        };
        table[0].0 = Value::Int(index);
        settle(&mut app, &ctx, &retained, true);
        choose_kind(&mut app, &ctx, &retained, "Entry 1 key", "int");
        assert_eq!(crate::value_editor::graph_draft_counts(&ctx), baseline);
        assert_eq!(crate::graph_draft_ui::retained_counts(&ctx), registrations);
        assert_eq!(
            ctx.data(|data| data.count::<egui::containers::menu::MenuState>()),
            menus
        );
        assert_eq!(crate::graph_draft_ui::container_counts(&ctx), containers);
    }
    retained.complete = true;
}
