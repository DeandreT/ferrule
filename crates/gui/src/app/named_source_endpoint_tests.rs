//! Named boundary identity, canvas migration and conservative shadowing policy.
//! Shadowed aliases are wired as exact owned reads (#929).

use super::*;
use crate::canvas::{SourceBlock, TargetBlock, project_source_blocks, source_blocks};
use egui_snarl::ui::SnarlViewer;
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Binding, FormatOptions, NamedSource, NamedTarget, ScopeIteration};

fn schema(name: &str, count: usize, repeating: bool) -> SchemaNode {
    let mut schema = SchemaNode::group(
        name,
        (0..count)
            .map(|index| SchemaNode::scalar(format!("field_{index:02}"), ScalarType::String))
            .collect(),
    );
    schema.repeating = repeating;
    schema
}

fn named(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema,
        options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        dynamic_path: None,
    }
}

fn project() -> Project {
    let mut project = blank_project();
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Out", ScalarType::String)],
    );
    project.graph.nodes.clear();
    project.root = Scope::default();
    project.extra_sources.clear();
    project.extra_targets.clear();
    project
}

fn input(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

fn output(value: &str) -> Instance {
    Instance::Group(vec![("Out".into(), Instance::Scalar(Value::String(value.into())))].into())
}

struct Catalogs {
    sources: Vec<SourceBlock>,
    targets: Vec<TargetBlock>,
    paths: SourcePathCatalog,
}

impl Catalogs {
    fn new(project: &Project) -> Self {
        Self {
            sources: project_source_blocks(project),
            targets: target_blocks(&project.target),
            paths: SourcePathCatalog::new(&project.source, &project.extra_sources)
                .with_primary_source_options(&project.source_options),
        }
    }
}

fn viewer<'a>(
    project: &'a mut Project,
    catalogs: &'a Catalogs,
    scroll: &'a mut crate::canvas_endpoints::EndpointScrollState,
) -> GraphViewer<'a> {
    let source_x12 = crate::x12_tooltips::boundary_has_x12(
        &project.source,
        project.source_path.as_deref(),
        &project.source_options,
    );
    GraphViewer {
        graph: &mut project.graph,
        root_scope: &mut project.root,
        primary_root_authoring: false,
        extra_targets: &project.extra_targets,
        inactive_target_scopes: &[],
        project_references: crate::graph_viewer::ProjectGraphReferences::new(
            &project.failure_rules,
            &project.extra_sources,
        )
        .with_user_functions(&project.user_functions)
        .with_primary_source(&project.source),
        source_blocks: &catalogs.sources,
        target_blocks: &catalogs.targets,
        source_x12,
        target_x12: false,
        source_paths: &catalogs.paths,
        function_names: Default::default(),
        function_inputs: Default::default(),
        parameter_names: Default::default(),
        protected_output: None,
        function_output: None,
        requested_function_open: None,
        colors: crate::appearance::SemanticThemeColors::default(),
        wire_color_mode: crate::appearance::WireColorMode::Theme,
        endpoint_scroll: scroll,
        value_map_wheel: None,
        endpoint_search_match: None,
        node_sizes: None,
        hovered_node: None,
        hovered_node_this_frame: None,
        camera_pan: egui::Vec2::ZERO,
        camera_focus: None,
        canvas_transform: None,
        pin_interaction_ids: Vec::new(),
        planned_edits: Vec::new(),
        error: None,
    }
}

fn canvas_id(snarl: &Snarl<CanvasNode>, node: CanvasNode) -> egui_snarl::NodeId {
    snarl
        .node_ids()
        .find_map(|(id, &candidate)| (candidate == node).then_some(id))
        .expect("canvas node exists")
}

fn position(snarl: &Snarl<CanvasNode>, node: CanvasNode) -> egui::Pos2 {
    snarl.get_node_info(canvas_id(snarl, node)).unwrap().pos
}

fn move_node(snarl: &mut Snarl<CanvasNode>, node: CanvasNode, position: egui::Pos2) {
    let id = canvas_id(snarl, node);
    snarl.get_node_info_mut(id).unwrap().pos = position;
}

fn connect(
    project: &mut Project,
    block: usize,
    pin: usize,
    scroll: &mut crate::canvas_endpoints::EndpointScrollState,
) -> Result<(), String> {
    let catalogs = Catalogs::new(project);
    let mut snarl = build_snarl(project);
    let from = snarl.out_pin(OutPinId {
        node: canvas_id(&snarl, CanvasNode::SourceBlock(block)),
        output: pin,
    });
    let to = snarl.in_pin(InPinId {
        node: canvas_id(&snarl, CanvasNode::TargetBlock(0)),
        input: 0,
    });
    let mut viewer = viewer(project, &catalogs, scroll);
    viewer.connect(&from, &to, &mut snarl);
    viewer.error.map_or(Ok(()), Err)
}

#[test]
fn project_catalog_preserves_primary_and_qualifies_named_frames_and_paths() {
    let mut project = project();
    project.source = schema("Source: Source: 長い主入力", 30, false);
    let plain = "Source: Source: 参照_very_long_boundary";
    project.extra_sources = vec![
        named(plain, schema("Different schema label", 30, false)),
        named("orders", schema("Rows", 30, true)),
        named(
            "nested",
            SchemaNode::group("Nested", vec![schema("Rows", 1, true)]),
        ),
    ];
    let blocks = project_source_blocks(&project);
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0], source_blocks(&project.source)[0]);
    assert_eq!(blocks[1].title, format!("Source: {plain}"));
    assert_eq!(blocks[1].pin_labels[29], "field_29");
    assert_eq!(blocks[1].leaves[29].label, "field_29");
    assert_eq!(blocks[1].leaves[29].frame, None);
    assert_eq!(blocks[1].leaves[29].path, [plain, "field_29"]);
    assert_eq!(blocks[2].frame, Some(vec!["orders".into()]));
    assert_eq!(blocks[2].leaves[12].frame, Some(vec!["orders".into()]));
    assert_eq!(blocks[2].leaves[12].path, ["field_12"]);
    assert_eq!(
        blocks[3].leaves[0].frame,
        Some(vec!["nested".into(), "Rows".into()])
    );
    assert_eq!(blocks[3].leaves[0].path, ["field_00"]);
    assert!(crate::canvas::source_conflicts(&project.source, &project.extra_sources).is_empty());
}

#[test]
fn real_named_connections_read_distinct_nonrepeating_and_repeating_values() {
    for repeating in [false, true] {
        let mut project = project();
        let mut named_schema = project.source.clone();
        named_schema.repeating = repeating;
        project.extra_sources = vec![named("reference", named_schema)];
        if repeating {
            project.target.repeating = true;
            project.root.iteration = ScopeIteration::Source(vec!["reference".into()]);
        }
        connect(&mut project, 1, 0, &mut Default::default()).unwrap();
        let expression = project.root.bindings[0].node;
        let expected_field = if repeating {
            Node::SourceField {
                frame: Some(vec!["reference".into()]),
                path: vec!["Value".into()],
            }
        } else {
            Node::SourceField {
                frame: None,
                path: vec!["reference".into(), "Value".into()],
            }
        };
        match (&project.graph.nodes[&expression], expected_field) {
            (
                Node::SourceField { frame, path },
                Node::SourceField {
                    frame: expected_frame,
                    path: expected_path,
                },
            ) => {
                assert_eq!(frame, &expected_frame);
                assert_eq!(path, &expected_path);
            }
            other => panic!("connection must create the exact named field: {other:?}"),
        }
        assert!(cli::validate(&project).is_empty());
        let extra = if repeating {
            Instance::Repeated(vec![input("named-row")])
        } else {
            input("named-object")
        };
        let actual = engine::run_with_sources(
            &project,
            &input("primary-value"),
            vec![("reference".into(), extra)],
        )
        .unwrap();
        let expected = if repeating {
            Instance::Repeated(vec![output("named-row")])
        } else {
            output("named-object")
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn primary_legacy_unframed_field_is_not_redirected_by_equal_named_leaf_names() {
    let mut project = project();
    project.source.repeating = true;
    project.target.repeating = true;
    project.root.iteration = ScopeIteration::Source(Vec::new());
    let mut extra = project.source.clone();
    extra.repeating = true;
    project.extra_sources = vec![named("reference", extra)];
    project.graph.nodes.insert(
        7,
        Node::SourceField {
            frame: None,
            path: vec!["Value".into()],
        },
    );
    project.root.bindings = vec![Binding {
        target_field: "Out".into(),
        node: 7,
    }];
    let snarl = build_snarl(&project);
    assert!(!snarl.nodes().any(|node| *node == CanvasNode::Graph(7)));
    let wires = snarl.wires().collect::<Vec<_>>();
    assert_eq!(wires.len(), 1);
    assert_eq!(snarl[wires[0].0.node], CanvasNode::SourceBlock(0));
    assert_eq!(wires[0].0.output, 0);
    assert_eq!(
        engine::run_with_sources(
            &project,
            &Instance::Repeated(vec![input("primary-legacy")]),
            vec![("reference".into(), Instance::Repeated(vec![input("named")]))],
        )
        .unwrap(),
        Instance::Repeated(vec![output("primary-legacy")])
    );
}

#[test]
fn ambiguous_named_pin_authors_its_exact_owner_while_saved_shadowing_is_unchanged() {
    for repeating in [false, true] {
        let mut project = project();
        let mut value_schema = project.source.clone();
        value_schema.repeating = repeating;
        let mut primary_catalog = SchemaNode::group(
            "catalog",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        primary_catalog.repeating = repeating;
        project.source = SchemaNode::group("Primary", vec![primary_catalog]);
        project.extra_sources = vec![named("catalog", value_schema)];
        project.target.repeating = repeating;
        if repeating {
            project.root.iteration = ScopeIteration::Source(vec!["catalog".into()]);
        }
        project.graph.nodes.insert(
            7,
            if repeating {
                Node::SourceField {
                    frame: Some(vec!["catalog".into()]),
                    path: vec!["Value".into()],
                }
            } else {
                Node::SourceField {
                    frame: None,
                    path: vec!["catalog".into(), "Value".into()],
                }
            },
        );
        project.root.bindings = vec![Binding {
            target_field: "Out".into(),
            node: 7,
        }];
        let catalogs = Catalogs::new(&project);
        let snarl = build_snarl(&project);
        assert_eq!(catalogs.sources.len(), 2);
        let wire = snarl.wires().next().unwrap();
        assert_eq!(snarl[wire.0.node], CanvasNode::SourceBlock(0));
        let wrap = |value: &str| {
            if repeating {
                Instance::Repeated(vec![input(value)])
            } else {
                input(value)
            }
        };
        let primary = Instance::Group(vec![("catalog".into(), wrap("primary-shadow"))].into());
        let run = |project: &Project| {
            engine::run_with_sources(project, &primary, vec![("catalog".into(), wrap("named"))])
                .unwrap()
        };
        let expected = |value: &str| {
            if repeating {
                Instance::Repeated(vec![output(value)])
            } else {
                output(value)
            }
        };
        assert!(
            cli::validate(&project).is_empty(),
            "native shadowing remains admitted"
        );
        assert_eq!(run(&project), expected("primary-shadow"));

        connect(&mut project, 1, 0, &mut Default::default()).unwrap();
        let read = project.root.bindings[0].node;
        let Node::OwnedSourceField(field) = &project.graph.nodes[&read] else {
            panic!("the pin authors an owned field");
        };
        assert_eq!(
            field,
            &mapping::OwnedSourceField {
                source: mapping::OwnedSourceRef {
                    owner: mapping::SourceOwner::Named {
                        name: "catalog".into()
                    },
                    path: vec!["Value".into()],
                },
                frame: repeating.then(Vec::new),
            }
        );
        assert!(
            !project.graph.nodes.contains_key(&7),
            "the displaced hidden legacy field is orphaned"
        );
        if repeating {
            // The legacy scope path still names the primary collection; an
            // owned row read needs the named rows to be iterated by owner.
            project
                .root
                .set_source_selector(Some(mapping::SourceSelector::Owned(
                    mapping::OwnedSourceRef {
                        owner: mapping::SourceOwner::Named {
                            name: "catalog".into(),
                        },
                        path: vec![],
                    },
                )));
        }
        assert!(cli::validate(&project).is_empty());
        assert_eq!(run(&project), expected("named"));
    }
}

#[test]
fn nested_other_named_and_unknown_runtime_fields_conservatively_mark_alias_conflicts() {
    let mut project = project();
    let original = project.source.clone();
    project.extra_sources = vec![named("catalog", original.clone())];
    project.source = SchemaNode::group(
        "Primary",
        vec![SchemaNode::group(
            "Rows",
            vec![SchemaNode::group("catalog", vec![])],
        )],
    );
    assert_eq!(
        crate::canvas::source_conflicts(&project.source, &project.extra_sources),
        [1].into()
    );
    project.source = original.clone();
    project.extra_sources.push(named(
        "orders",
        SchemaNode::group("Rows", vec![SchemaNode::group("catalog", vec![])]),
    ));
    assert_eq!(
        crate::canvas::source_conflicts(&project.source, &project.extra_sources),
        [1].into()
    );
    project.extra_sources[1].schema = SchemaNode::group(
        "Orders",
        vec![SchemaNode::group(
            "catalog",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )],
    );
    project.extra_sources[1].schema.repeating = true;
    project.target.repeating = true;
    project.root.iteration = ScopeIteration::Source(vec!["orders".into()]);
    project.graph.nodes.insert(
        7,
        Node::SourceField {
            frame: None,
            path: vec!["catalog".into(), "Value".into()],
        },
    );
    project.root.bindings = vec![Binding {
        target_field: "Out".into(),
        node: 7,
    }];
    assert!(cli::validate(&project).is_empty());
    let actual = engine::run_with_sources(
        &project,
        &input("primary"),
        vec![
            ("catalog".into(), input("named-catalog")),
            (
                "orders".into(),
                Instance::Repeated(vec![Instance::Group(
                    vec![("catalog".into(), input("inner-named-shadow"))].into(),
                )]),
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        actual,
        Instance::Repeated(vec![output("inner-named-shadow")])
    );
    // Another input's field shadows the alias, so the pin reads it by owner.
    connect(&mut project, 1, 0, &mut Default::default()).unwrap();
    let read = project.root.bindings[0].node;
    assert!(matches!(
        &project.graph.nodes[&read],
        Node::OwnedSourceField(field)
            if field.source.owner == mapping::SourceOwner::Named { name: "catalog".into() }
                && field.source.path == ["Value"]
                && field.frame.is_none()
    ));
    assert!(cli::validate(&project).is_empty());
    assert_eq!(
        engine::run_with_sources(
            &project,
            &input("primary"),
            vec![
                ("catalog".into(), input("named-catalog")),
                (
                    "orders".into(),
                    Instance::Repeated(vec![Instance::Group(
                        vec![("catalog".into(), input("inner-named-shadow"))].into(),
                    )]),
                ),
            ],
        )
        .unwrap(),
        Instance::Repeated(vec![output("named-catalog")])
    );
    if let SchemaKind::Group { dynamic, .. } = &mut project.source.kind {
        *dynamic = Some(Box::new(original.clone()));
    }
    assert_eq!(
        crate::canvas::source_conflicts(&project.source, &project.extra_sources),
        [1, 2].into()
    );
    project.source = original.clone();
    project.extra_sources[1].schema.recursive_ref = Some("Unknown".into());
    assert_eq!(
        crate::canvas::source_conflicts(&project.source, &project.extra_sources),
        [1, 2].into()
    );
    project.extra_sources = vec![
        named("catalog", original.clone()),
        named("catalog", original.clone()),
    ];
    assert_eq!(
        crate::canvas::source_conflicts(&project.source, &project.extra_sources),
        [1, 2].into()
    );
    project.extra_sources = vec![named("", original.clone()), named("orders", original)];
    assert_eq!(
        crate::canvas::source_conflicts(&project.source, &project.extra_sources),
        [1].into()
    );
}

#[test]
fn named_target_wires_keep_real_field_identity_at_three_scroll_offsets() {
    let mut project = project();
    project.source = schema("Input", 30, false);
    project.target = schema("Output", 30, false);
    project.extra_sources = vec![named("reference", schema("Rows", 30, true))];
    let target = schema("Named output", 30, true);
    let bindings = [0, 11, 12, 29]
        .into_iter()
        .map(|pin| {
            let node = 100 + pin;
            project.graph.nodes.insert(
                node,
                Node::SourceField {
                    frame: Some(vec!["reference".into()]),
                    path: vec![format!("field_{pin:02}")],
                },
            );
            Binding {
                target_field: format!("field_{pin:02}"),
                node,
            }
        })
        .collect();
    project.extra_targets = vec![NamedTarget {
        name: "result".into(),
        schema: target,
        path: None,
        options: Default::default(),
        root: Scope {
            iteration: ScopeIteration::Source(vec!["reference".into()]),
            bindings,
            ..Default::default()
        },
    }];
    let original = mapping::project_file::encode_pretty(&project).unwrap();
    let sources = project_source_blocks(&project);
    let targets = target_blocks(&project.extra_targets[0].schema);
    let owned = crate::graph_viewer::project_sequence_item_ids(&project);
    let unavailable = source_conflicts(&project.source, &project.extra_sources);
    for (offset, expected) in [
        (0, vec![(0, 0), (11, 11), (12, 12)]),
        (10, vec![(0, 0), (2, 2), (3, 3), (13, 13)]),
        (18, vec![(0, 0), (12, 12)]),
    ] {
        let mut snarl = canvas_build::build_named_target_snarl(&project, 0);
        let mut scroll = crate::canvas_endpoints::EndpointScrollState::default();
        scroll.scroll_rows(CanvasNode::SourceBlock(1), 30, offset);
        scroll.scroll_rows(CanvasNode::TargetBlock(0), 30, offset);
        sync_endpoint_wires_with_policy(
            &project.graph,
            &project.extra_targets[0].root,
            &sources,
            &targets,
            &scroll,
            &mut snarl,
            SourcePinPolicy {
                owned_items: &owned,
                unavailable_blocks: &unavailable,
            },
        );
        let mut actual = snarl
            .wires()
            .map(|(from, to)| {
                assert_eq!(snarl[from.node], CanvasNode::SourceBlock(1));
                assert_eq!(snarl[to.node], CanvasNode::TargetBlock(0));
                (from.output, to.input)
            })
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        assert_eq!(
            mapping::project_file::encode_pretty(&project).unwrap(),
            original
        );
    }
    let mut scroll = crate::canvas_endpoints::EndpointScrollState::default();
    scroll.scroll_rows(CanvasNode::SourceBlock(1), 30, 18);
    assert_eq!(
        connect(&mut project, 1, 0, &mut scroll),
        Err("source pin 0 does not exist".into())
    );
    assert_eq!(
        mapping::project_file::encode_pretty(&project).unwrap(),
        original
    );
}

#[test]
fn source_removal_migrates_alias_positions_in_main_and_cached_target_history_and_layout() {
    let mut project = project();
    let source_schema = project.source.clone();
    project.extra_sources = vec![
        named("old", source_schema.clone()),
        named("kept", source_schema),
    ];
    for (node, alias) in [(40, "old"), (41, "kept")] {
        project.graph.nodes.insert(
            node,
            Node::SourceField {
                frame: None,
                path: vec![alias.into(), "Value".into()],
            },
        );
    }
    project.graph.nodes.insert(
        99,
        Node::Const {
            value: Value::String("kept-node".into()),
        },
    );
    project
        .graph
        .nodes
        .insert(100, Node::Const { value: Value::Null });
    project.root.bindings = vec![Binding {
        target_field: "Out".into(),
        node: 41,
    }];
    project.extra_targets = vec![NamedTarget {
        name: "other".into(),
        schema: project.target.clone(),
        path: None,
        options: Default::default(),
        root: project.root.clone(),
    }];
    let mut app = FerruleApp {
        project,
        ..Default::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    let placeholder = canvas_id(&app.main_canvas.snarl, CanvasNode::Graph(100));
    app.main_canvas.snarl[placeholder] = CanvasNode::Placeholder(100);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(0),
        egui::pos2(70.0, 80.0),
    );
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::SourceBlock(2),
        egui::pos2(340.0, 270.0),
    );
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::Graph(99),
        egui::pos2(600.0, 420.0),
    );
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::Placeholder(100),
        egui::pos2(540.0, 330.0),
    );
    move_node(
        &mut app.main_canvas.snarl,
        CanvasNode::TargetBlock(0),
        egui::pos2(860.0, 180.0),
    );
    move_node(
        &mut app
            .mapping_workspace
            .target_canvases
            .get_mut(&0)
            .unwrap()
            .snarl,
        CanvasNode::SourceBlock(2),
        egui::pos2(290.0, 310.0),
    );
    app.rebase_history();
    let mut draft = ExtraSourceDraft::default();
    draft.name = "later".into();
    draft.instance_path = "later.json".into();
    draft.options.json_document = true;
    draft.set_schema(app.project.source.clone());
    app.extra_source_draft = Some(draft);
    app.finish_extra_source();
    assert_eq!(app.project.extra_sources.len(), 3);
    assert!(
        app.main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::SourceBlock(3))
    );
    assert!(
        app.mapping_workspace.target_canvases[&0]
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::SourceBlock(3))
    );
    let _ = app.observe_editor_history(std::time::Instant::now(), false);
    let before_remove = mapping::project_file::encode_pretty(&app.project).unwrap();
    app.remove_project_extra_source(0);
    let _ = app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.project.extra_sources[0].name, "kept");
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::SourceBlock(1)),
        egui::pos2(340.0, 270.0)
    );
    assert_eq!(
        position(
            &app.mapping_workspace.target_canvases[&0].snarl,
            CanvasNode::SourceBlock(1)
        ),
        egui::pos2(290.0, 310.0)
    );
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::SourceBlock(0)),
        egui::pos2(70.0, 80.0)
    );
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Graph(99)),
        egui::pos2(600.0, 420.0)
    );
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::Placeholder(100)),
        egui::pos2(540.0, 330.0)
    );
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::TargetBlock(0)),
        egui::pos2(860.0, 180.0)
    );
    assert_eq!(app.project.root.bindings[0].node, 41);
    assert!(matches!(
        app.project.graph.nodes.get(&40),
        Some(Node::SourceField { frame: None, path }) if path == &["old", "Value"]
    ));
    assert!(
        app.main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Graph(40))
    );
    assert!(!cli::validate(&app.project).is_empty());
    let removed_state = mapping::project_file::encode_pretty(&app.project).unwrap();
    app.undo_project();
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project).unwrap(),
        before_remove
    );
    assert_eq!(
        position(&app.main_canvas.snarl, CanvasNode::SourceBlock(2)),
        egui::pos2(340.0, 270.0)
    );
    app.redo_project();
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project).unwrap(),
        removed_state
    );
    let layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    let saved_layout = serde_json::to_vec(&layout).unwrap();
    let saved_project = mapping::project_file::encode_pretty(&app.project).unwrap();
    let reopened = mapping::project_file::decode_str(&saved_project).unwrap();
    let layout: CanvasLayout = serde_json::from_slice(&saved_layout).unwrap();
    assert_eq!(layout.version, 3);
    let main = build_snarl_with_layout(&reopened, Some(&layout));
    let workspace = MappingWorkspace::from_layout(&reopened, Some(&layout));
    assert_eq!(
        position(&main, CanvasNode::SourceBlock(1)),
        egui::pos2(340.0, 270.0)
    );
    assert_eq!(
        position(&main, CanvasNode::Placeholder(100)),
        egui::pos2(540.0, 330.0)
    );
    assert_eq!(
        position(
            &workspace.target_canvases[&0].snarl,
            CanvasNode::SourceBlock(1)
        ),
        egui::pos2(290.0, 310.0)
    );
    assert_eq!(workspace.tabs, app.mapping_workspace.tabs);
    assert_eq!(workspace.active, app.mapping_workspace.active);
}

#[test]
fn x12_hints_belong_to_each_boundary_and_never_use_the_named_alias_as_segment() {
    let mut project = project();
    project.source_options.edi_kind = Some(mapping::EdiBoundaryKind::X12);
    project.extra_sources = vec![named("reference", project.source.clone())];
    let catalogs = Catalogs::new(&project);
    let mut scroll = Default::default();
    {
        let viewer = viewer(&mut project, &catalogs, &mut scroll);
        assert!(viewer.source_block_x12(0));
        assert!(!viewer.source_block_x12(1));
    }
    project.source_options.edi_kind = None;
    let mut x12 = SchemaNode::group("NM1", vec![SchemaNode::scalar("NM101", ScalarType::String)]);
    x12.repeating = true;
    project.extra_sources[0].schema = x12;
    project.extra_sources[0].options.edi_kind = Some(mapping::EdiBoundaryKind::X12);
    let catalogs = Catalogs::new(&project);
    let viewer = viewer(&mut project, &catalogs, &mut scroll);
    assert!(!viewer.source_block_x12(0));
    assert!(viewer.source_block_x12(1));
    assert_eq!(
        viewer.source_endpoint_hint(1, &catalogs.sources[1]),
        Some("X12 NM1: Individual or Organizational Name".into())
    );
}
