//! Real Inspector selectors must have the same admission boundary as pin drags.
use super::context_regressions::{connection_fixture, observed, run};
use super::*;

fn app(document: MappingDocument) -> FerruleApp {
    let mut app = connection_fixture("generated");
    app.project.graph.nodes.insert(
        7,
        Node::OwnedLookup(OwnedLookup {
            collection: source(SourceOwner::Primary, "Rows"),
            key: vec!["Code".into()],
            matches: 3,
            value: vec!["PrimaryValue".into()],
        }),
    );
    app.project.graph.nodes.insert(
        12,
        Node::Call {
            function: "concat".into(),
            args: vec![7],
        },
    );
    rebuild(&mut app, document);
    app.selected_scope = vec![1];
    run(&app, INPUT);
    app
}

fn root(app: &FerruleApp) -> &Scope {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    }
}
fn root_mut(app: &mut FerruleApp) -> &mut Scope {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &mut app.project.extra_targets[index].root,
        _ => &mut app.project.root,
    }
}
fn inspector_frame(
    app: &mut FerruleApp,
    ctx: &egui::Context,
    enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let out = ctx.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 2400.0),
            )),
            time: Some(ctx.cumulative_frame_nr() as f64 / 10.0),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, enabled),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    out
}
fn settle_inspector(app: &mut FerruleApp, ctx: &egui::Context, enabled: bool) -> egui::FullOutput {
    let mut out = inspector_frame(app, ctx, enabled, vec![]);
    for _ in 0..4 {
        out = inspector_frame(app, ctx, enabled, vec![]);
    }
    out
}
fn click_label(app: &mut FerruleApp, ctx: &egui::Context, enabled: bool, label: &str, last: bool) {
    let out = settle_inspector(app, ctx, enabled);
    let positions = visible(&out, label);
    let pos = *(if last {
        positions.last()
    } else {
        positions.first()
    })
    .unwrap_or_else(|| panic!("Inspector is missing {label:?}"));
    inspector_frame(app, ctx, enabled, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        inspector_frame(
            app,
            ctx,
            enabled,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}
fn label(app: &FerruleApp, id: NodeId) -> String {
    format!(
        "{} · #{id}",
        crate::graph_viewer::readable_node_label(&app.project.graph.nodes[&id])
    )
}
fn close(app: &mut FerruleApp, ctx: &egui::Context) {
    inspector_frame(
        app,
        ctx,
        true,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    settle_inspector(app, ctx, true);
}

#[test]
fn actual_inspector_binding_picker_refuses_direct_and_indirect_lookup_but_allows_siblings() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = app(document);
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        for candidate in [7, 12] {
            let current = label(&app, 3);
            click_label(&mut app, &ctx, true, &current, true);
            let before = observed(&app);
            let candidate = label(&app, candidate);
            click_label(&mut app, &ctx, true, &candidate, true);
            assert_eq!(observed(&app), before);
            assert_eq!(root(&app).children[1].bindings[0].node, 3);
            run(&app, INPUT);
            close(&mut app, &ctx);
        }
        app.selected_scope = vec![0];
        settle_inspector(&mut app, &ctx, true);
        for candidate in [7, 12] {
            let current = label(&app, root(&app).children[0].bindings[0].node);
            click_label(&mut app, &ctx, true, &current, true);
            let candidate_label = label(&app, candidate);
            click_label(&mut app, &ctx, true, &candidate_label, true);
            assert_eq!(root(&app).children[0].bindings[0].node, candidate);
            let result = run(&app, INPUT);
            let output = if document == MappingDocument::Main {
                &result.primary
            } else {
                &result.extras[0].instance
            };
            assert_eq!(
                output.field("Allowed").unwrap().field("Result").unwrap(),
                &ir::Instance::Scalar(Value::String("primary-first".into()))
            );
            assert!(matches!(
                app.history.next_undo(),
                Some(HistoryRecord::Command(_))
            ));
        }
    }
}

#[test]
fn generated_filter_group_sort_and_window_pickers_share_the_lookup_gate() {
    for control in [
        "filter",
        "group",
        "group-start",
        "blocks",
        "post-group",
        "sort",
        "window",
    ] {
        let mut app = app(MappingDocument::Main);
        let scope = &mut app.project.root.children[1];
        let current = match control {
            "filter" => {
                scope.filter = Some(30);
                30
            }
            "group" => {
                scope.group_by = Some(3);
                3
            }
            "group-start" => {
                scope.group_starting_with = Some(30);
                30
            }
            "blocks" => {
                scope.group_into_blocks = Some(21);
                21
            }
            "post-group" => {
                scope.group_by = Some(3);
                scope.post_group_filter = Some(30);
                30
            }
            "sort" => {
                scope.sort_by = Some(3);
                3
            }
            "window" => {
                scope
                    .windows
                    .push(mapping::SequenceWindow::First { count: 21 });
                21
            }
            _ => unreachable!(),
        };
        run(&app, INPUT);
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        let current_label = label(&app, current);
        // Binding uses #3 too; the controls precede it.
        click_label(&mut app, &ctx, true, &current_label, false);
        let before = observed(&app);
        let candidate = label(&app, 12);
        click_label(&mut app, &ctx, true, &candidate, true);
        assert_eq!(observed(&app), before, "{control}");
        run(&app, INPUT);
    }
}

#[test]
fn computed_property_name_and_value_pickers_refuse_lookup_dependencies() {
    for value in [false, true] {
        let mut app = app(MappingDocument::Main);
        let reference = connection_fixture("dynamic");
        app.project.target = reference.project.target;
        app.project.root = reference.project.root;
        app.project.extra_targets = reference.project.extra_targets;
        rebuild(&mut app, MappingDocument::Main);
        app.selected_scope = vec![1];
        run(&app, INPUT);
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        let current = label(&app, if value { 3 } else { 31 });
        click_label(&mut app, &ctx, true, &current, value);
        let before = observed(&app);
        let candidate = label(&app, 7);
        click_label(&mut app, &ctx, true, &candidate, true);
        assert_eq!(observed(&app), before);
        run(&app, INPUT);
    }
}

#[test]
fn saved_unsupported_scope_lookup_metadata_and_locked_picker_are_unchanged() {
    for saved in [false, true] {
        let mut app = app(MappingDocument::Main);
        if saved {
            root_mut(&mut app).children[1].bindings[0].node = 12;
        } else {
            app.selected_scope = vec![0];
        }
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        let before = observed(&app);
        let id = if saved { 12 } else { 3 };
        let current = label(&app, id);
        click_label(&mut app, &ctx, saved, &current, true);
        assert_eq!(observed(&app), before);
        if saved {
            assert!(
                !visible(
                    &settle_inspector(&mut app, &ctx, true),
                    crate::scope_editor::lookup_admission::READ_ONLY
                )
                .is_empty()
            );
        }
    }
}

#[test]
fn presence_condition_picker_excludes_indirect_lookup_booleans() {
    let mut app = app(MappingDocument::Main);
    let reference = connection_fixture("presence");
    app.project.target = reference.project.target;
    app.project.root = reference.project.root;
    app.project.extra_targets = reference.project.extra_targets;
    app.project.graph.nodes.insert(
        13,
        Node::Call {
            function: "exists".into(),
            args: vec![7],
        },
    );
    rebuild(&mut app, MappingDocument::Main);
    app.selected_scope = vec![1];
    run(&app, INPUT);
    let ctx = context();
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let current = label(&app, 30);
    click_label(&mut app, &ctx, true, &current, true);
    let output = settle_inspector(&mut app, &ctx, true);
    assert!(visible(&output, &label(&app, 7)).is_empty());
    assert!(visible(&output, &label(&app, 13)).is_empty());
    assert!(!app.can_undo());
    run(&app, INPUT);
}

#[test]
fn adding_dynamic_properties_cannot_invalidate_an_existing_ordinary_lookup_binding() {
    let mut app = app(MappingDocument::Main);
    app.project.target = app
        .project
        .target
        .clone()
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .unwrap();
    app.project.root.bindings = vec![mapping::Binding {
        target_field: "Result".into(),
        node: 7,
    }];
    if let ir::SchemaKind::Group { children, .. } = &mut app.project.target.kind {
        children.push(SchemaNode::scalar("Result", ScalarType::String));
    }
    app.project.extra_targets.clear();
    // Keep the fixture's two-canvas observation helper available without adding
    // the edited primary scope to its independent target.
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "Audit".into(),
        path: None,
        schema: SchemaNode::group("Audit", vec![]),
        options: Default::default(),
        root: Default::default(),
    });
    rebuild(&mut app, MappingDocument::Main);
    app.selected_scope = vec![];
    run(&app, INPUT);
    let ctx = context();
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    click_label(&mut app, &ctx, true, "Add property", true);
    assert_eq!(observed(&app), before);
    assert!(app.project.root.dynamic_bindings.is_empty());
    run(&app, INPUT);
}

#[test]
fn unrelated_missing_expression_does_not_block_an_ordinary_inspector_assignment() {
    let mut app = app(MappingDocument::Main);
    app.project.root.children[1].bindings[0].node = 9999;
    app.selected_scope = vec![0];
    let ctx = context();
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let current = label(&app, 3);
    click_label(&mut app, &ctx, true, &current, true);
    let selected = label(&app, 7);
    click_label(&mut app, &ctx, true, &selected, true);
    assert_eq!(app.project.root.children[0].bindings[0].node, 7);
    assert_eq!(app.project.root.children[1].bindings[0].node, 9999);
    assert!(matches!(
        app.history.next_undo(),
        Some(HistoryRecord::Command(_))
    ));
}

#[test]
fn filter_map_scope_routes_share_the_closed_lookup_admission() {
    let mut app = app(MappingDocument::Main);
    for node in [7, 12] {
        let error = crate::filter_map_editor::parent_error(
            &app.project.graph,
            node,
            &Default::default(),
            None,
        )
        .unwrap();
        assert!(error.contains("Owned Lookup"));
    }
    assert!(
        crate::filter_map_editor::parent_error(&app.project.graph, 3, &Default::default(), None)
            .is_none()
    );
    app.project.root.children[1].iteration = mapping::ScopeIteration::Source(vec!["Rows".into()]);
    app.project.root.children[1].bindings[0].node = 7;
    run(&app, INPUT);
    let before = observed(&app);
    assert!(
        app.create_selected_filter_map()
            .unwrap_err()
            .contains("ordinary repeating")
    );
    assert_eq!(observed(&app), before);
}

#[test]
fn saved_dynamic_lookup_owner_stays_read_only_in_an_ordinary_scope_picker() {
    let mut app = app(MappingDocument::Main);
    app.project.extra_sources[0].dynamic_path =
        Some(mapping::DynamicSourcePath::legacy(3, vec!["Rows".into()]));
    if let Node::OwnedLookup(lookup) = app.project.graph.nodes.get_mut(&7).unwrap() {
        lookup.collection.owner = named_owner();
    }
    app.project.root.children[0].bindings[0].node = 7;
    app.selected_scope = vec![0];
    let ctx = context();
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    let current = label(&app, 7);
    click_label(&mut app, &ctx, true, &current, true);
    assert_eq!(observed(&app), before);
    assert!(
        !visible(
            &settle_inspector(&mut app, &ctx, true),
            crate::scope_editor::lookup_admission::READ_ONLY
        )
        .is_empty()
    );
}

fn grouped_fixture(grouped: bool, legacy: bool, document: MappingDocument) -> FerruleApp {
    let mut app = super::fixture(document);
    app.project.graph.nodes.insert(
        5,
        Node::Aggregate {
            function: mapping::AggregateOp::Count,
            collection: vec!["Rows".into()],
            value: vec![],
            expression: None,
            arg: None,
        },
    );
    if legacy {
        app.project.graph.nodes.insert(
            7,
            Node::Lookup {
                collection: vec!["Rows".into()],
                key: vec!["Code".into()],
                matches: 3,
                value: vec!["PrimaryValue".into()],
            },
        );
    }
    app.project.graph.nodes.insert(
        12,
        Node::Call {
            function: "concat".into(),
            args: vec![7],
        },
    );
    app.project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Grouped",
                vec![
                    SchemaNode::scalar("Count", ScalarType::Int),
                    SchemaNode::scalar("Result", ScalarType::String),
                ],
            )
            .repeating(),
            SchemaNode::group(
                "Plain",
                vec![SchemaNode::scalar("Result", ScalarType::String)],
            ),
        ],
    );
    app.project.root = Scope {
        children: vec![
            Scope {
                target_field: "Grouped".into(),
                iteration: mapping::ScopeIteration::Source(vec!["Rows".into()]),
                group_by: grouped.then_some(3),
                bindings: vec![
                    mapping::Binding {
                        target_field: "Count".into(),
                        node: 5,
                    },
                    mapping::Binding {
                        target_field: "Result".into(),
                        node: 3,
                    },
                ],
                ..Default::default()
            },
            Scope {
                target_field: "Plain".into(),
                bindings: vec![mapping::Binding {
                    target_field: "Result".into(),
                    node: 3,
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    app.project.extra_targets[0].schema = app.project.target.clone();
    app.project.extra_targets[0].root = app.project.root.clone();
    rebuild(&mut app, document);
    arrange_grouped_nodes(&mut app);
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    app
}

fn arrange_grouped_nodes(app: &mut FerruleApp) {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let canvas = canvas_mut(app, document);
        let ids = canvas
            .snarl
            .node_ids()
            .map(|(id, node)| (id, *node))
            .collect::<Vec<_>>();
        for (id, node) in ids {
            if let CanvasNode::Graph(node) = node {
                canvas.snarl.get_node_info_mut(id).unwrap().pos = match node {
                    7 => egui::pos2(580.0, 20.0),
                    5 => egui::pos2(850.0, 220.0),
                    12 => egui::pos2(850.0, 410.0),
                    _ => egui::pos2(320.0, 20.0),
                };
            }
        }
    }
}

fn grouped_counterexample(app: &FerruleApp, document: MappingDocument, expression: NodeId) {
    let mut project = app.project.clone();
    let root = if document == MappingDocument::Main {
        &mut project.root
    } else {
        &mut project.extra_targets[0].root
    };
    root.children[0].bindings[1].node = expression;
    assert!(engine::validate(&project).is_empty());
    assert!(matches!(
        engine::check_source_ownership_capability(&project, None),
        Err(engine::EngineError::UnsupportedOwnedContext {
            context: engine::OwnedCapabilityContext::AggregateLookup,
            ..
        })
    ));
}

#[test]
fn grouped_count_sibling_blocks_owned_lookup_assignment_but_plain_sibling_works() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = grouped_fixture(true, false, document);
        let original = run(&app, INPUT);
        let rows = original.primary.field("Grouped").unwrap();
        let ir::Instance::Repeated(rows) = rows else {
            panic!("grouped rows")
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].field("Count"),
            Some(&ir::Instance::Scalar(Value::Int(3)))
        );
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        for expression in [7, 12] {
            grouped_counterexample(&app, document, expression);
            let current = label(&app, 3);
            click_label(&mut app, &ctx, true, &current, true);
            let before = observed(&app);
            let candidate = label(&app, expression);
            click_label(&mut app, &ctx, true, &candidate, true);
            assert_eq!(observed(&app), before);
            assert_eq!(run(&app, INPUT).primary, original.primary);
            close(&mut app, &ctx);
        }
        app.selected_scope = vec![1];
        settle_inspector(&mut app, &ctx, true);
        for expression in [7, 12] {
            let current = label(&app, root(&app).children[1].bindings[0].node);
            click_label(&mut app, &ctx, true, &current, true);
            let candidate = label(&app, expression);
            click_label(&mut app, &ctx, true, &candidate, true);
            assert_eq!(root(&app).children[1].bindings[0].node, expression);
            let result = run(&app, INPUT);
            let output = if document == MappingDocument::Main {
                &result.primary
            } else {
                &result.extras[0].instance
            };
            assert_eq!(
                output.field("Plain").unwrap().field("Result"),
                Some(&ir::Instance::Scalar(Value::String("primary-first".into())))
            );
            assert_eq!(output.field("Grouped"), original.primary.field("Grouped"));
        }
    }
}

#[test]
fn grouped_subtree_wire_routes_and_legacy_conversion_are_noops() {
    for expression in [7, 12] {
        let mut app = grouped_fixture(true, false, MappingDocument::Main);
        let original = run(&app, INPUT);
        grouped_counterexample(&app, MappingDocument::Main, expression);
        let target = CanvasNode::TargetBlock(
            target_blocks(&app.project.target)
                .iter()
                .position(|block| block.chain == ["Grouped"])
                .unwrap(),
        );
        let before = observed(&app);
        assert!(wire_action(&mut app, CanvasNode::Graph(expression), 0, target, 1, false).is_err());
        assert_eq!(observed(&app), before);
        assert_eq!(run(&app, INPUT).primary, original.primary);
    }
    let mut app = grouped_fixture(true, true, MappingDocument::Main);
    app.project.root.children[0].bindings[1].node = 7;
    app.project.extra_targets[0].root = app.project.root.clone();
    rebuild(&mut app, MappingDocument::Main);
    arrange_grouped_nodes(&mut app);
    // The conversion keeps the needle and value paths but would activate owned grouping.
    let original = run(&app, INPUT);
    let mut converted = app.project.clone();
    converted.graph.nodes.insert(
        7,
        Node::OwnedLookup(OwnedLookup {
            collection: source(SourceOwner::Primary, "Rows"),
            key: vec!["Code".into()],
            matches: 3,
            value: vec!["PrimaryValue".into()],
        }),
    );
    assert!(matches!(
        engine::check_source_ownership_capability(&converted, None),
        Err(engine::EngineError::UnsupportedOwnedContext { .. })
    ));
    let ctx = context();
    super::context_regressions::open_legacy(&mut app, &ctx);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    click_nth(&mut app, &ctx, true, "Use an exact input", 0);
    assert_eq!(observed(&app), before);
    assert_eq!(run(&app, INPUT).primary, original.primary);
}

#[test]
fn grouping_controls_cannot_promote_existing_lookup_subtrees() {
    let mut app = grouped_fixture(false, false, MappingDocument::Main);
    app.project.root.children[0].bindings[1].node = 7;
    app.project.extra_targets[0].root = app.project.root.clone();
    let original = run(&app, INPUT);
    let ctx = context();
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    for mode in [
        "By key",
        "Adjacent keys",
        "Starting predicate",
        "Ending predicate",
        "Fixed-size blocks",
    ] {
        click_label(&mut app, &ctx, true, "No grouping", false);
        let before = observed(&app);
        click_label(&mut app, &ctx, true, mode, true);
        assert_eq!(observed(&app), before, "{mode}");
        assert!(!app.project.root.children[0].has_grouping());
        assert_eq!(run(&app, INPUT).primary, original.primary);
    }
}

#[test]
fn grouped_descendants_and_saved_owned_metadata_remain_read_only() {
    let mut app = grouped_fixture(true, false, MappingDocument::Main);
    app.project.root.children[0].children.push(Scope {
        target_field: "Detail".into(),
        bindings: vec![mapping::Binding {
            target_field: "Result".into(),
            node: 3,
        }],
        ..Default::default()
    });
    if let ir::SchemaKind::Group { children, .. } = &mut app.project.target.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
    {
        children.push(SchemaNode::group(
            "Detail",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        ));
    }
    let original = run(&app, INPUT);
    app.selected_scope = vec![0, 0];
    let ctx = context();
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let current = label(&app, 3);
    click_label(&mut app, &ctx, true, &current, true);
    let before = observed(&app);
    let candidate = label(&app, 7);
    click_label(&mut app, &ctx, true, &candidate, true);
    assert_eq!(observed(&app), before);
    assert_eq!(run(&app, INPUT).primary, original.primary);
    close(&mut app, &ctx);
    app.project.root.children[0].bindings[1].node = 7;
    app.selected_scope = vec![0];
    settle_inspector(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    let current = label(&app, 7);
    click_label(&mut app, &ctx, true, &current, true);
    assert_eq!(observed(&app), before);
    assert!(
        !visible(
            &settle_inspector(&mut app, &ctx, true),
            crate::scope_editor::lookup_admission::READ_ONLY
        )
        .is_empty()
    );
}

#[test]
fn grouped_canvas_cannot_create_owned_lookups() {
    let mut app = grouped_fixture(true, false, MappingDocument::Main);
    app.project.root = app.project.root.children[0].clone();
    app.project.target = match &app.project.target.kind {
        ir::SchemaKind::Group { children, .. } => children[0].clone(),
        _ => unreachable!(),
    };
    rebuild(&mut app, MappingDocument::Main);
    let original = run(&app, INPUT);
    let ctx = context();
    settle(&mut app, &ctx, true);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    pointer(
        &mut app,
        &ctx,
        true,
        egui::pos2(1100.0, 850.0),
        egui::PointerButton::Secondary,
    );
    click_nth(&mut app, &ctx, true, "Lookup from one input", 0);
    assert_eq!(observed(&app), before);
    assert_eq!(run(&app, INPUT).primary, original.primary);
}

#[test]
fn grouping_an_ancestor_cannot_promote_an_already_grouped_lookup_descendant() {
    for bridge in [false, true] {
        let mut app = grouped_fixture(false, false, MappingDocument::Main);
        let child = Scope {
            target_field: "Nested".into(),
            iteration: mapping::ScopeIteration::Source(vec!["Rows".into()]),
            group_by: Some(3),
            bindings: vec![mapping::Binding {
                target_field: "Result".into(),
                node: 7,
            }],
            ..Default::default()
        };
        let schema = SchemaNode::group(
            "Nested",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        )
        .repeating();
        let (child, schema) = if bridge {
            (
                Scope {
                    target_field: "Bridge".into(),
                    children: vec![child],
                    ..Default::default()
                },
                SchemaNode::group("Bridge", vec![schema]),
            )
        } else {
            (child, schema)
        };
        app.project.root.children[0].children.push(child);
        if let ir::SchemaKind::Group { children, .. } = &mut app.project.target.kind
            && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        {
            children.push(schema);
        }
        // One Lookup is shared with an ordinary sibling and the grouped child.
        app.project.root.children[1].bindings[0].node = 7;
        app.project.extra_targets[0].schema = app.project.target.clone();
        app.project.extra_targets[0].root = app.project.root.clone();
        rebuild(&mut app, MappingDocument::Main);
        app.selected_scope = vec![0];
        let original = run(&app, INPUT);
        let mut counterfactual = app.project.clone();
        counterfactual.root.children[0].group_by = Some(3);
        assert!(engine::validate(&counterfactual).is_empty());
        assert!(matches!(
            engine::check_source_ownership_capability(&counterfactual, None),
            Err(engine::EngineError::UnsupportedOwnedContext {
                context: engine::OwnedCapabilityContext::AggregateLookup,
                ..
            })
        ));
        let ctx = context();
        settle_inspector(&mut app, &ctx, true);
        app.mark_clean();
        app.rebase_history();
        for mode in [
            "By key",
            "Adjacent keys",
            "Starting predicate",
            "Ending predicate",
            "Fixed-size blocks",
        ] {
            click_label(&mut app, &ctx, true, "No grouping", false);
            let before = observed(&app);
            click_label(&mut app, &ctx, true, mode, true);
            assert_eq!(observed(&app), before, "bridge={bridge} mode={mode}");
            assert!(!app.project.root.children[0].has_grouping());
            let output = run(&app, INPUT);
            assert_eq!(output.primary, original.primary);
            assert_eq!(output.extras[0].instance, original.extras[0].instance);
            close(&mut app, &ctx);
        }
    }
}

#[test]
fn grouping_defaults_skip_unrelated_lookups_and_use_literal_or_field_candidates() {
    for field in [false, true] {
        for mode in ["By key", "Adjacent keys"] {
            let mut app = grouped_fixture(false, false, MappingDocument::Main);
            let lookup = app.project.graph.nodes.remove(&7).unwrap();
            app.project.graph.nodes.remove(&12);
            app.project.graph.nodes.insert(0, lookup);
            app.project.graph.nodes.insert(
                1,
                Node::Call {
                    function: "concat".into(),
                    args: vec![0],
                },
            );
            let mut input = INPUT.to_owned();
            let candidate = if field {
                if let ir::SchemaKind::Group { children, .. } = &mut app.project.source.kind {
                    children.push(SchemaNode::scalar("GroupKey", ScalarType::String));
                }
                let mut json: serde_json::Value = serde_json::from_str(INPUT).unwrap();
                json["GroupKey"] = serde_json::json!("one-group");
                input = json.to_string();
                Node::SourceField {
                    path: vec!["GroupKey".into()],
                    frame: None,
                }
            } else {
                Node::Const {
                    value: Value::String("one-group".into()),
                }
            };
            app.project.graph.nodes.insert(2, candidate);
            app.project.root.children[1].bindings[0].node = 0;
            app.project.extra_targets[0].root = app.project.root.clone();
            rebuild(&mut app, MappingDocument::Main);
            app.selected_scope = vec![0];
            run(&app, &input);
            let ctx = context();
            settle_inspector(&mut app, &ctx, true);
            app.mark_clean();
            app.rebase_history();
            click_label(&mut app, &ctx, true, "No grouping", false);
            click_label(&mut app, &ctx, true, mode, true);
            let scope = &app.project.root.children[0];
            assert_eq!(
                scope.group_by.or(scope.group_adjacent_by),
                Some(2),
                "field={field} mode={mode}"
            );
            assert!(matches!(
                app.history.next_undo(),
                Some(HistoryRecord::Command(_))
            ));
            let grouped = run(&app, &input);
            let ir::Instance::Repeated(rows) = grouped.primary.field("Grouped").unwrap() else {
                panic!("grouped rows")
            };
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0].field("Count"),
                Some(&ir::Instance::Scalar(Value::Int(3)))
            );
            assert_eq!(
                grouped.primary.field("Plain").unwrap().field("Result"),
                Some(&ir::Instance::Scalar(Value::String("primary-first".into())))
            );
            app.undo_project();
            assert!(!app.project.root.children[0].has_grouping());
            run(&app, &input);
            app.redo_project();
            assert_eq!(run(&app, &input).primary, grouped.primary);
        }
    }
}
