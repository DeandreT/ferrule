use super::*;
use crate::target_xml_type::{
    Action,
    tests::{BASE, DERIVED, assert_outputs, marker, project},
};

fn app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp {
        project: project(),
        ..Default::default()
    };
    let hints = std::env::temp_dir().join("ferrule-target-type-instance-hints");
    app.project.source_path = Some(hints.join("input.json").to_string_lossy().into_owned());
    app.project.target_path = Some(hints.join("output.xml").to_string_lossy().into_owned());
    app.project.extra_targets[0].path =
        Some(hints.join("other.xml").to_string_lossy().into_owned());
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
        // Opening a tab is lazy; the ordinary renderer creates its canvas.
        assert!(app.ensure_target_canvas(index));
    }
    app.selected_scope = if document == MappingDocument::Main {
        vec![0]
    } else {
        vec![]
    };
    app.mark_clean();
    app.rebase_history();
    app
}

fn active_scope(app: &FerruleApp) -> &Scope {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root.children[0],
    }
}

fn snarl(app: &FerruleApp) -> &egui_snarl::Snarl<CanvasNode> {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.mapping_workspace.target_canvases[&index].snarl,
        _ => &app.main_canvas.snarl,
    }
}

fn assert_type_wire(app: &FerruleApp) {
    let schema = match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].schema,
        _ => &app.project.target,
    };
    let blocks = crate::canvas::target_blocks(schema);
    let expression = marker(active_scope(app));
    let wires: Vec<_> = snarl(app).wires().filter(|(from, to)| {
        matches!(snarl(app).get_node(from.node), Some(CanvasNode::Graph(id)) if *id == expression)
            && matches!(snarl(app).get_node(to.node), Some(CanvasNode::TargetBlock(block))
                if blocks[*block].leaves[to.input].field == ir::XML_TYPE_FIELD)
    }).collect();
    assert_eq!(wires.len(), 1, "one visible XML type binding");
}

fn state(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).unwrap()
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 2000.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, enabled),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn positions(shape: &egui::epaint::Shape, label: &str, found: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            found.push(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                positions(shape, label, found);
            }
        }
        _ => {}
    }
}

fn click(app: &mut FerruleApp, context: &egui::Context, enabled: bool, label: &str, last: bool) {
    let mut output = frame(app, context, enabled, vec![]);
    for _ in 0..3 {
        output = frame(app, context, enabled, vec![]);
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        positions(&shape.shape, label, &mut found);
    }
    let point = if last { found.last() } else { found.first() }
        .copied()
        .unwrap_or_else(|| panic!("rendered XML type control: {label}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            enabled,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn choose(app: &mut FerruleApp, context: &egui::Context, selected: &str, desired: &str) {
    click(app, context, true, selected, false);
    click(app, context, true, desired, true);
}

#[test]
fn declared_type_pointer_selection_preserves_other_owners_wires_and_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app(document);
        let canvas = match document {
            MappingDocument::Target(index) => app
                .mapping_workspace
                .target_canvases
                .get_mut(&index)
                .unwrap(),
            _ => &mut app.main_canvas,
        };
        let old = canvas
            .snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(0)).then_some(id))
            .unwrap();
        canvas.snarl.get_node_info_mut(old).unwrap().pos = egui::pos2(321.0, 456.0);
        canvas.viewport_width = 987.0;
        canvas.view_generation = 19;
        app.mark_clean();
        app.rebase_history();
        assert_type_wire(&app);
        assert!(snarl(&app).nodes_pos().any(|(pos, node)| *node == CanvasNode::Graph(0) && pos == egui::pos2(321.0, 456.0)));
        assert_eq!(
            app.selected_scope,
            if document == MappingDocument::Main {
                vec![0]
            } else {
                vec![]
            }
        );
        if let MappingDocument::Target(index) = document {
            assert_eq!(
                app.mapping_workspace.target_canvases[&index].viewport_width,
                987.0
            );
            assert_eq!(
                app.mapping_workspace.target_canvases[&index].view_generation,
                19
            );
        }
        let original = state(&app);
        let context = egui::Context::default();
        crate::icons::install(&context);
        for _ in 0..4 {
            let _ = frame(&mut app, &context, true, vec![]);
        }
        assert_eq!(
            state(&app),
            original,
            "opening the chooser does not normalize imported bindings"
        );
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        choose(&mut app, &context, BASE, DERIVED);
        assert_type_wire(&app);
        assert!(snarl(&app).nodes_pos().any(|(pos, node)| *node == CanvasNode::Graph(0) && pos == egui::pos2(321.0, 456.0)));
        assert_eq!(
            app.selected_scope,
            if document == MappingDocument::Main {
                vec![0]
            } else {
                vec![]
            }
        );
        if let MappingDocument::Target(index) = document {
            assert_eq!(
                app.mapping_workspace.target_canvases[&index].viewport_width,
                987.0
            );
            assert_eq!(
                app.mapping_workspace.target_canvases[&index].view_generation,
                19
            );
        }
        assert!(
            matches!(app.project.graph.nodes.get(&0), Some(Node::Const { value: ir::Value::String(value) }) if value == BASE)
        );
        assert_eq!(app.project.root.bindings[0].node, 0);
        assert!(app.is_dirty());
        assert!(app.can_undo());
        let changed = state(&app);
        if document == MappingDocument::Main {
            assert_outputs(&app.project, DERIVED, BASE);
        } else {
            assert_outputs(&app.project, BASE, DERIVED);
        }
        app.undo_project();
        assert_eq!(state(&app), original);
        assert!(!app.is_dirty());
        assert_type_wire(&app);
        app.redo_project();
        assert_eq!(state(&app), changed);
        assert_type_wire(&app);
    }
}

#[test]
fn target_type_selection_save_reopen_preserves_exact_identity_and_named_canvas()
-> anyhow::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app(document);
        app.apply_selected_target_xml_type(&Action::Declared(DERIVED.into()));
        app.observe_editor_history(std::time::Instant::now(), false);
        let directory = std::env::temp_dir().join(format!(
            "ferrule-target-type-save-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("project.json");
        let saved = app.save_document_to(&path)?;
        assert!(saved.validation_issues.is_empty());
        assert!(saved.layout_warning.is_none());
        let expected = state(&app);
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        if let MappingDocument::Target(index) = document {
            reopened.open_target_tab(index);
        }
        reopened.selected_scope = if document == MappingDocument::Main {
            vec![0]
        } else {
            vec![]
        };
        assert_eq!(state(&reopened), expected);
        assert!(!reopened.is_dirty());
        assert!(!reopened.can_undo());
        assert_type_wire(&reopened);
        if document == MappingDocument::Main {
            assert_outputs(&reopened.project, DERIVED, BASE);
        } else {
            assert_outputs(&reopened.project, BASE, DERIVED);
        }
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[test]
fn unavailable_imported_type_binding_stays_visible_and_removal_can_be_undone() {
    let mut app = app(MappingDocument::Target(0));
    // Declared type choices are unavailable, but this schema remains
    // persistable so the real editor history/codec path can be exercised.
    app.project.extra_targets[0].schema.xml_type_alternatives = false;
    app.project.extra_targets[0].schema.xml_default_type = None;
    assert!(app.project.extra_targets[0].schema.metadata_is_valid());
    app.project.extra_targets[0].root.bindings[1].node = 41;
    app.mapping_workspace.target_canvases.insert(
        0,
        CanvasDocumentState::with_snarl(canvas_build::build_named_target_snarl(&app.project, 0)),
    );
    app.mark_clean();
    app.rebase_history();
    let original = state(&app);
    let context = egui::Context::default();
    crate::icons::install(&context);
    let output = frame(&mut app, &context, true, vec![]);
    let mut labels = Vec::new();
    for shape in &output.shapes {
        positions(&shape.shape, "Missing expression", &mut labels);
    }
    assert!(!labels.is_empty(), "invalid imported binding is visible");
    assert_eq!(state(&app), original);
    assert!(!app.can_undo());
    choose(&mut app, &context, "Missing expression", "Schema inference");
    assert_eq!(active_scope(&app).bindings.len(), 1);
    assert!(!app.project.graph.nodes.contains_key(&41));
    assert_eq!(marker(&app.project.root.children[0]), 0);
    app.undo_project();
    assert_eq!(state(&app), original);
}

#[test]
fn locked_type_control_does_not_change_project_or_history() {
    let mut app = app(MappingDocument::Target(0));
    let original = state(&app);
    let context = egui::Context::default();
    crate::icons::install(&context);
    click(&mut app, &context, false, BASE, false);
    assert_eq!(state(&app), original);
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
    assert!(app.pending_history.is_none());
}

fn document_canvas(app: &FerruleApp, document: MappingDocument) -> &CanvasDocumentState {
    match document {
        MappingDocument::Main => &app.main_canvas,
        MappingDocument::Target(index) => &app.mapping_workspace.target_canvases[&index],
        MappingDocument::Function(_) => panic!("target type fixture has no function canvas"),
    }
}

fn assert_retained_positions(canvas: &CanvasDocumentState, previous: &[CanvasNodeLayout]) {
    assert!(!previous.is_empty());
    let current = CanvasLayout::capture_nodes(&canvas.snarl);
    for node in previous {
        assert_eq!(
            current.iter().find(|entry| entry.node == node.node),
            Some(node)
        );
    }
}

#[test]
fn new_type_constants_clear_retained_nodes_in_primary_and_named_canvases() -> anyhow::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for offset in [egui::Vec2::ZERO, egui::vec2(37.0, 19.0)] {
            let mut app = app(document);
            app.project.graph.nodes.insert(
                2,
                Node::Const {
                    value: ir::Value::String(DERIVED.into()),
                },
            );
            app.main_canvas = CanvasDocumentState::main(&app.project);
            app.mapping_workspace.target_canvases.insert(
                0,
                CanvasDocumentState::with_snarl(canvas_build::build_named_target_snarl(
                    &app.project,
                    0,
                )),
            );
            for canvas in [
                &mut app.main_canvas,
                app.mapping_workspace.target_canvases.get_mut(&0).unwrap(),
            ] {
                for (id, position) in [
                    (0, egui::pos2(267.0, 112.0)),
                    (1, egui::pos2(267.0, 0.0)),
                    (2, egui::pos2(267.0, 224.0)),
                ] {
                    let snarl_id = canvas
                        .snarl
                        .node_ids()
                        .find_map(|(snarl_id, node)| {
                            (*node == CanvasNode::Graph(id)).then_some(snarl_id)
                        })
                        .unwrap();
                    canvas.snarl.get_node_info_mut(snarl_id).unwrap().pos = position + offset;
                    canvas
                        .node_sizes
                        .insert(CanvasNode::Graph(id), egui::vec2(235.0, 110.0));
                }
            }
            app.mark_clean();
            app.rebase_history();
            let inactive = if document == MappingDocument::Main {
                MappingDocument::Target(0)
            } else {
                MappingDocument::Main
            };
            let inactive_positions =
                CanvasLayout::capture_nodes(&document_canvas(&app, inactive).snarl);
            let inactive_wires = document_canvas(&app, inactive)
                .snarl
                .wires()
                .collect::<Vec<_>>();
            let context = egui::Context::default();
            crate::icons::install(&context);
            for identity in [DERIVED, BASE] {
                let retained = CanvasLayout::capture_nodes(snarl(&app));
                assert!(
                    retained
                        .iter()
                        .filter(|entry| matches!(entry.node, PersistedCanvasNode::Graph { .. }))
                        .count()
                        >= 3
                );
                app.apply_selected_target_xml_type(&Action::Declared(identity.into()));
                let fresh = marker(active_scope(&app));
                assert!(fresh > 2, "a new constant leaves the shared base untouched");
                assert_retained_positions(document_canvas(&app, document), &retained);
                assert_eq!(
                    CanvasLayout::capture_nodes(&document_canvas(&app, inactive).snarl),
                    inactive_positions
                );
                assert_eq!(
                    document_canvas(&app, inactive)
                        .snarl
                        .wires()
                        .collect::<Vec<_>>(),
                    inactive_wires
                );
                assert_type_wire(&app);
                // Render the real canvas to verify rectangles using actual widget
                // measurements rather than only the placement fallback dimensions.
                let mut output = None;
                for _ in 0..4 {
                    output = Some(context.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1600.0, 1600.0),
                            )),
                            ..Default::default()
                        },
                        |ui| app.show_mapping_workspace_canvas(ui, true),
                    ));
                }
                let canvas = document_canvas(&app, document);
                let position = canvas
                    .snarl
                    .nodes_pos()
                    .find_map(|(position, node)| {
                        (*node == CanvasNode::Graph(fresh)).then_some(position)
                    })
                    .unwrap();
                let size = *canvas
                    .node_sizes
                    .get(&CanvasNode::Graph(fresh))
                    .expect("fresh constant was rendered");
                let rect = egui::Rect::from_min_size(position, size);
                for (other_position, node) in canvas.snarl.nodes_pos() {
                    if *node == CanvasNode::Graph(fresh) {
                        continue;
                    }
                    let size = *canvas
                        .node_sizes
                        .get(node)
                        .expect("retained node was rendered");
                    assert!(
                        !rect.intersects(egui::Rect::from_min_size(other_position, size)),
                        "new constant occludes {node:?}"
                    );
                }
                let mut labels = Vec::new();
                for shape in &output.unwrap().shapes {
                    positions(&shape.shape, &format!("Const: {identity}"), &mut labels);
                }
                assert!(!labels.is_empty(), "new type constant label is visible");
                assert_retained_positions(document_canvas(&app, document), &retained);
                assert_eq!(app.project.root.bindings[0].node, 0);
                assert!(
                    matches!(app.project.graph.nodes.get(&0), Some(Node::Const { value: ir::Value::String(value) }) if value == BASE)
                );
                if document == MappingDocument::Main {
                    assert_outputs(&app.project, identity, BASE);
                } else {
                    assert_outputs(&app.project, BASE, identity);
                }
            }
            let layout =
                CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let directory = std::env::temp_dir().join(format!(
                "ferrule-target-type-placement-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&directory)?;
            let path = directory.join("project.json");
            app.save_document_to(&path)?;
            let mut reopened = FerruleApp::default();
            reopened.load_project_from(&path);
            assert_eq!(state(&reopened), state(&app));
            assert_retained_positions(&reopened.main_canvas, &layout.nodes);
            assert_retained_positions(
                &reopened.mapping_workspace.target_canvases[&0],
                &layout.target_nodes[&0],
            );
            std::fs::remove_dir_all(directory)?;
        }
    }
    Ok(())
}

fn history_key_frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    key: egui::Key,
    shift: bool,
) -> bool {
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        shift,
        ..Default::default()
    };
    let mut consumed = false;
    let _ = context.run_ui(
        egui::RawInput {
            modifiers,
            events: [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .collect(),
            ..Default::default()
        },
        |ui| {
            app.handle_history_shortcuts(ui.ctx(), editing_enabled);
            consumed = !ui.ctx().input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key { key: observed, pressed: true, .. }
                        if *observed == key)
                })
            });
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    consumed
}

#[test]
fn raw_history_keys_redo_before_undo_and_preserve_locked_main_and_named_owners() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app(document);
        let canvas = match document {
            MappingDocument::Target(index) => app
                .mapping_workspace
                .target_canvases
                .get_mut(&index)
                .unwrap(),
            _ => &mut app.main_canvas,
        };
        let original_node = canvas
            .snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(0)).then_some(id))
            .unwrap();
        canvas.snarl.get_node_info_mut(original_node).unwrap().pos = egui::pos2(321.0, 456.0);
        app.mark_clean();
        app.rebase_history();
        let original = state(&app);
        app.apply_selected_target_xml_type(&Action::Declared(DERIVED.into()));
        app.observe_editor_history(std::time::Instant::now(), false);
        let changed = state(&app);
        assert_ne!(changed, original);
        let context = egui::Context::default();

        // Locked editing neither changes history nor consumes the host's key.
        assert!(!history_key_frame(
            &mut app,
            &context,
            false,
            egui::Key::Z,
            false
        ));
        assert_eq!(state(&app), changed);
        assert!(app.can_undo());
        assert!(!app.history.can_redo());
        assert!(history_key_frame(
            &mut app,
            &context,
            true,
            egui::Key::Z,
            false
        ));
        assert_eq!(state(&app), original);
        assert!(!app.is_dirty());
        assert!(app.history.can_redo());
        for (key, shift) in [(egui::Key::Z, true), (egui::Key::Y, false)] {
            assert!(!history_key_frame(&mut app, &context, false, key, shift));
            assert_eq!(state(&app), original);
            assert!(app.history.can_redo());
        }

        // The shifted Z event must redo through the real app history handler.
        // Matching plain undo first consumes this event and leaves Base bound.
        assert!(history_key_frame(
            &mut app,
            &context,
            true,
            egui::Key::Z,
            true
        ));
        assert_eq!(state(&app), changed);
        assert!(app.is_dirty());
        assert!(!app.history.can_redo());
        assert_type_wire(&app);
        assert!(history_key_frame(
            &mut app,
            &context,
            true,
            egui::Key::Z,
            false
        ));
        assert_eq!(state(&app), original);
        assert!(history_key_frame(
            &mut app,
            &context,
            true,
            egui::Key::Y,
            false
        ));
        assert_eq!(state(&app), changed);
        assert_type_wire(&app);
        assert_eq!(app.mapping_workspace.active, document);
        assert_eq!(app.project.extra_targets[0].name, "Other");
        assert_eq!(app.project.root.bindings[0].node, 0);
        assert!(matches!(app.project.graph.nodes.get(&0),
            Some(Node::Const { value: ir::Value::String(value) }) if value == BASE));
        assert!(snarl(&app).nodes_pos().any(|(pos, node)|
            *node == CanvasNode::Graph(0) && pos == egui::pos2(321.0, 456.0)));
        if document == MappingDocument::Main {
            assert_outputs(&app.project, DERIVED, BASE);
        } else {
            assert_outputs(&app.project, BASE, DERIVED);
        }
    }
}
