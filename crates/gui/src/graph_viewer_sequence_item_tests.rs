use super::*;
use crate::canvas::{source_blocks, target_blocks};
use egui_snarl::OutPinId;
use ir::{Instance, SchemaNode};
use mapping::{
    FailureIteration, FailureRule, FailureSelection, Project, ScopeIteration, ScopeSequence,
    SequenceExpr,
};

const ITEM: NodeId = 7;
const NAMED_ITEM: NodeId = 8;

fn sequence(item: NodeId) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input: 0,
        delimiter: 1,
        item,
    }
}

fn generated_scope(item: NodeId) -> Scope {
    Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(sequence(item)),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    }
}

fn project() -> Project {
    let mut project = crate::new_mapping::blank_project();
    project.source = SchemaNode::group("Source", Vec::new());
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            ITEM,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            NAMED_ITEM,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]
    .into_iter()
    .collect();
    project.root = Scope {
        children: vec![generated_scope(ITEM)],
        ..Scope::default()
    };
    project.extra_targets = vec![NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: Scope {
            children: vec![generated_scope(NAMED_ITEM)],
            ..Scope::default()
        },
    }];
    project
}

fn snapshot(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).expect("project snapshot")
}

fn outputs(project: &Project) -> engine::ExecutionOutputs {
    let issues = engine::validate(project);
    assert!(
        issues.is_empty(),
        "valid generated-item fixture: {issues:?}"
    );
    engine::run_outputs(project, &Instance::Group((Vec::new()).into())).expect("generated outputs")
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

/// Exercise the actual SourceField output widget; only the enclosing Snarl
/// placement is omitted so pointer coordinates stay deterministic.
fn raw_frame(
    project: &mut Project,
    named: bool,
    item: NodeId,
    context: &egui::Context,
    events: Vec<egui::Event>,
    isolated_function: bool,
) -> (egui::FullOutput, egui::Rect, String) {
    let source_blocks = source_blocks(&project.source);
    let target_blocks = target_blocks(&project.target);
    let source_paths = SourcePathCatalog::new(&project.source, &project.extra_sources);
    let inactive;
    let (root_scope, extra_targets) = if named {
        inactive = inactive_target_scopes(&project.root, &[], &[]);
        (&mut project.extra_targets[0].root, &[] as &[NamedTarget])
    } else {
        inactive = Vec::new();
        (&mut project.root, project.extra_targets.as_slice())
    };
    let mut endpoint_scroll = crate::canvas_endpoints::EndpointScrollState::default();
    let mut snarl = Snarl::new();
    let node = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(item));
    let pin = snarl.out_pin(OutPinId { node, output: 0 });
    let mut function_output = item;
    let mut rect = egui::Rect::NOTHING;
    let mut title = String::new();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = GraphViewer {
                graph: &mut project.graph,
                root_scope: &mut *root_scope,
                primary_root_authoring: false,
                extra_targets,
                inactive_target_scopes: &inactive,
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
                protected_output: isolated_function.then_some(item),
                function_output: isolated_function.then_some(&mut function_output),
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
            title = viewer.title(&CanvasNode::Graph(item));
            rect = ui
                .vertical(|ui| {
                    viewer.show_output(&pin, ui, &mut snarl);
                })
                .response
                .rect;
        },
    );
    (output, rect, title)
}

fn text_rect(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Rect> {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            Some(text.visual_bounding_rect())
        }
        egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, label)),
        _ => None,
    }
}

fn property_rect(output: &egui::FullOutput, project: &Project, item: NodeId) -> Option<egui::Rect> {
    let label_rect = |label: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, label))
    };
    if let Some(rect) = label_rect("Generated item (read-only)") {
        return Some(rect);
    }
    let Some(Node::SourceField { path, .. }) = project.graph.nodes.get(&item) else {
        return None;
    };
    if !path.is_empty() {
        return label_rect(&path.join("/"));
    }
    // An empty TextEdit has no text galley. Target its actual painted field,
    // not the requested width or the properties button's enclosing row.
    fn empty_field(shape: &egui::epaint::Shape) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.rect.width() - SOURCE_FIELD_EDIT_WIDTH).abs() <= 8.0
                    && (10.0..=40.0).contains(&rect.rect.height()) =>
            {
                Some(rect.rect)
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(empty_field),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| empty_field(&shape.shape))
}

fn frame(
    project: &mut Project,
    named: bool,
    item: NodeId,
    context: &egui::Context,
    events: Vec<egui::Event>,
    isolated_function: bool,
) -> (egui::FullOutput, egui::Rect, String) {
    let (mut output, _, mut title) =
        raw_frame(project, named, item, context, events, isolated_function);
    if property_rect(&output, project, item).is_none() {
        let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
        let position = output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, &pencil))
            .expect("SourceField exposes its existing properties")
            .center();
        for pressed in [true, false] {
            (output, _, title) = raw_frame(
                project,
                named,
                item,
                context,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                isolated_function,
            );
        }
        for _ in 0..3 {
            (output, _, title) =
                raw_frame(project, named, item, context, Vec::new(), isolated_function);
        }
    }
    eprintln!(
        "Sequence-item property originals: title={title:?}; project={}; shapes={:?}",
        snapshot(project),
        output.shapes
    );
    let rect = property_rect(&output, project, item)
        .expect("actual SourceField editor or read-only item label is rendered");
    (output, rect, title)
}

fn labels(shape: &egui::epaint::Shape, output: &mut Vec<String>) {
    match shape {
        egui::epaint::Shape::Text(text) => output.push(text.galley.text().to_string()),
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, output);
            }
        }
        _ => {}
    }
}

fn rendered_labels(output: &egui::FullOutput) -> Vec<String> {
    let mut found = Vec::new();
    for shape in &output.shapes {
        labels(&shape.shape, &mut found);
    }
    found
}

fn attempt_path_edit(
    project: &mut Project,
    named: bool,
    item: NodeId,
    context: &egui::Context,
    isolated_function: bool,
) {
    let (_, rect, _) = frame(project, named, item, context, Vec::new(), isolated_function);
    // The first row is the path editor for ordinary fields and the read-only
    // item label for owned fields. Both are clicked through real egui input.
    let position = rect.left_top() + egui::vec2(35.0, 10.0);
    for pressed in [true, false] {
        frame(
            project,
            named,
            item,
            context,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            isolated_function,
        );
    }
    frame(
        project,
        named,
        item,
        context,
        vec![egui::Event::Text("edited".into())],
        isolated_function,
    );
}

#[test]
fn scope_owned_items_stay_read_only_on_primary_and_named_canvases() {
    for named in [false, true] {
        for item in [ITEM, NAMED_ITEM] {
            let mut project = project();
            let context = context();
            let expected = outputs(&project);
            let saved = snapshot(&project);
            let (output, _, title) = frame(&mut project, named, item, &context, Vec::new(), false);
            assert_eq!(title, format!("Generated item #{item}"));
            let labels = rendered_labels(&output);
            assert!(
                labels
                    .iter()
                    .any(|label| label == "Generated item (read-only)")
            );
            attempt_path_edit(&mut project, named, item, &context, false);
            assert_eq!(
                snapshot(&project),
                saved,
                "owned field changed on {named}/{item}"
            );
            let actual = outputs(&project);
            assert_eq!(actual.primary, expected.primary);
            assert_eq!(actual.extras, expected.extras);
            assert_eq!(
                mapping::project_file::decode_str(&saved)
                    .map(|saved| snapshot(&saved))
                    .expect("saved item metadata"),
                saved,
            );
        }
    }
}

#[test]
fn malformed_scope_owned_fields_remain_visible_and_unchanged() {
    for named in [false, true] {
        let mut project = project();
        project.graph.nodes.insert(
            ITEM,
            Node::SourceField {
                path: vec!["kept".into(), "original".into()],
                frame: Some(vec!["outer".into()]),
            },
        );
        let saved = snapshot(&project);
        let before_issues = engine::validate(&project);
        assert!(
            before_issues
                .iter()
                .any(|issue| issue.message.contains("sequence item must reference"))
        );
        let context = context();
        for _ in 0..3 {
            let (output, _, _) = frame(&mut project, named, ITEM, &context, Vec::new(), false);
            assert!(
                rendered_labels(&output)
                    .iter()
                    .any(|label| label == "Invalid item shape (unchanged)")
            );
        }
        attempt_path_edit(&mut project, named, ITEM, &context, false);
        assert_eq!(snapshot(&project), saved);
        assert_eq!(engine::validate(&project), before_issues);
    }
}

#[test]
fn graph_reducer_and_failure_items_cannot_be_edited_even_when_malformed() {
    for kind in 0..4 {
        for malformed in [false, true] {
            let mut project = project();
            project.root.children.clear();
            let expected_label = match kind {
                0 => {
                    project.graph.nodes.insert(
                        31,
                        Node::SequenceExists {
                            sequence: sequence(ITEM),
                            predicate: 3,
                        },
                    );
                    "graph node 31 (sequence-exists)"
                }
                1 => {
                    project.graph.nodes.insert(
                        32,
                        Node::SequenceItemAt {
                            sequence: sequence(ITEM),
                            index: 2,
                        },
                    );
                    "graph node 32 (sequence-item-at)"
                }
                2 => {
                    project.graph.nodes.insert(
                        33,
                        Node::SequenceAggregate {
                            function: AggregateOp::Count,
                            sequence: sequence(ITEM),
                            predicate: None,
                            expression: None,
                            arg: None,
                        },
                    );
                    "graph node 33 (sequence-aggregate)"
                }
                _ => {
                    project.failure_rules.push(FailureRule {
                        iteration: FailureIteration::Sequence {
                            sequence: sequence(ITEM),
                        },
                        selection: FailureSelection::WhenFalse { predicate: 3 },
                        message: None,
                    });
                    "failure rule 1"
                }
            };
            if malformed {
                project.graph.nodes.insert(
                    ITEM,
                    Node::SourceField {
                        path: vec!["unchanged".into()],
                        frame: Some(Vec::new()),
                    },
                );
            }
            let saved = snapshot(&project);
            let context = context();
            let (output, _, _) = frame(&mut project, false, ITEM, &context, Vec::new(), false);
            let labels = rendered_labels(&output);
            assert!(
                labels.iter().any(|label| label == expected_label),
                "missing owner: {labels:?}"
            );
            assert_eq!(
                labels
                    .iter()
                    .any(|label| label == "Invalid item shape (unchanged)"),
                malformed
            );
            attempt_path_edit(&mut project, false, ITEM, &context, false);
            assert_eq!(
                snapshot(&project),
                saved,
                "changed reducer/failure item {kind}/{malformed}"
            );
        }
    }
}

#[test]
fn unowned_empty_fields_and_same_id_function_fields_remain_editable() {
    for isolated_function in [false, true] {
        let mut project = project();
        if !isolated_function {
            project.root.children.clear();
        }
        let context = context();
        let (output, _, _) = frame(
            &mut project,
            false,
            ITEM,
            &context,
            Vec::new(),
            isolated_function,
        );
        assert!(
            !rendered_labels(&output)
                .iter()
                .any(|label| label == "Generated item (read-only)")
        );
        attempt_path_edit(&mut project, false, ITEM, &context, isolated_function);
        assert!(
            matches!(project.graph.nodes.get(&ITEM), Some(Node::SourceField { path, frame: None }) if path == &["edited"])
        );
    }
}

#[test]
fn owner_metadata_keeps_duplicate_names_and_scope_routes_distinct() {
    let mut project = project();
    project.root.children.push(generated_scope(ITEM));
    project.extra_targets[0].name = "Same".into();
    project.extra_targets[0].root.children[0].set_sequence(Some(sequence(ITEM)));
    let mut other = project.extra_targets[0].clone();
    other.root = Scope {
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(
            generated_scope(ITEM),
            vec![generated_scope(ITEM)],
        )),
        dynamic_children: vec![mapping::DynamicChild {
            key: 0,
            scope: generated_scope(ITEM),
        }],
        ..Scope::default()
    };
    project.extra_targets.push(other);
    let owners = graph_references::sequence_item_owners(
        &project.graph,
        &project.root,
        &project.extra_targets,
        &[],
        Default::default(),
        ITEM,
    );
    assert_eq!(owners.len(), 6);
    let labels = owners.iter().map(|owner| owner.label()).collect::<Vec<_>>();
    assert_eq!(
        labels
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        labels.len()
    );
    assert!(labels.iter().any(|label| label.contains("child 1 'Rows'")));
    assert!(labels.iter().any(|label| label.contains("child 2 'Rows'")));
    assert!(
        labels
            .iter()
            .any(|label| label.contains("named target 1 'Same'"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("named target 2 'Same' / segment 2"))
    );
    assert!(labels.iter().any(|label| label.contains("dynamic child 1")));
    let context = context();
    let saved = snapshot(&project);
    let (output, _, _) = frame(&mut project, false, ITEM, &context, Vec::new(), false);
    assert!(
        rendered_labels(&output)
            .iter()
            .any(|label| label == "Multiple sequence owners (unchanged)")
    );
    attempt_path_edit(&mut project, false, ITEM, &context, false);
    assert_eq!(snapshot(&project), saved);
}

#[test]
fn inactive_item_owners_retain_actual_named_slots_around_the_active_target() {
    let mut project = project();
    let mut named = project.extra_targets[0].clone();
    named.name = "Same".into();
    named.root.children[0].set_sequence(Some(sequence(ITEM)));
    project.extra_targets = vec![named.clone(), named.clone(), named];
    let (before, current_and_after) = project.extra_targets.split_at(1);
    let (current, after) = current_and_after.split_first().expect("active target");
    let inactive = graph_references::inactive_target_scopes(&project.root, before, after);
    let owners = graph_references::sequence_item_owners(
        &project.graph,
        &current.root,
        &[],
        &inactive,
        Default::default(),
        ITEM,
    );
    let mut slots = owners
        .iter()
        .filter_map(|owner| match owner {
            graph_sequence_ownership::SequenceItemOwner::Scope {
                target:
                    graph_sequence_ownership::TargetOwner::InactiveNamed {
                        index,
                        name: "Same",
                    },
                ..
            } => Some(*index),
            _ => None,
        })
        .collect::<Vec<_>>();
    slots.sort_unstable();
    assert_eq!(slots, [0, 2]);
    assert!(owners.iter().any(|owner| matches!(
        owner,
        graph_sequence_ownership::SequenceItemOwner::Scope {
            target: graph_sequence_ownership::TargetOwner::CurrentNamed,
            ..
        }
    )));
    let labels = owners.iter().map(|owner| owner.label()).collect::<Vec<_>>();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("other named target 1 'Same'"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("other named target 3 'Same'"))
    );
    assert!(
        !labels
            .iter()
            .any(|label| label.contains("other named target 2 'Same'"))
    );
}

const REUSE_ITEMS: [NodeId; 8] = [ITEM, NAMED_ITEM, 9, 10, 11, 12, 13, 14];
const REUSE_CALL: NodeId = 90;
const REUSE_EMPTY: NodeId = 91;
const REUSE_FIELD: NodeId = 50;

fn physical_field(frame: Option<Vec<String>>, name: &str) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame,
    }
}

fn source_reuse_project() -> Project {
    let mut project = project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Input", ScalarType::String),
            SchemaNode::scalar("Other", ScalarType::String),
            SchemaNode::group(
                "Records",
                vec![
                    SchemaNode::scalar("Input", ScalarType::String),
                    SchemaNode::scalar("Other", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("Output", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    project.extra_targets = ["Before", "Current", "After"]
        .into_iter()
        .zip([NAMED_ITEM, 9, 10])
        .map(|(name, item)| NamedTarget {
            name: name.into(),
            path: None,
            schema: project.target.clone(),
            options: Default::default(),
            root: Scope {
                children: vec![generated_scope(item)],
                ..Scope::default()
            },
        })
        .collect();
    for item in REUSE_ITEMS {
        project.graph.nodes.insert(
            item,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
    }
    project.graph.nodes.extend([
        (
            31,
            Node::SequenceExists {
                sequence: sequence(11),
                predicate: 3,
            },
        ),
        (
            32,
            Node::SequenceItemAt {
                sequence: sequence(12),
                index: 2,
            },
        ),
        (
            33,
            Node::SequenceAggregate {
                function: AggregateOp::Count,
                sequence: sequence(13),
                predicate: None,
                expression: None,
                arg: None,
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String("!".into()),
            },
        ),
        (20, physical_field(Some(vec!["Records".into()]), "Input")),
        (21, physical_field(None, "Other")),
        (
            REUSE_CALL,
            Node::Call {
                function: "concat".into(),
                args: vec![REUSE_EMPTY, 4],
            },
        ),
        (REUSE_EMPTY, Node::Unconnected),
    ]);
    project.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: sequence(14),
        },
        selection: FailureSelection::WhenFalse { predicate: 3 },
        message: None,
    }];
    project
}

fn reuse_root(project: &mut Project, named: bool) -> &mut Scope {
    if named {
        &mut project.extra_targets[1].root
    } else {
        &mut project.root
    }
}

/// Use the same active/inactive borrowing as a middle named-target canvas,
/// then call the real viewer connection handler with Snarl endpoint pins.
fn connect_physical_source(
    project: &mut Project,
    named: bool,
    to_function: bool,
    framed: bool,
    isolated_function: bool,
) -> NodeId {
    connect_physical_source_attempt(project, named, to_function, framed, isolated_function)
        .expect("physical endpoint connection")
}

fn connect_physical_source_attempt(
    project: &mut Project,
    named: bool,
    to_function: bool,
    framed: bool,
    isolated_function: bool,
) -> Result<NodeId, String> {
    let source_blocks = source_blocks(&project.source);
    let target_blocks = target_blocks(&project.target);
    let source_paths = SourcePathCatalog::new(&project.source, &project.extra_sources);
    let frame = framed.then(|| vec!["Records".into()]);
    let (source_block, source_pin) = source_blocks
        .iter()
        .enumerate()
        .find_map(|(block, data)| {
            data.leaves
                .iter()
                .position(|leaf| leaf.path == ["Input"] && leaf.frame == frame)
                .map(|pin| (block, pin))
        })
        .expect("physical source leaf");
    let (target_block, target_pin) = target_blocks
        .iter()
        .enumerate()
        .find_map(|(block, data)| {
            data.leaves
                .iter()
                .position(|leaf| leaf.field == "Output" && leaf.chain.is_empty())
                .map(|pin| (block, pin))
        })
        .expect("root output leaf");
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::SourceBlock(source_block));
    let target = snarl.insert_node(
        egui::pos2(400.0, 0.0),
        CanvasNode::TargetBlock(target_block),
    );
    let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(REUSE_CALL));
    let from_id = OutPinId {
        node: source,
        output: source_pin,
    };
    let target_id = InPinId {
        node: target,
        input: target_pin,
    };
    let call_input = InPinId {
        node: call,
        input: 0,
    };
    // Recreate pre-existing ordinary wires for displacement/error tests.
    // These fields live on this physical Source endpoint block.
    let old_source_pin = |node: NodeId| {
        let Node::SourceField { path, frame } = project.graph.nodes.get(&node)? else {
            return None;
        };
        source_blocks[source_block]
            .leaves
            .iter()
            .position(|leaf| &leaf.path == path && &leaf.frame == frame)
            .map(|output| OutPinId {
                node: source,
                output,
            })
    };
    let previous_root = if named {
        &project.extra_targets[1].root
    } else {
        &project.root
    };
    if let Some(from) = previous_root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Output")
        .and_then(|binding| old_source_pin(binding.node))
    {
        snarl.connect(from, target_id);
    }
    if let Some(Node::Call { args, .. }) = project.graph.nodes.get(&REUSE_CALL)
        && let Some(from) = args.first().copied().and_then(old_source_pin)
    {
        snarl.connect(from, call_input);
    }
    let mut endpoint_scroll = crate::canvas_endpoints::EndpointScrollState::default();
    let inactive;
    let (root_scope, extra_targets) = if named {
        let (before, current_and_after) = project.extra_targets.split_at_mut(1);
        let (current, after) = current_and_after
            .split_first_mut()
            .expect("active named target");
        inactive = inactive_target_scopes(&project.root, before, after);
        (&mut current.root, &[] as &[NamedTarget])
    } else {
        inactive = Vec::new();
        (&mut project.root, project.extra_targets.as_slice())
    };
    let mut function_output = REUSE_CALL;
    let mut viewer = GraphViewer {
        primary_root_authoring: false,
        graph: &mut project.graph,
        root_scope,
        extra_targets,
        inactive_target_scopes: &inactive,
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
        protected_output: isolated_function.then_some(REUSE_CALL),
        function_output: isolated_function.then_some(&mut function_output),
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
    let to_id = if to_function { call_input } else { target_id };
    let before_wires = snarl.wires().collect::<std::collections::BTreeSet<_>>();
    viewer.connect(&snarl.out_pin(from_id), &snarl.in_pin(to_id), &mut snarl);
    if let Some(error) = viewer.error.take() {
        assert!(
            !before_wires.is_empty(),
            "exhaustion fixture must begin with existing ordinary wires"
        );
        assert_eq!(
            snarl.wires().collect::<std::collections::BTreeSet<_>>(),
            before_wires
        );
        return Err(error);
    }
    let field = if to_function {
        let Node::Call { args, .. } = &viewer.graph.nodes[&REUSE_CALL] else {
            panic!("call")
        };
        args[0]
    } else {
        viewer
            .root_scope
            .bindings
            .iter()
            .find(|binding| binding.target_field == "Output")
            .expect("physical output binding")
            .node
    };
    assert!(snarl.wires().any(|wire| wire == (from_id, to_id)));
    if to_function && !isolated_function {
        let call_output = OutPinId {
            node: call,
            output: 0,
        };
        viewer.connect(
            &snarl.out_pin(call_output),
            &snarl.in_pin(target_id),
            &mut snarl,
        );
        assert_eq!(viewer.error, None, "function output connection");
        assert!(snarl.wires().any(|wire| wire == (call_output, target_id)));
        assert_eq!(snarl.wires().count(), 2);
    } else {
        assert_eq!(snarl.wires().count(), 1);
    }
    assert!(
        !snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
    Ok(field)
}

fn repaired_physical_outputs(project: &Project, named: bool, to_function: bool) {
    // The stored fixture intentionally has a malformed generated item. Repair
    // only a clone to exercise the new ordinary wire as an executable mapping.
    let mut executable = project.clone();
    for item in crate::graph_viewer::project_sequence_item_ids(&executable) {
        executable.graph.nodes.insert(
            item,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
    }
    let issues = engine::validate(&executable);
    assert!(issues.is_empty(), "repaired physical mapping: {issues:?}");
    let source = Instance::Group(
        (vec![
            (
                "Input".into(),
                Instance::Scalar(Value::String("physical".into())),
            ),
            (
                "Other".into(),
                Instance::Scalar(Value::String("other".into())),
            ),
            (
                "Records".into(),
                Instance::Repeated(vec![Instance::Group(
                    (vec![
                        (
                            "Input".into(),
                            Instance::Scalar(Value::String("framed".into())),
                        ),
                        (
                            "Other".into(),
                            Instance::Scalar(Value::String("other framed".into())),
                        ),
                    ])
                    .into(),
                )]),
            ),
        ])
        .into(),
    );
    let output = engine::run_outputs(&executable, &source).expect("physical mapping output");
    let rows = Instance::Repeated(vec![
        Instance::Group(
            (vec![("Value".into(), Instance::Scalar(Value::String("a".into())))]).into(),
        ),
        Instance::Group(
            (vec![("Value".into(), Instance::Scalar(Value::String("b".into())))]).into(),
        ),
    ]);
    for (index, instance) in std::iter::once(&output.primary)
        .chain(output.extras.iter().map(|output| &output.instance))
        .enumerate()
    {
        let Instance::Group(fields) = instance else {
            panic!("group output")
        };
        assert_eq!(
            fields
                .iter()
                .find(|(name, _)| name == "Rows")
                .map(|(_, value)| value),
            Some(&rows)
        );
        let value = fields
            .iter()
            .find(|(name, _)| name == "Output")
            .map(|(_, value)| value);
        if index == if named { 2 } else { 0 } {
            let expected = Instance::Scalar(Value::String(
                if to_function { "physical!" } else { "physical" }.into(),
            ));
            assert_eq!(value, Some(&expected));
        } else {
            assert!(value.is_none() || value == Some(&Instance::Scalar(Value::Null)));
        }
    }
}

fn source_reuse_matrix(to_function: bool) {
    for named in [false, true] {
        for item in REUSE_ITEMS {
            for preexisting in [false, true] {
                let mut project = source_reuse_project();
                project
                    .graph
                    .nodes
                    .insert(item, physical_field(None, "Input"));
                if preexisting {
                    project
                        .graph
                        .nodes
                        .insert(REUSE_FIELD, physical_field(None, "Input"));
                }
                let mut expected = project.clone();
                let field = connect_physical_source(&mut project, named, to_function, false, false);
                assert!(
                    !REUSE_ITEMS.contains(&field),
                    "reused owned item {item}/{named}/{to_function}/{preexisting}"
                );
                assert!(matches!(
                    project.graph.nodes.get(&field),
                    Some(Node::SourceField { path, frame: None }) if path == &["Input"]
                ));
                if preexisting {
                    assert_eq!(field, REUSE_FIELD);
                } else {
                    assert!(!expected.graph.nodes.contains_key(&field));
                    expected
                        .graph
                        .nodes
                        .insert(field, physical_field(None, "Input"));
                }
                if to_function {
                    expected.graph.nodes.insert(
                        REUSE_CALL,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![field, 4],
                        },
                    );
                    expected.graph.nodes.remove(&REUSE_EMPTY);
                }
                reuse_root(&mut expected, named).bindings.push(Binding {
                    target_field: "Output".into(),
                    node: if to_function { REUSE_CALL } else { field },
                });
                assert_eq!(
                    snapshot(&project),
                    snapshot(&expected),
                    "unexpected owner/graph/scope mutation {item}/{named}/{to_function}/{preexisting}"
                );
                repaired_physical_outputs(&project, named, to_function);
            }
        }
    }
}

#[test]
fn physical_source_to_target_excludes_all_generated_item_owners() {
    source_reuse_matrix(false);
}

#[test]
fn physical_source_to_function_excludes_all_generated_item_owners() {
    source_reuse_matrix(true);
}

#[test]
fn physical_source_reuse_preserves_exact_frame_and_relative_path() {
    for named in [false, true] {
        let mut project = source_reuse_project();
        let frame = Some(vec!["Records".into()]);
        project
            .graph
            .nodes
            .insert(ITEM, physical_field(frame.clone(), "Input"));
        project
            .graph
            .nodes
            .insert(20, physical_field(None, "Input"));
        project
            .graph
            .nodes
            .insert(21, physical_field(frame.clone(), "Other"));
        project
            .graph
            .nodes
            .insert(REUSE_FIELD, physical_field(frame, "Input"));
        let mut expected = project.clone();
        assert_eq!(
            connect_physical_source(&mut project, named, false, true, false),
            REUSE_FIELD
        );
        reuse_root(&mut expected, named).bindings.push(Binding {
            target_field: "Output".into(),
            node: REUSE_FIELD,
        });
        assert_eq!(snapshot(&project), snapshot(&expected));
    }
}

#[test]
fn isolated_function_source_reuse_ignores_project_item_id_collisions() {
    for named in [false, true] {
        let mut project = source_reuse_project();
        // Project-owned IDs belong to another canvas. The isolated graph has
        // an ordinary field with the same numeric ID, not a generated item.
        project
            .graph
            .nodes
            .retain(|id, _| [ITEM, 4, REUSE_CALL, REUSE_EMPTY].contains(id));
        project
            .graph
            .nodes
            .insert(ITEM, physical_field(None, "Input"));
        let mut expected = project.clone();
        assert_eq!(
            connect_physical_source(&mut project, named, true, false, true),
            ITEM
        );
        expected.graph.nodes.insert(
            REUSE_CALL,
            Node::Call {
                function: "concat".into(),
                args: vec![ITEM, 4],
            },
        );
        expected.graph.nodes.remove(&REUSE_EMPTY);
        assert_eq!(snapshot(&project), snapshot(&expected));
    }
}

#[path = "graph_viewer_sequence_item_tests/allocation.rs"]
mod allocation;
