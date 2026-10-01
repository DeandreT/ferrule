use super::*;
use egui_snarl::{InPinId, NodeId as SnarlNodeId};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, NamedTarget, ScopeIteration, SequenceExpr};

const PRIMARY_ITEM: NodeId = 7;
const NAMED_ITEM: NodeId = 8;
const ORDINARY: NodeId = 20;

fn project() -> Project {
    let mut project = crate::new_mapping::blank_project();
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("physical", ScalarType::String)],
    );
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
    for (id, node) in [
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
            PRIMARY_ITEM,
            Node::SourceField {
                path: vec!["physical".into()],
                frame: None,
            },
        ),
        (
            NAMED_ITEM,
            Node::SourceField {
                path: vec!["physical".into()],
                frame: None,
            },
        ),
        (
            ORDINARY,
            Node::SourceField {
                path: vec!["physical".into()],
                frame: None,
            },
        ),
        (
            30,
            Node::Call {
                function: "upper".into(),
                args: vec![ORDINARY],
            },
        ),
        (
            31,
            Node::Call {
                function: "upper".into(),
                args: vec![PRIMARY_ITEM],
            },
        ),
        (
            32,
            Node::Call {
                function: "upper".into(),
                args: vec![NAMED_ITEM],
            },
        ),
    ] {
        project.graph.nodes.insert(id, node);
    }
    let rows = |item| Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
            input: 0,
            delimiter: 1,
            item,
        }),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: item,
        }],
        ..Scope::default()
    };
    project.root = Scope {
        children: vec![rows(PRIMARY_ITEM)],
        ..Scope::default()
    };
    project.extra_targets = vec![NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: Scope {
            children: vec![rows(NAMED_ITEM)],
            ..Scope::default()
        },
    }];
    project
}

fn canvas_node(snarl: &Snarl<CanvasNode>, canvas: CanvasNode) -> SnarlNodeId {
    snarl
        .node_ids()
        .find_map(|(id, node)| (*node == canvas).then_some(id))
        .expect("visible canvas node")
}

#[test]
fn malformed_owned_source_leaves_remain_graph_nodes_through_build_sync_and_layout_restore() {
    let project = project();
    let before =
        mapping::project_file::encode_pretty(&project).expect("malformed project metadata");
    assert!(
        engine::validate(&project)
            .iter()
            .any(|issue| issue.message.contains("sequence item must reference"))
    );
    for named in [false, true] {
        let mut snarl = if named {
            build_named_target_snarl(&project, 0)
        } else {
            build_snarl(&project)
        };
        assert!(
            !snarl
                .nodes()
                .any(|node| *node == CanvasNode::Graph(ORDINARY)),
            "ordinary leaf stays an endpoint"
        );
        let source = canvas_node(&snarl, CanvasNode::SourceBlock(0));
        let ordinary_call = canvas_node(&snarl, CanvasNode::Graph(30));
        assert!(
            snarl
                .wires()
                .any(|(from, to)| from.node == source && to.node == ordinary_call && to.input == 0)
        );
        for (item, consumer) in [(PRIMARY_ITEM, 31), (NAMED_ITEM, 32)] {
            let node = canvas_node(&snarl, CanvasNode::Graph(item));
            let consumer = canvas_node(&snarl, CanvasNode::Graph(consumer));
            assert!(
                snarl
                    .wires()
                    .any(|(from, to)| from.node == node && to.node == consumer && to.input == 0)
            );
            let position = egui::pos2(420.0 + item as f32, 210.0 + item as f32);
            snarl.get_node_info_mut(node).expect("owned graph node").pos = position;
        }
        let saved_nodes = CanvasLayout::capture_nodes(&snarl);
        let mut restored = if named {
            let mut restored = build_named_target_snarl(&project, 0);
            CanvasLayout::apply_nodes(&saved_nodes, &mut restored);
            restored
        } else {
            let layout =
                CanvasLayout::capture(&project, &snarl, &crate::app::MappingWorkspace::default());
            build_snarl_with_layout(&project, Some(&layout))
        };
        let source = canvas_node(&restored, CanvasNode::SourceBlock(0));
        let consumer = canvas_node(&restored, CanvasNode::Graph(31));
        restored.connect(
            OutPinId {
                node: source,
                output: 0,
            },
            InPinId {
                node: consumer,
                input: 0,
            },
        );
        let root = if named {
            &project.extra_targets[0].root
        } else {
            &project.root
        };
        let target = if named {
            &project.extra_targets[0].schema
        } else {
            &project.target
        };
        let owned_items = crate::graph_viewer::project_sequence_item_ids(&project);
        for _ in 0..3 {
            sync_endpoint_wires_with_owned_items(
                &project.graph,
                root,
                &source_blocks(&project.source),
                &target_blocks(target),
                &EndpointScrollState::default(),
                &mut restored,
                &owned_items,
            );
        }
        assert!(
            !restored
                .wires()
                .any(|(from, to)| from.node == source && to.node == consumer)
        );
        for (item, call) in [(PRIMARY_ITEM, 31), (NAMED_ITEM, 32)] {
            let node = canvas_node(&restored, CanvasNode::Graph(item));
            let call = canvas_node(&restored, CanvasNode::Graph(call));
            assert!(
                restored
                    .wires()
                    .any(|(from, to)| from.node == node && to.node == call)
            );
            assert_eq!(
                restored
                    .get_node_info(node)
                    .expect("restored owned node")
                    .pos,
                egui::pos2(420.0 + item as f32, 210.0 + item as f32)
            );
        }
        let ordinary_call = canvas_node(&restored, CanvasNode::Graph(30));
        assert!(
            restored
                .wires()
                .any(|(from, to)| from.node == source && to.node == ordinary_call)
        );
    }
    assert_eq!(
        mapping::project_file::encode_pretty(&project).expect("after projection"),
        before
    );
}
