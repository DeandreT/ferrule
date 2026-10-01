use super::*;
use ir::{Instance, Value};
use mapping::{
    AggregateOp, FailureIteration, FailureRule, FailureSelection, ScopeIteration, SequenceExpr,
};

const ITEMS: [NodeId; 8] = [100, 101, 102, 103, 104, 105, 106, 107];
const ORDINARY_FIELD: NodeId = 50;

fn physical_field() -> Node {
    Node::SourceField {
        path: vec!["Name".into()],
        frame: None,
    }
}

fn item_sequence(item: NodeId) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input: 0,
        delimiter: 1,
        item,
    }
}

fn item_scope(item: NodeId) -> Scope {
    Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(item_sequence(item)),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    }
}

fn item_project() -> Project {
    let mut project = blank_project();
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Name", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
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
            20,
            Node::SequenceExists {
                sequence: item_sequence(104),
                predicate: 3,
            },
        ),
        (
            21,
            Node::SequenceItemAt {
                sequence: item_sequence(105),
                index: 2,
            },
        ),
        (
            22,
            Node::SequenceAggregate {
                function: AggregateOp::Count,
                sequence: item_sequence(106),
                predicate: None,
                expression: None,
                arg: None,
            },
        ),
    ]
    .into_iter()
    .collect();
    for item in ITEMS {
        project.graph.nodes.insert(
            item,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
    }
    project.root.children = vec![item_scope(100)];
    project.extra_targets = ["Before", "Current", "After"]
        .into_iter()
        .zip([101, 102, 103])
        .map(|(name, item)| NamedTarget {
            name: name.into(),
            path: Some(format!("{name}.json")),
            schema: project.target.clone(),
            options: Default::default(),
            root: Scope {
                children: vec![item_scope(item)],
                ..Scope::default()
            },
        })
        .collect();
    project.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: item_sequence(107),
        },
        selection: FailureSelection::WhenFalse { predicate: 3 },
        message: None,
    }];
    project
}

fn snapshot(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).expect("project snapshot")
}

fn active_root(project: &mut Project, named: bool) -> &mut Scope {
    if named {
        &mut project.extra_targets[1].root
    } else {
        &mut project.root
    }
}

fn active_canvas(app: &FerruleApp, named: bool) -> &Snarl<CanvasNode> {
    if named {
        &app.mapping_workspace.target_canvases[&1].snarl
    } else {
        &app.main_canvas.snarl
    }
}

fn item_app(project: Project, named: bool) -> FerruleApp {
    let mut app = FerruleApp {
        project,
        ..FerruleApp::default()
    };
    app.main_canvas.snarl = build_snarl(&app.project);
    for index in 0..3 {
        assert!(app.ensure_target_canvas(index));
    }
    if named {
        app.open_target_tab(1);
    }
    app
}

fn repaired_outputs(project: &Project, named: bool) {
    // Keep the imported malformed item in the stored mapping. Repair only a
    // clone so execution can verify the newly connected physical source.
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
    assert!(issues.is_empty(), "repaired mapping: {issues:?}");
    let source = Instance::Group(vec![(
        "Name".into(),
        Instance::Scalar(Value::String("physical".into())),
    )]);
    let output = engine::run_outputs(&executable, &source).expect("physical mapping output");
    let rows = Instance::Repeated(
        ["a", "b"]
            .into_iter()
            .map(|value| {
                Instance::Group(vec![(
                    "Value".into(),
                    Instance::Scalar(Value::String(value.into())),
                )])
            })
            .collect(),
    );
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
            .find(|(name, _)| name == "Name")
            .map(|(_, value)| value);
        if index == if named { 2 } else { 0 } {
            assert_eq!(
                value,
                Some(&Instance::Scalar(Value::String("physical".into())))
            );
        } else {
            assert!(value.is_none() || value == Some(&Instance::Scalar(Value::Null)));
        }
    }
}

fn auto_connect_item_matrix(named: bool) {
    for item in ITEMS {
        for preexisting in [false, true] {
            let mut project = item_project();
            project.graph.nodes.insert(item, physical_field());
            if preexisting {
                project.graph.nodes.insert(ORDINARY_FIELD, physical_field());
            }
            let mut app = item_app(project, named);
            let custom = egui::pos2(117.0, 259.0);
            if named {
                move_canvas_node(
                    &mut app
                        .mapping_workspace
                        .target_canvases
                        .get_mut(&1)
                        .expect("named canvas")
                        .snarl,
                    CanvasNode::SourceBlock(0),
                    custom,
                );
            } else {
                move_canvas_node(
                    &mut app.main_canvas.snarl,
                    CanvasNode::SourceBlock(0),
                    custom,
                );
            }
            app.mark_clean();
            app.rebase_history();
            let before = app.project.clone();
            let before_snapshot = snapshot(&before);

            app.begin_auto_connect();
            let pending = app
                .pending_auto_connect
                .as_ref()
                .expect("confirmation staged");
            assert_eq!(
                pending.target,
                if named {
                    AutoConnectTarget::Named(1)
                } else {
                    AutoConnectTarget::Primary
                }
            );
            assert_eq!(pending.plan.connections.len(), 1);
            app.apply_pending_auto_connect();
            app.observe_editor_history(std::time::Instant::now(), false);

            let binding = active_root(&mut app.project, named)
                .bindings
                .iter()
                .find(|binding| binding.target_field == "Name")
                .expect("physical field bound")
                .node;
            assert!(!ITEMS.contains(&binding), "owned item {item} reused");
            assert!(
                matches!(&app.project.graph.nodes[&binding], Node::SourceField { path, frame } if path == &["Name"] && frame.is_none())
            );
            let mut expected = before.clone();
            if preexisting {
                assert_eq!(binding, ORDINARY_FIELD, "lower-ID ordinary field is reused");
            } else {
                assert!(!before.graph.nodes.contains_key(&binding));
                expected.graph.nodes.insert(binding, physical_field());
            }
            active_root(&mut expected, named).bindings.push(Binding {
                target_field: "Name".into(),
                node: binding,
            });
            let after_snapshot = snapshot(&app.project);
            assert_eq!(
                after_snapshot,
                snapshot(&expected),
                "owned graph/scope metadata remains untouched"
            );
            let canvas = active_canvas(&app, named);
            assert!(
                canvas.wires().any(|(from, to)| {
                    canvas[from.node] == CanvasNode::SourceBlock(0)
                        && from.output == 0
                        && canvas[to.node] == CanvasNode::TargetBlock(0)
                        && to.input == 0
                }),
                "physical endpoint wire is rebuilt"
            );
            assert!(canvas.nodes().any(|node| *node == CanvasNode::Graph(item)));
            assert!(
                !canvas
                    .nodes()
                    .any(|node| matches!(node, CanvasNode::Placeholder(_)))
            );
            assert_eq!(canvas_position(canvas, CanvasNode::SourceBlock(0)), custom);
            repaired_outputs(&app.project, named);
            assert_eq!(app.history.undo_len(), 1);

            app.undo_project();
            assert_eq!(snapshot(&app.project), before_snapshot);
            assert!(!app.is_dirty());
            assert_eq!(
                canvas_position(active_canvas(&app, named), CanvasNode::SourceBlock(0)),
                custom
            );
            app.redo_project();
            assert_eq!(snapshot(&app.project), after_snapshot);
            assert_eq!(
                app.mapping_workspace.active,
                if named {
                    MappingDocument::Target(1)
                } else {
                    MappingDocument::Main
                }
            );
            assert_eq!(
                canvas_position(active_canvas(&app, named), CanvasNode::SourceBlock(0)),
                custom
            );
        }
    }
}

#[test]
fn primary_auto_connect_never_reuses_owned_sequence_items() {
    auto_connect_item_matrix(false);
}

#[test]
fn named_auto_connect_never_reuses_active_or_inactive_sequence_items() {
    auto_connect_item_matrix(true);
}

#[test]
fn auto_connect_reserves_missing_owned_item_ids_on_both_canvases() {
    for named in [false, true] {
        let mut project = item_project();
        project.root.children = vec![item_scope(4)];
        project.graph.nodes.remove(&100);
        assert!(!project.graph.nodes.contains_key(&4));
        let mut app = item_app(project, named);
        app.mark_clean();
        app.rebase_history();
        let before = app.project.clone();
        app.begin_auto_connect();
        assert_eq!(
            app.pending_auto_connect
                .as_ref()
                .expect("confirmation staged")
                .plan
                .connections
                .len(),
            1
        );
        app.apply_pending_auto_connect();
        app.observe_editor_history(std::time::Instant::now(), false);

        let binding = active_root(&mut app.project, named).bindings[0].node;
        assert_eq!(binding, 5, "first unowned free identifier is reserved");
        assert!(
            !app.project.graph.nodes.contains_key(&4),
            "missing item is not materialized by an ordinary connection"
        );
        let mut expected = before.clone();
        expected.graph.nodes.insert(5, physical_field());
        active_root(&mut expected, named).bindings.push(Binding {
            target_field: "Name".into(),
            node: 5,
        });
        let after = snapshot(&app.project);
        assert_eq!(after, snapshot(&expected));
        repaired_outputs(&app.project, named);
        assert_eq!(app.history.undo_len(), 1);
        app.undo_project();
        assert_eq!(snapshot(&app.project), snapshot(&before));
        app.redo_project();
        assert_eq!(snapshot(&app.project), after);
    }
}
