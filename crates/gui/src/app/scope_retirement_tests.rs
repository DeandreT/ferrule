use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FormatOptions, NamedTarget,
    SequenceExpr,
};

fn sequence_scope(item: NodeId) -> Scope {
    let mut scope = Scope {
        target_field: "Rows".into(),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    };
    scope.set_sequence(Some(SequenceExpr::Generate {
        from: None,
        to: 1,
        item,
    }));
    scope
}

fn app_for(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Input", vec![]);
    app.project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    app.project.graph.nodes.clear();
    app.project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(3),
        },
    );
    for item in [10, 20] {
        app.project.graph.nodes.insert(
            item,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
    }
    app.project.graph.nodes.insert(
        30,
        Node::Const {
            value: Value::String("keep".into()),
        },
    );
    app.project.root = Scope {
        children: vec![sequence_scope(10)],
        ..Scope::default()
    };
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.extra_targets = vec![NamedTarget {
        name: "Other".into(),
        path: Some("other.json".into()),
        schema: app.project.target.clone(),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        root: Scope {
            children: vec![sequence_scope(20)],
            ..Scope::default()
        },
    }];
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.open_target_tab(0);
    app.mapping_workspace.active = document;
    app.mapping_workspace.focused = document;
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn encoded(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).expect("encode project")
}

fn layout(app: &FerruleApp) -> CanvasLayout {
    CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, editing),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn remove_position(shape: &egui::epaint::Shape) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == "Remove scope" => {
            Some(text.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(remove_position),
        _ => None,
    }
}

fn click_remove(app: &mut FerruleApp, editing: bool) {
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut output = frame(app, &context, editing, vec![]);
    for _ in 0..3 {
        output = frame(app, &context, editing, vec![]);
    }
    let position = output
        .shapes
        .iter()
        .find_map(|shape| remove_position(&shape.shape))
        .expect("Remove scope control");
    for pressed in [true, false] {
        frame(
            app,
            &context,
            editing,
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

fn values(output: &Instance) -> Vec<i64> {
    output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .map(|rows| {
            rows.iter()
                .map(
                    |row| match row.field("Value").and_then(Instance::as_scalar) {
                        Some(Value::Int(value)) => *value,
                        other => panic!("integer row: {other:?}"),
                    },
                )
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn scope_retirement_pointer_removal_retires_private_item_on_every_mapping_canvas_and_undoes() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app_for(document);
        let before = encoded(&app);
        let before_layout = layout(&app);
        let item = if document == MappingDocument::Main {
            10
        } else {
            20
        };
        let other = if item == 10 { 20 } else { 10 };
        let source = Instance::Group((vec![]).into());
        let outputs = engine::run_outputs(&app.project, &source).expect("run before removal");
        assert_eq!(values(&outputs.primary), vec![1, 2, 3]);
        assert_eq!(values(&outputs.extras[0].instance), vec![1, 2, 3]);
        click_remove(&mut app, true);
        assert!(!app.project.graph.nodes.contains_key(&item));
        assert!(app.project.graph.nodes.contains_key(&other));
        assert!(app.project.graph.nodes.contains_key(&1));
        assert!(app.project.graph.nodes.contains_key(&30));
        assert!(
            app.main_canvas
                .snarl
                .nodes()
                .all(|node| *node != CanvasNode::Graph(item))
        );
        for canvas in app.mapping_workspace.target_canvases.values() {
            assert!(
                canvas
                    .snarl
                    .nodes()
                    .all(|node| *node != CanvasNode::Graph(item))
            );
        }
        assert!(cli::validate(&app.project).is_empty());
        let outputs = engine::run_outputs(&app.project, &source).expect("run after removal");
        if item == 10 {
            assert!(values(&outputs.primary).is_empty());
            assert_eq!(values(&outputs.extras[0].instance), vec![1, 2, 3]);
        } else {
            assert_eq!(values(&outputs.primary), vec![1, 2, 3]);
            assert!(values(&outputs.extras[0].instance).is_empty());
        }
        let removed = encoded(&app);
        app.undo_project();
        assert_eq!(encoded(&app), before);
        assert_eq!(layout(&app), before_layout);
        app.redo_project();
        assert_eq!(encoded(&app), removed);
    }
}

#[test]
fn scope_retirement_save_reopen_preserves_removal_and_other_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app_for(document);
        click_remove(&mut app, true);
        let path = std::env::temp_dir().join(format!(
            "ferrule-scope-retirement-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let outcome = app.save_document_to(&path)?;
        assert!(outcome.validation_issues.is_empty());
        assert!(outcome.layout_warning.is_none());
        assert!(!app.is_dirty());
        let expected = encoded(&app);
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        assert_eq!(encoded(&reopened), expected);
        assert!(cli::validate(&reopened.project).is_empty());
        let outputs = engine::run_outputs(&reopened.project, &Instance::Group((vec![]).into()))?;
        assert_eq!(
            values(&outputs.primary),
            if document == MappingDocument::Main {
                vec![]
            } else {
                vec![1, 2, 3]
            }
        );
        assert_eq!(
            values(&outputs.extras[0].instance),
            if document == MappingDocument::Main {
                vec![1, 2, 3]
            } else {
                vec![]
            }
        );
        std::fs::remove_file(&path)?;
        let sidecar = crate::layout_store::layout_path(&path);
        if sidecar.exists() {
            std::fs::remove_file(sidecar)?;
        }
    }
    Ok(())
}

#[test]
fn scope_retirement_remaining_graph_target_and_failure_references_block_atomically() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        for case in 0..10 {
            let mut app = app_for(document);
            let item = if document == MappingDocument::Main {
                10
            } else {
                20
            };
            match case {
                0 => {
                    app.project.graph.nodes.insert(
                        40,
                        Node::Call {
                            function: "add".into(),
                            args: vec![item, 1],
                        },
                    );
                }
                1 => {
                    app.project.root.bindings.push(Binding {
                        target_field: "outside".into(),
                        node: item,
                    });
                }
                2 => {
                    app.project.extra_targets[0].root.bindings.push(Binding {
                        target_field: "outside".into(),
                        node: item,
                    });
                }
                3 => {
                    app.project.failure_rules.push(FailureRule {
                        iteration: FailureIteration::Source { collection: vec![] },
                        selection: FailureSelection::All,
                        message: Some(item),
                    });
                }
                4 => {
                    app.project.extra_sources.push(mapping::NamedSource {
                        name: "Dynamic".into(),
                        path: "input.json".into(),
                        schema: app.project.source.clone(),
                        options: FormatOptions::default(),
                        dynamic_path: Some(mapping::DynamicSourcePath {
                            node: item,
                            iteration: vec![],
                        }),
                    });
                }
                5 => {
                    app.project
                        .root
                        .dynamic_children
                        .push(mapping::DynamicChild {
                            key: item,
                            scope: Scope::default(),
                        });
                }
                6 => {
                    app.project.root.children.push(Scope {
                        target_field: "Rows".into(),
                        construction: mapping::ScopeConstruction::RecursiveFilter {
                            plan: mapping::RecursiveFilterPlan::new(
                                "Children".into(),
                                "Items".into(),
                                item,
                            )
                            .expect("valid paths"),
                        },
                        ..Scope::default()
                    });
                }
                7 => {
                    app.project.graph.nodes.insert(
                        40,
                        Node::SequenceExists {
                            sequence: SequenceExpr::Generate {
                                from: None,
                                to: 1,
                                item,
                            },
                            predicate: 1,
                        },
                    );
                }
                8 => {
                    let root = if document == MappingDocument::Main {
                        &mut app.project.root
                    } else {
                        &mut app.project.extra_targets[0].root
                    };
                    // Exact index identity must retain a same-named sibling's ownership.
                    root.children.push(sequence_scope(item));
                }
                9 => {
                    app.project.failure_rules.push(FailureRule {
                        iteration: FailureIteration::Sequence {
                            sequence: SequenceExpr::Generate {
                                from: None,
                                to: 1,
                                item,
                            },
                        },
                        selection: FailureSelection::All,
                        message: None,
                    });
                }
                _ => unreachable!(),
            }
            app.main_canvas = CanvasDocumentState::main(&app.project);
            app.mapping_workspace.target_canvases.insert(
                0,
                CanvasDocumentState::with_snarl(canvas_build::build_named_target_snarl(
                    &app.project,
                    0,
                )),
            );
            app.mark_clean();
            app.rebase_history();
            let before = encoded(&app);
            let before_layout = layout(&app);
            click_remove(&mut app, true);
            assert_eq!(encoded(&app), before, "case {case}");
            assert_eq!(layout(&app), before_layout, "case {case}");
            assert!(!app.can_undo());
            assert!(!app.is_dirty());
            assert!(
                app.diagnostics
                    .items()
                    .iter()
                    .any(|issue| issue.message.contains(&format!("Generated item #{item}")))
            );
        }
    }
}

#[test]
fn scope_retirement_locked_control_and_root_removal_preserve_project_and_history() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app_for(document);
        let before = encoded(&app);
        let before_layout = layout(&app);
        click_remove(&mut app, false);
        assert_eq!(encoded(&app), before);
        assert_eq!(layout(&app), before_layout);
        assert!(!app.can_undo());
        app.selected_scope.clear();
        app.remove_selected_target_scope();
        assert_eq!(encoded(&app), before);
        assert_eq!(layout(&app), before_layout);
    }
}

#[test]
fn scope_retirement_retires_nested_dynamic_and_concatenated_items_without_removing_arguments() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app_for(document);
        let root = if document == MappingDocument::Main {
            &mut app.project.root
        } else {
            &mut app.project.extra_targets[0].root
        };
        root.children[0].children.push(Scope {
            target_field: "Nested".into(),
            iteration: mapping::ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                sequence_scope(11),
                vec![sequence_scope(12)],
            )),
            ..Scope::default()
        });
        root.children[0]
            .dynamic_children
            .push(mapping::DynamicChild {
                key: 30,
                scope: sequence_scope(13),
            });
        for item in [11, 12, 13] {
            app.project.graph.nodes.insert(
                item,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
        }
        let before = encoded(&app);
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.mark_clean();
        app.rebase_history();
        app.remove_selected_target_scope();
        app.observe_editor_history(std::time::Instant::now(), false);
        for item in [
            11,
            12,
            13,
            if document == MappingDocument::Main {
                10
            } else {
                20
            },
        ] {
            assert!(!app.project.graph.nodes.contains_key(&item));
        }
        assert!(app.project.graph.nodes.contains_key(&1));
        assert!(app.project.graph.nodes.contains_key(&30));
        assert!(cli::validate(&app.project).is_empty());
        app.undo_project();
        assert_eq!(encoded(&app), before);
    }
}

#[test]
fn scope_retirement_invalid_item_node_kind_is_not_treated_as_an_orphan() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app_for(document);
        let item = if document == MappingDocument::Main {
            10
        } else {
            20
        };
        app.project.graph.nodes.insert(
            item,
            Node::SequenceExists {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 1,
                    item,
                },
                predicate: 1,
            },
        );
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.mark_clean();
        app.rebase_history();
        let before = encoded(&app);
        let before_layout = layout(&app);
        app.remove_selected_target_scope();
        app.observe_editor_history(std::time::Instant::now(), false);
        assert_eq!(encoded(&app), before);
        assert_eq!(layout(&app), before_layout);
        assert!(!app.can_undo());
        assert!(!app.is_dirty());
        assert!(
            app.diagnostics
                .items()
                .iter()
                .any(|issue| issue.message.contains("invalid node kind"))
        );
    }
}
