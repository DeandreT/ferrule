use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Endpoint {
    Graph(NodeId),
    Target,
}

fn endpoint(node: &CanvasNode) -> Endpoint {
    match node {
        CanvasNode::Graph(id) => Endpoint::Graph(*id),
        CanvasNode::Target => Endpoint::Target,
    }
}

fn project(definitions: &[(&str, NodeId)]) -> mapping::Project {
    let mut project = super::super::sample::demo_project();
    project.target = ir::SchemaNode::group(
        "Target",
        definitions
            .iter()
            .map(|(name, _)| ir::SchemaNode::scalar(*name, ir::ScalarType::String))
            .collect(),
    );
    project.graph = Graph::default();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("left".into()),
        },
    );
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("right".into()),
        },
    );
    project.graph.nodes.insert(
        2,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 1],
        },
    );
    project.graph.nodes.insert(3, Node::Unconnected);
    project.root = Scope {
        bindings: definitions
            .iter()
            .map(|(name, node)| mapping::Binding {
                target_field: (*name).into(),
                node: *node,
            })
            .collect(),
        ..Scope::default()
    };
    project
}

fn check_both_layouts(definitions: &[(&str, NodeId)], target_wires: &[(NodeId, usize)]) {
    for compact in [false, true] {
        let mut project = project(definitions);
        let original = serde_json::to_value(&project).unwrap();
        let mut bindings = Vec::new();
        flat_bindings(&project.root, "", &mut bindings);
        let expected_bindings: Vec<_> = definitions
            .iter()
            .map(|(name, node)| ((*name).to_string(), *node))
            .collect();
        // Keep the complete incomplete Project and inputs in the original test
        // output even when the baseline builder panics before returning a canvas.
        eprintln!(
            "canvas target original: compact={compact}; project={original:#?}; bindings={bindings:?}"
        );
        let snarl = build_snarl(&project, &bindings, compact);
        let mut actual_nodes: Vec<_> = snarl
            .node_ids()
            .map(|(id, node)| {
                let info = snarl.get_node_info(id).unwrap();
                (endpoint(node), info.pos, info.open)
            })
            .collect();
        actual_nodes.sort_by_key(|(node, _, _)| *node);
        let mut actual_wires: Vec<_> = snarl
            .wires()
            .map(|(from, to)| {
                (
                    endpoint(&snarl[from.node]),
                    from.output,
                    endpoint(&snarl[to.node]),
                    to.input,
                )
            })
            .collect();
        actual_wires.sort();
        let mut expected_wires = vec![
            (Endpoint::Graph(0), 0, Endpoint::Graph(2), 0),
            (Endpoint::Graph(1), 0, Endpoint::Graph(2), 1),
        ];
        expected_wires.extend(
            target_wires
                .iter()
                .map(|(from, input)| (Endpoint::Graph(*from), 0, Endpoint::Target, *input)),
        );
        expected_wires.sort();
        let expected_nodes = vec![
            (Endpoint::Graph(0), egui::pos2(20.0, 30.0), true),
            (Endpoint::Graph(1), egui::pos2(20.0, 120.0), true),
            (
                Endpoint::Graph(2),
                if compact {
                    egui::pos2(220.0, 30.0)
                } else {
                    egui::pos2(180.0, 80.0)
                },
                true,
            ),
            (
                Endpoint::Target,
                if compact {
                    egui::pos2(120.0, 390.0)
                } else {
                    egui::pos2(330.0, 60.0)
                },
                true,
            ),
        ];
        let mut run_pending = false;
        let mut project_changed = false;
        let mut viewer = DemoViewer::new(
            &mut project.graph,
            &bindings,
            &mut run_pending,
            &mut project_changed,
        );
        let actual_pin_counts = (
            viewer.inputs(&CanvasNode::Target),
            viewer.outputs(&CanvasNode::Target),
            viewer.inputs(&CanvasNode::Graph(2)),
        );
        let restored = serde_json::to_value(&project).unwrap();
        eprintln!(
            "canvas target returned: nodes={actual_nodes:?}; wires={actual_wires:?}; pins={actual_pin_counts:?}; project={restored:#?}; pending={run_pending}; changed={project_changed}"
        );
        assert_eq!(bindings, expected_bindings);
        // Compare complete endpoint tuples with their multiplicity, not counts.
        assert_eq!(actual_wires, expected_wires);
        // The missing and deliberately unconnected nodes create no placeholders.
        assert_eq!(actual_nodes, expected_nodes);
        assert_eq!(actual_pin_counts, (definitions.len(), 0, 2));
        assert_eq!(restored, original);
        assert!(!run_pending && !project_changed);
    }
}

#[test]
fn valid_browser_target_bindings_preserve_every_wire_and_input_index() {
    check_both_layouts(
        &[("left", 0), ("right", 1), ("joined", 2), ("again", 0)],
        &[(0, 0), (1, 1), (2, 2), (0, 3)],
    );
}

#[test]
fn missing_browser_target_bindings_remain_disconnected() {
    check_both_layouts(&[("missing", 999), ("unconnected", 3)], &[]);
}

#[test]
fn mixed_browser_target_bindings_keep_gaps_and_duplicate_source_wires() {
    check_both_layouts(
        &[
            ("missing", 999),
            ("left", 0),
            ("unconnected", 3),
            ("joined", 2),
            ("also_missing", 1000),
            ("right", 1),
            ("again", 0),
        ],
        &[(0, 1), (2, 3), (1, 5), (0, 6)],
    );
}

#[test]
fn empty_browser_target_bindings_keep_the_graph_and_zero_target_inputs() {
    check_both_layouts(&[], &[]);
}
