use super::*;
use crate::canvas::{source_blocks, target_blocks};
use crate::canvas_endpoints::EndpointScrollState;
use egui_snarl::{InPinId, OutPinId};
use ir::{Instance, SchemaNode};
use mapping::{Binding, NamedTarget, Project, ScopeConstruction, ScopeIteration};

fn project(root_copy: bool) -> Project {
    let mut project = crate::new_mapping::blank_project();
    project.source = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Nested",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::group(
                        "Children",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
            SchemaNode::group(
                "Editable",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
        ],
    );
    project.target = project.source.clone();
    project.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::String("edited".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("before".into()),
            },
        ),
        (
            2,
            Node::Call {
                function: "upper".into(),
                args: vec![0],
            },
        ),
    ]);
    project.root = if root_copy {
        Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        }
    } else {
        Scope {
            children: vec![
                Scope {
                    target_field: "Rows".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into()]),
                    construction: ScopeConstruction::CopyCurrentSource,
                    ..Scope::default()
                },
                Scope {
                    target_field: "Editable".into(),
                    bindings: vec![Binding {
                        target_field: "Name".into(),
                        node: 1,
                    }],
                    ..Scope::default()
                },
            ],
            ..Scope::default()
        }
    };
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    project
}

fn source() -> Instance {
    Instance::Group(vec![
        (
            "Name".into(),
            Instance::Scalar(Value::String("source root".into())),
        ),
        (
            "Nested".into(),
            Instance::Group(vec![(
                "Value".into(),
                Instance::Scalar(Value::String("source nested".into())),
            )]),
        ),
        (
            "Rows".into(),
            Instance::Repeated(vec![Instance::Group(vec![
                (
                    "Name".into(),
                    Instance::Scalar(Value::String("source row".into())),
                ),
                (
                    "Children".into(),
                    Instance::Repeated(vec![
                        Instance::Group(vec![(
                            "Value".into(),
                            Instance::Scalar(Value::String("first".into())),
                        )]),
                        Instance::Group(vec![(
                            "Value".into(),
                            Instance::Scalar(Value::String("second".into())),
                        )]),
                    ]),
                ),
            ])]),
        ),
        (
            "Editable".into(),
            Instance::Group(vec![(
                "Name".into(),
                Instance::Scalar(Value::String("source editable".into())),
            )]),
        ),
    ])
}

fn outputs(project: &Project) -> engine::ExecutionOutputs {
    let issues = engine::validate(project);
    assert!(issues.is_empty(), "valid copy project: {issues:?}");
    engine::run_outputs(project, &source()).expect("copy mapping executes")
}

fn target_pin(blocks: &[TargetBlock], path: &[&str]) -> (usize, usize) {
    let (field, chain) = path.split_last().expect("target path");
    blocks
        .iter()
        .enumerate()
        .find_map(|(block, section)| {
            section.leaves.iter().enumerate().find_map(|(pin, leaf)| {
                (leaf.field == *field
                    && leaf
                        .chain
                        .iter()
                        .map(String::as_str)
                        .eq(chain.iter().copied()))
                .then_some((block, pin))
            })
        })
        .expect("target endpoint")
}

type Wires = Vec<(OutPinId, InPinId)>;

fn connect_target(
    project: &mut Project,
    target: Option<usize>,
    path: &[&str],
    from_source: bool,
) -> (Option<String>, Wires, Wires) {
    let sources = source_blocks(&project.source);
    let source_paths = SourcePathCatalog::new(&project.source, &project.extra_sources);
    let targets = target_blocks(match target {
        Some(index) => &project.extra_targets[index].schema,
        None => &project.target,
    });
    let (target_block, input) = target_pin(&targets, path);
    let mut endpoint_scroll = EndpointScrollState::default();
    let mut snarl = Snarl::new();
    let source_ids = sources
        .iter()
        .enumerate()
        .map(|(index, _)| {
            snarl.insert_node(
                egui::pos2(0.0, index as f32 * 120.0),
                CanvasNode::SourceBlock(index),
            )
        })
        .collect::<Vec<_>>();
    let target_ids = targets
        .iter()
        .enumerate()
        .map(|(index, _)| {
            snarl.insert_node(
                egui::pos2(400.0, index as f32 * 120.0),
                CanvasNode::TargetBlock(index),
            )
        })
        .collect::<Vec<_>>();
    let constant = snarl.insert_node(egui::pos2(180.0, 0.0), CanvasNode::Graph(0));
    let call = snarl.insert_node(egui::pos2(180.0, 120.0), CanvasNode::Graph(2));
    snarl.connect(
        OutPinId {
            node: constant,
            output: 0,
        },
        InPinId {
            node: call,
            input: 0,
        },
    );
    let before_wires = snarl.wires().collect::<Vec<_>>();
    let from = if from_source {
        let (block, pin) = sources
            .iter()
            .enumerate()
            .find_map(|(block, section)| {
                section
                    .leaves
                    .iter()
                    .position(|leaf| leaf.label == "Editable/Name")
                    .map(|pin| (block, pin))
            })
            .expect("source editable endpoint");
        snarl.out_pin(OutPinId {
            node: source_ids[block],
            output: pin,
        })
    } else {
        snarl.out_pin(OutPinId {
            node: constant,
            output: 0,
        })
    };
    let to = snarl.in_pin(InPinId {
        node: target_ids[target_block],
        input,
    });

    let mut inactive = Vec::new();
    let (root_scope, extra_targets) = match target {
        None => (&mut project.root, project.extra_targets.as_slice()),
        Some(index) => {
            let (before, active_and_after) = project.extra_targets.split_at_mut(index);
            let (active, after) = active_and_after.split_first_mut().expect("active target");
            inactive = inactive_target_scopes(&project.root, before, after);
            (&mut active.root, &[][..])
        }
    };
    let mut viewer = GraphViewer {
        graph: &mut project.graph,
        root_scope,
        extra_targets,
        inactive_target_scopes: &inactive,
        project_references: ProjectGraphReferences::new(
            &project.failure_rules,
            &project.extra_sources,
        ),
        source_blocks: &sources,
        target_blocks: &targets,
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
    viewer.connect(&from, &to, &mut snarl);
    (viewer.error.take(), before_wires, snarl.wires().collect())
}

#[test]
fn source_and_graph_connections_reject_primary_and_named_copy_ancestors_atomically() {
    for target in [None, Some(0)] {
        for root_copy in [true, false] {
            let paths: &[&[&str]] = if root_copy {
                &[&["Name"], &["Nested", "Value"]]
            } else {
                &[&["Rows", "Name"], &["Rows", "Children", "Value"]]
            };
            for path in paths {
                for from_source in [true, false] {
                    let mut project = project(root_copy);
                    let before =
                        mapping::project_file::encode_pretty(&project).expect("project state");
                    let expected = outputs(&project);
                    let (error, before_wires, after_wires) =
                        connect_target(&mut project, target, path, from_source);
                    assert!(
                        error
                            .as_deref()
                            .is_some_and(|error| error.contains("whole source group copy")),
                        "copy owner rejects route {path:?}: {error:?}"
                    );
                    assert_eq!(before_wires, after_wires, "no displaced or new wires");
                    assert_eq!(
                        mapping::project_file::encode_pretty(&project).expect("unchanged state"),
                        before,
                        "no new graph input, binding or nested scope"
                    );
                    assert!(
                        !project
                            .graph
                            .nodes
                            .values()
                            .any(|node| matches!(node, Node::SourceField { .. })),
                        "rejected source connection cannot create a hidden input"
                    );
                    let actual = outputs(&project);
                    assert_eq!(actual.primary, expected.primary);
                    assert_eq!(actual.extras, expected.extras);
                }
            }
        }
    }
}

#[test]
fn source_and_graph_connections_to_constructed_siblings_still_execute() {
    for target in [None, Some(0)] {
        for from_source in [true, false] {
            let mut project = project(false);
            let before = outputs(&project);
            let (error, before_wires, after_wires) =
                connect_target(&mut project, target, &["Editable", "Name"], from_source);
            assert!(error.is_none(), "ordinary connection succeeds: {error:?}");
            assert_eq!(after_wires.len(), before_wires.len() + 1);
            let actual = outputs(&project);
            let (active, inactive) = match target {
                None => (&actual.primary, &actual.extras[0].instance),
                Some(_) => (&actual.extras[0].instance, &actual.primary),
            };
            let expected = if from_source {
                "source editable"
            } else {
                "edited"
            };
            assert_eq!(
                active
                    .field("Editable")
                    .and_then(|value| value.field("Name"))
                    .and_then(Instance::as_scalar),
                Some(&Value::String(expected.into()))
            );
            let expected_inactive = match target {
                None => &before.extras[0].instance,
                Some(_) => &before.primary,
            };
            assert_eq!(inactive, expected_inactive);
            assert_eq!(
                active.field("Rows"),
                before.primary.field("Rows"),
                "copied sibling keeps exact nested repetition"
            );
        }
    }
}
