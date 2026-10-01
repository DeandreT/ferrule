use super::*;
use crate::canvas::{source_blocks, target_blocks};
use crate::canvas_endpoints::EndpointScrollState;
use egui_snarl::OutPinId;
use ir::{Instance, SchemaNode};
use mapping::{
    DynamicSourcePath, FailureIteration, FailureRule, FailureSelection, NamedSource, Project,
    ScopeConstruction, ScopeIteration, SequenceExpr, SortKey,
};

const SHARED: NodeId = 77;

fn output_schema(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("value", ScalarType::String)])
}

fn bound_root(node: NodeId) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "value".to_string(),
            node,
        }],
        ..Scope::default()
    }
}

fn three_output_project(node: Node) -> Project {
    let mut project = crate::new_mapping::blank_project();
    project.source = SchemaNode::group(
        "source",
        vec![SchemaNode::scalar("input", ScalarType::String)],
    );
    project.target = output_schema("primary");
    project.graph.nodes.insert(SHARED, node);
    project.root = bound_root(SHARED);
    project.extra_targets = ["audit", "archive"]
        .map(|name| NamedTarget {
            name: name.to_string(),
            path: None,
            schema: output_schema(name),
            options: Default::default(),
            root: bound_root(SHARED),
        })
        .into_iter()
        .collect();
    project
}

fn source() -> Instance {
    Instance::Group(vec![(
        "input".to_string(),
        Instance::Scalar(Value::String("source value".to_string())),
    )])
}

fn outputs(project: &Project) -> engine::ExecutionOutputs {
    let issues = engine::validate(project);
    assert!(issues.is_empty(), "mapping must remain valid: {issues:?}");
    engine::run_outputs(project, &source()).expect("valid three-output mapping")
}

fn assert_value(instance: &Instance, expected: &str) {
    assert_eq!(
        instance.field("value").and_then(Instance::as_scalar),
        Some(&Value::String(expected.to_string())),
    );
}

fn edit_named_target<R>(
    project: &mut Project,
    target_index: usize,
    edit: impl FnOnce(&mut GraphViewer<'_>) -> R,
) -> R {
    let source_blocks = source_blocks(&project.source);
    let source_paths = SourcePathCatalog::new(&project.source, &project.extra_sources);
    let (before, active_and_after) = project.extra_targets.split_at_mut(target_index);
    let (target, after) = active_and_after.split_first_mut().expect("named target");
    let inactive_target_scopes = inactive_target_scopes(&project.root, before, after);
    let target_blocks = target_blocks(&target.schema);
    let mut endpoint_scroll = EndpointScrollState::default();
    let mut viewer = GraphViewer {
        graph: &mut project.graph,
        root_scope: &mut target.root,
        extra_targets: &[],
        inactive_target_scopes: &inactive_target_scopes,
        project_references: ProjectGraphReferences::new(
            &project.failure_rules,
            &project.extra_sources,
        ),
        source_blocks: &source_blocks,
        target_blocks: &target_blocks,
        source_x12: false,
        target_x12: false,
        source_paths: &source_paths,
        function_names: Default::default(),
        function_inputs: Default::default(),
        parameter_names: Default::default(),
        protected_output: None,
        function_output: None,
        requested_function_open: None,
        colors: SemanticThemeColors::default(),
        wire_color_mode: WireColorMode::Theme,
        endpoint_scroll: &mut endpoint_scroll,
        value_map_wheel: None,
        endpoint_search_match: None,
        node_sizes: None,
        hovered_node: None,
        hovered_node_this_frame: None,
        camera_pan: egui::Vec2::ZERO,
        camera_focus: None,
        canvas_transform: None,
        pin_interaction_ids: Vec::new(),
        error: None,
    };
    edit(&mut viewer)
}

#[test]
fn named_canvas_blocks_shared_node_deletion_and_preserves_all_outputs() {
    // Both partitions are exercised: a sibling can precede or follow the
    // active target. Test the context-menu and selection deletion paths.
    for target_index in 0..2 {
        let mut project = three_output_project(Node::Const {
            value: Value::String("shared value".to_string()),
        });
        let before = outputs(&project);
        assert_value(&before.primary, "shared value");
        assert_eq!(before.extras.len(), 2);
        for target in &before.extras {
            assert_value(&target.instance, "shared value");
        }
        let saved = mapping::project_file::encode_pretty(&project).expect("project snapshot");
        let mut snarl = Snarl::new();
        let shared = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(SHARED));
        let sibling = if target_index == 0 {
            "archive"
        } else {
            "audit"
        };

        edit_named_target(&mut project, target_index, |viewer| {
            assert_eq!(
                viewer.blocking_references_to(SHARED),
                vec![
                    "scope <primary target> binding value".to_string(),
                    format!("scope <target {sibling}> binding value"),
                ],
            );
            if target_index == 0 {
                assert!(!viewer.remove_graph_node(SHARED, shared, &mut snarl));
                let error = viewer.error.as_deref().expect("shared-node error");
                assert!(error.contains("<primary target>"));
                assert!(error.contains("<target archive>"));
            } else {
                assert_eq!(viewer.remove_snarl_nodes(&[shared], &mut snarl), 0);
                assert!(viewer.error.is_some());
            }
            assert!(snarl.get_node(shared).is_some());
        });

        assert_eq!(
            mapping::project_file::encode_pretty(&project).expect("project after blocked edit"),
            saved,
        );
        let after = outputs(&project);
        assert_eq!(after.primary, before.primary);
        assert_eq!(after.extras, before.extras);
    }
}

#[test]
fn named_canvas_disconnection_keeps_a_hidden_source_field_used_by_other_targets() {
    let mut project = three_output_project(Node::SourceField {
        path: vec!["input".to_string()],
        frame: None,
    });
    let before = outputs(&project);
    assert_value(&before.primary, "source value");
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
    let target = snarl.insert_node(egui::pos2(400.0, 0.0), CanvasNode::TargetBlock(0));
    let from_id = OutPinId {
        node: source,
        output: 0,
    };
    let to_id = InPinId {
        node: target,
        input: 0,
    };
    snarl.connect(from_id, to_id);
    let from = snarl.out_pin(from_id);
    let to = snarl.in_pin(to_id);

    edit_named_target(&mut project, 0, |viewer| {
        // Source fields represented by endpoint wires are not graph nodes on
        // the canvas. Removing the active wire still invokes orphan cleanup.
        assert!(snarl.nodes().all(|node| *node != CanvasNode::Graph(SHARED)));
        viewer.disconnect(&from, &to, &mut snarl);
        assert!(viewer.root_scope.bindings.is_empty());
        assert!(viewer.graph.nodes.contains_key(&SHARED));
        assert!(snarl.in_pin(to_id).remotes.is_empty());
    });

    let after = outputs(&project);
    assert_eq!(after.primary, before.primary);
    assert_eq!(after.extras[1], before.extras[1]);
    assert_eq!(project.root.bindings[0].node, SHARED);
    assert_eq!(project.extra_targets[1].root.bindings[0].node, SHARED);
}

#[test]
fn deleting_an_unshared_named_function_keeps_its_other_target_source_dependency() {
    let mut project = three_output_project(Node::SourceField {
        path: vec!["input".to_string()],
        frame: None,
    });
    project.graph.nodes.insert(
        88,
        Node::Call {
            function: "upper".to_string(),
            args: vec![SHARED],
        },
    );
    project.extra_targets[0].root = bound_root(88);
    let before = outputs(&project);
    assert_value(&before.extras[0].instance, "SOURCE VALUE");
    let mut snarl = Snarl::new();
    let function = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(88));

    edit_named_target(&mut project, 0, |viewer| {
        assert!(viewer.remove_graph_node(88, function, &mut snarl));
        assert!(viewer.error.is_none());
        assert!(viewer.root_scope.bindings.is_empty());
        assert!(!viewer.graph.nodes.contains_key(&88));
        assert!(viewer.graph.nodes.contains_key(&SHARED));
    });

    let after = outputs(&project);
    assert_eq!(after.primary, before.primary);
    assert_eq!(after.extras[1], before.extras[1]);
}

#[test]
fn named_canvas_unshared_delete_disconnects_ordinary_graph_consumers() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared value".to_string()),
    });
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::String("private value".to_string()),
        },
    );
    project.graph.nodes.insert(
        89,
        Node::Call {
            function: "upper".to_string(),
            args: vec![88],
        },
    );
    project.extra_targets[0].root = bound_root(88);
    let before = outputs(&project);
    assert_value(&before.extras[0].instance, "private value");
    let mut snarl = Snarl::new();
    let private = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(88));
    let consumer = snarl.insert_node(egui::pos2(300.0, 0.0), CanvasNode::Graph(89));
    let consumer_pin = InPinId {
        node: consumer,
        input: 0,
    };
    snarl.connect(
        OutPinId {
            node: private,
            output: 0,
        },
        consumer_pin,
    );

    edit_named_target(&mut project, 0, |viewer| {
        assert_eq!(viewer.remove_snarl_nodes(&[private], &mut snarl), 1);
        assert!(viewer.error.is_none());
        assert!(viewer.root_scope.bindings.is_empty());
        assert!(!viewer.graph.nodes.contains_key(&88));
        let Some(Node::Call { args, .. }) = viewer.graph.nodes.get(&89) else {
            panic!("ordinary consumer should remain on the canvas");
        };
        assert_eq!(args.len(), 1);
        assert!(matches!(
            viewer.graph.nodes.get(&args[0]),
            Some(Node::Unconnected)
        ));
        assert!(snarl.in_pin(consumer_pin).remotes.is_empty());
    });

    let after = outputs(&project);
    assert_eq!(after.primary, before.primary);
    assert_eq!(after.extras[1], before.extras[1]);
}

#[test]
fn inactive_target_protection_survives_identical_active_scope_labels() {
    for (scope_name, protected_target) in
        [("<primary target>", None), ("<target archive>", Some(1))]
    {
        let mut project = three_output_project(Node::Const {
            value: Value::String("shared value".to_string()),
        });
        project.graph.nodes.insert(
            88,
            Node::Const {
                value: Value::String("independent value".to_string()),
            },
        );
        if protected_target.is_some() {
            project.root = bound_root(88);
        } else {
            project.extra_targets[1].root = bound_root(88);
        }
        project.extra_targets[0].schema =
            SchemaNode::group("audit", vec![output_schema(scope_name)]);
        let mut child = bound_root(SHARED);
        child.target_field = scope_name.to_string();
        project.extra_targets[0].root = Scope {
            children: vec![child],
            ..Scope::default()
        };
        let before = outputs(&project);
        let saved = mapping::project_file::encode_pretty(&project).expect("snapshot");
        let mut snarl = Snarl::new();
        let node = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(SHARED));

        edit_named_target(&mut project, 0, |viewer| {
            // This is also the active child's removable binding label. Its
            // text must not erase the independently protected reference.
            assert_eq!(
                viewer.blocking_references_to(SHARED),
                vec![format!("scope {scope_name} binding value")]
            );
            assert!(!viewer.remove_graph_node(SHARED, node, &mut snarl));
        });

        assert_eq!(
            mapping::project_file::encode_pretty(&project).expect("unchanged project"),
            saved
        );
        let after = outputs(&project);
        assert_eq!(after.primary, before.primary);
        assert_eq!(after.extras, before.extras);
    }
}

fn assert_valid(project: &Project) {
    let issues = engine::validate(project);
    assert!(
        issues.is_empty(),
        "valid protected-control mapping: {issues:#?}"
    );
}

fn assert_named_control_blocks(project: &mut Project, node: NodeId, label: &str) {
    let saved = mapping::project_file::encode_pretty(project).expect("control snapshot");
    let mut snarl = Snarl::new();
    let canvas = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(node));
    edit_named_target(project, 0, |viewer| {
        assert!(
            viewer
                .blocking_references_to(node)
                .contains(&label.to_string())
        );
        assert!(!viewer.remove_graph_node(node, canvas, &mut snarl));
        assert!(
            viewer
                .error
                .as_deref()
                .is_some_and(|error| error.contains(label))
        );
    });
    assert_eq!(
        mapping::project_file::encode_pretty(project).expect("blocked control edit"),
        saved
    );
    assert_valid(project);
}

fn row_source_schema() -> SchemaNode {
    SchemaNode::group(
        "source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("input", ScalarType::String),
                    SchemaNode::scalar("a", ScalarType::Int),
                    SchemaNode::scalar("b", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}

fn rows_source() -> Instance {
    Instance::Group(vec![(
        "Rows".to_string(),
        Instance::Repeated(
            [2, 1]
                .map(|b| {
                    Instance::Group(vec![
                        (
                            "input".to_string(),
                            Instance::Scalar(Value::String("first.xml".to_string())),
                        ),
                        ("a".to_string(), Instance::Scalar(Value::Int(0))),
                        ("b".to_string(), Instance::Scalar(Value::Int(b))),
                    ])
                })
                .into_iter()
                .collect(),
        ),
    )])
}

#[test]
fn secondary_sort_key_remains_owned_when_also_bound_to_the_active_target() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.source = row_source_schema();
    project.graph.nodes.insert(
        88,
        Node::SourceField {
            path: vec!["Rows".to_string(), "a".to_string()],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        89,
        Node::SourceField {
            path: vec!["Rows".to_string(), "b".to_string()],
            frame: None,
        },
    );
    project.extra_targets[0].schema =
        SchemaNode::group("audit", vec![SchemaNode::scalar("value", ScalarType::Int)]).repeating();
    project.extra_targets[0].root = Scope {
        iteration: ScopeIteration::Source(vec!["Rows".to_string()]),
        sort_by: Some(88),
        sort_then_by: vec![SortKey {
            node: 89,
            descending: false,
        }],
        bindings: bound_root(89).bindings,
        ..Scope::default()
    };
    assert_valid(&project);
    let before = engine::run_outputs(&project, &rows_source()).expect("sorted outputs");
    assert_eq!(
        before.extras[0].instance,
        Instance::Repeated(
            [1, 2]
                .map(|value| {
                    Instance::Group(vec![(
                        "value".to_string(),
                        Instance::Scalar(Value::Int(value)),
                    )])
                })
                .into_iter()
                .collect()
        )
    );
    assert_named_control_blocks(&mut project, 89, "root scope sort key 2");
    let after = engine::run_outputs(&project, &rows_source()).expect("retained sorting");
    assert_eq!(after.extras, before.extras);
}

#[test]
fn scalar_construction_expression_remains_owned_by_the_named_target() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::String("scalar output".to_string()),
        },
    );
    project.extra_targets[0].schema = SchemaNode::scalar("audit", ScalarType::String);
    project.extra_targets[0].root = Scope {
        construction: ScopeConstruction::Scalar { value: 88 },
        ..Scope::default()
    };
    let before = outputs(&project);
    assert_eq!(
        before.extras[0].instance,
        Instance::Scalar(Value::String("scalar output".to_string()))
    );
    assert_named_control_blocks(&mut project, 88, "root scope scalar value");
    assert_eq!(outputs(&project).extras, before.extras);
}

#[test]
fn dynamic_document_output_path_remains_owned_by_the_named_target() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.source = row_source_schema();
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::String("record.xml".to_string()),
        },
    );
    project.extra_targets[0].options.xml_document = true;
    project.extra_targets[0].root.iteration = ScopeIteration::DynamicDocuments {
        source: vec!["Rows".to_string()],
        output_path: 88,
    };
    let input = Instance::Group(vec![(
        "Rows".to_string(),
        Instance::Repeated(vec![Instance::Group(vec![
            (
                "input".to_string(),
                Instance::Scalar(Value::String("first.xml".to_string())),
            ),
            ("a".to_string(), Instance::Scalar(Value::Int(0))),
            ("b".to_string(), Instance::Scalar(Value::Int(1))),
        ])]),
    )]);
    assert_valid(&project);
    let before = engine::run_outputs(&project, &input).expect("named document output");
    let Instance::DocumentSet(documents) = &before.extras[0].instance else {
        panic!("named output should retain member identities");
    };
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].path(), "record.xml");
    assert_value(documents[0].value(), "shared");
    assert_named_control_blocks(&mut project, 88, "root scope dynamic target path");
    let after = engine::run_outputs(&project, &input).expect("retained member path");
    assert_eq!(after.extras, before.extras);
}

#[test]
fn sequence_aggregate_item_ownership_blocks_deletion_without_expression_wires() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::Int(3),
        },
    );
    project.graph.nodes.insert(
        89,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.graph.nodes.insert(
        90,
        Node::SequenceAggregate {
            function: AggregateOp::Sum,
            sequence: SequenceExpr::Generate {
                from: None,
                to: 88,
                item: 89,
            },
            predicate: None,
            expression: None,
            arg: None,
        },
    );
    project.extra_targets[0].schema =
        SchemaNode::group("audit", vec![SchemaNode::scalar("value", ScalarType::Int)]);
    project.extra_targets[0].root = bound_root(90);
    let before = outputs(&project);
    assert_eq!(
        before.extras[0].instance.field("value"),
        Some(&Instance::Scalar(Value::Int(6)))
    );
    assert_named_control_blocks(&mut project, 89, "graph node 90 sequence item");
    edit_named_target(&mut project, 0, |viewer| {
        let mut snarl = Snarl::new();
        viewer.remove_orphaned_input(89, &mut snarl);
        assert!(viewer.graph.nodes.contains_key(&89));
    });
    assert_eq!(outputs(&project).extras, before.extras);
}

#[test]
fn failure_predicate_message_and_generated_item_remain_owned_on_named_canvases() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    project.graph.nodes.insert(
        89,
        Node::Const {
            value: Value::String("selected failure".to_string()),
        },
    );
    project.graph.nodes.insert(
        90,
        Node::Const {
            value: Value::Int(2),
        },
    );
    project.graph.nodes.insert(
        91,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 90,
                item: 91,
            },
        },
        selection: FailureSelection::WhenTrue { predicate: 88 },
        message: Some(89),
    });
    let before = outputs(&project);
    for (node, label) in [
        (88, "failure rule 1 predicate"),
        (89, "failure rule 1 message"),
        (90, "failure rule 1 sequence input"),
        (91, "failure rule 1 sequence item"),
    ] {
        assert_named_control_blocks(&mut project, node, label);
    }
    edit_named_target(&mut project, 0, |viewer| {
        let mut snarl = Snarl::new();
        viewer.remove_orphaned_input(91, &mut snarl);
        assert!(viewer.graph.nodes.contains_key(&91));
    });
    assert_eq!(outputs(&project).extras, before.extras);
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    assert_valid(&project);
    assert!(
        matches!(engine::run_outputs(&project, &source()), Err(engine::EngineError::MappingFailure {
        rule: 1, message: Some(message),
    }) if message == "selected failure")
    );
}

#[test]
fn failure_message_source_field_is_not_removed_as_an_orphan() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.source = row_source_schema();
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    project.graph.nodes.insert(
        89,
        Node::SourceField {
            path: vec!["Rows".to_string(), "input".to_string()],
            frame: None,
        },
    );
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Rows".to_string()],
        },
        selection: FailureSelection::WhenTrue { predicate: 88 },
        message: Some(89),
    });
    assert_valid(&project);
    engine::run_outputs(&project, &rows_source()).expect("unselected failure rule");
    edit_named_target(&mut project, 0, |viewer| {
        let mut snarl = Snarl::new();
        viewer.remove_orphaned_input(89, &mut snarl);
        assert!(viewer.graph.nodes.contains_key(&89));
    });
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    assert_valid(&project);
    assert!(
        matches!(engine::run_outputs(&project, &rows_source()), Err(engine::EngineError::MappingFailure {
        rule: 1, message: Some(message),
    }) if message == "first.xml")
    );
}

struct FixtureLoader;

impl engine::DynamicSourceLoader for FixtureLoader {
    fn load(&self, source: &str, path: &str) -> Result<std::sync::Arc<Instance>, String> {
        if source != "document" || path != "first.xml" {
            return Err(format!("unexpected source {source} or path {path}"));
        }
        Ok(std::sync::Arc::new(Instance::Group(vec![(
            "Item".to_string(),
            Instance::Repeated(vec![Instance::Group(vec![(
                "value".to_string(),
                Instance::Scalar(Value::String("loaded value".to_string())),
            )])]),
        )])))
    }
}

#[test]
fn dynamic_named_source_path_is_protected_during_delete_and_orphan_cleanup() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.source = row_source_schema();
    project.graph.nodes.insert(
        88,
        Node::SourceField {
            path: vec!["input".to_string()],
            frame: Some(vec!["Rows".to_string()]),
        },
    );
    project.graph.nodes.insert(
        89,
        Node::SourceField {
            path: vec!["value".to_string()],
            frame: Some(vec!["document".to_string(), "Item".to_string()]),
        },
    );
    // Removing this unbound consumer invokes orphan cleanup on the driver
    // expression, which is still owned by the document's path metadata.
    project.graph.nodes.insert(
        90,
        Node::Call {
            function: "upper".to_string(),
            args: vec![88],
        },
    );
    project.extra_sources.push(NamedSource {
        name: "document".to_string(),
        path: String::new(),
        schema: SchemaNode::group("Document", vec![output_schema("Item").repeating()]),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 88,
            iteration: vec!["Rows".to_string()],
        }),
    });
    project.extra_targets[0].schema =
        SchemaNode::group("audit", vec![output_schema("row").repeating()]);
    let mut row_scope = bound_root(89);
    row_scope.target_field = "row".to_string();
    row_scope.iteration = ScopeIteration::Source(vec!["document".to_string(), "Item".to_string()]);
    project.extra_targets[0].root = Scope {
        children: vec![row_scope],
        ..Scope::default()
    };
    assert_valid(&project);
    let context = engine::ExecutionContext::new(std::path::Path::new("mapping.json"))
        .with_dynamic_source_loader(&FixtureLoader);
    let before = engine::run_outputs_with_sources_and_context(
        &project,
        &rows_source(),
        Vec::new(),
        &context,
    )
    .expect("loaded named target");
    let rows = before.extras[0].instance.field("row").expect("loaded rows");
    let Instance::Repeated(rows) = rows else {
        panic!("repeating loaded rows");
    };
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_value(row, "loaded value");
    }
    assert_named_control_blocks(&mut project, 88, "source document dynamic path");
    edit_named_target(&mut project, 0, |viewer| {
        let mut snarl = Snarl::new();
        let consumer = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(90));
        assert!(viewer.remove_graph_node(90, consumer, &mut snarl));
        assert!(viewer.graph.nodes.contains_key(&88));
    });
    assert_valid(&project);
    let after = engine::run_outputs_with_sources_and_context(
        &project,
        &rows_source(),
        Vec::new(),
        &context,
    )
    .expect("retained dynamic path and loading");
    assert_eq!(after.primary, before.primary);
    assert_eq!(after.extras, before.extras);
}

#[test]
fn active_control_protection_survives_a_static_binding_with_the_same_label() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.source = row_source_schema();
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    project.extra_targets[0].schema = SchemaNode::group(
        "audit",
        vec![
            SchemaNode::group("X", vec![SchemaNode::scalar("filter", ScalarType::Bool)]),
            output_schema("X binding").repeating(),
        ],
    );
    let mut first = Scope {
        target_field: "X".to_string(),
        ..Scope::default()
    };
    first.bindings.push(Binding {
        target_field: "filter".to_string(),
        node: 88,
    });
    let mut second = bound_root(SHARED);
    second.target_field = "X binding".to_string();
    second.iteration = ScopeIteration::Source(vec!["Rows".to_string()]);
    second.filter = Some(88);
    project.extra_targets[0].root = Scope {
        children: vec![first, second],
        ..Scope::default()
    };
    assert_valid(&project);
    let before = engine::run_outputs(&project, &rows_source()).expect("filtered output");
    assert_named_control_blocks(&mut project, 88, "scope X binding filter");
    let after = engine::run_outputs(&project, &rows_source()).expect("retained filter and binding");
    assert_eq!(after.extras, before.extras);
}

#[test]
fn active_dynamic_child_binding_remains_protected_and_keeps_its_output() {
    let mut project = three_output_project(Node::Const {
        value: Value::String("shared".to_string()),
    });
    project.graph.nodes.insert(
        88,
        Node::Const {
            value: Value::String("computed".to_string()),
        },
    );
    project.graph.nodes.insert(
        89,
        Node::Const {
            value: Value::String("child value".to_string()),
        },
    );
    project.extra_targets[0].schema = SchemaNode::group("audit", Vec::new())
        .with_dynamic_fields(output_schema("*"))
        .expect("open group target");
    project.extra_targets[0].root = Scope {
        dynamic_children: vec![mapping::DynamicChild {
            key: 88,
            scope: bound_root(89),
        }],
        ..Scope::default()
    };
    let before = outputs(&project);
    assert_value(
        before.extras[0]
            .instance
            .field("computed")
            .expect("computed child"),
        "child value",
    );
    assert_named_control_blocks(&mut project, 89, "scope <dynamic child 1> binding value");
    let after = outputs(&project);
    assert_eq!(after.primary, before.primary);
    assert_eq!(after.extras, before.extras);
}
