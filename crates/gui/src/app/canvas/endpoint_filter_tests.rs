use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::Binding;

#[test]
fn connected_filters_and_search_preserve_canonical_wires_and_mapping_data() {
    let source_schema = SchemaNode::group(
        "source",
        (0..15)
            .map(|i| SchemaNode::scalar(format!("s{i}"), ScalarType::String))
            .collect(),
    );
    let target_schema = SchemaNode::group(
        "target",
        (0..15)
            .map(|i| SchemaNode::scalar(format!("t{i}"), ScalarType::String))
            .collect(),
    );
    let sources = source_blocks(&source_schema);
    let targets = target_blocks(&target_schema);
    let mut graph = mapping::Graph::default();
    for (id, field) in [(0, 2), (1, 8), (2, 12), (3, 14)] {
        graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![format!("s{field}")],
                frame: None,
            },
        );
    }
    // s14 is an unused source node. Missing node 404 cannot supply a wire.
    let root = Scope {
        bindings: [("t1", 0), ("t5", 1), ("t13", 2), ("t3", 0), ("t11", 404)]
            .into_iter()
            .map(|(field, node)| Binding {
                target_field: field.into(),
                node,
            })
            .collect(),
        ..Default::default()
    };
    let mapping_before = serde_json::to_value((&graph, &root)).unwrap();
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::pos2(10.0, 20.0), CanvasNode::SourceBlock(0));
    let target = snarl.insert_node(egui::pos2(400.0, 20.0), CanvasNode::TargetBlock(0));
    let mut scroll = EndpointScrollState::default();
    let owned = Default::default();
    let unavailable = Default::default();
    let policy = || SourcePinPolicy {
        owned_items: &owned,
        unavailable_blocks: &unavailable,
    };
    let canonical_before =
        semantic_canvas_wires(&graph, &root, &sources, &targets, &snarl, policy());
    assert_eq!(canonical_before.len(), 4);
    assert!(update_endpoint_connections_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &mut scroll,
        &snarl,
        policy(),
    ));
    assert_eq!(scroll.connected_count(CanvasNode::SourceBlock(0), 15), 3);
    assert_eq!(scroll.connected_count(CanvasNode::TargetBlock(0), 15), 4);
    scroll.set_connected_only(CanvasNode::SourceBlock(0), true);
    scroll.set_connected_only(CanvasNode::TargetBlock(0), true);
    sync_endpoint_wires_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &scroll,
        &mut snarl,
        policy(),
    );
    let pins = |snarl: &Snarl<CanvasNode>| {
        snarl
            .wires()
            .map(|(from, to)| {
                assert_eq!(from.node, source);
                assert_eq!(to.node, target);
                (from.output, to.input)
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        scroll.eligible_indices(CanvasNode::SourceBlock(0), 15),
        vec![2, 8, 12]
    );
    assert_eq!(
        scroll.eligible_indices(CanvasNode::TargetBlock(0), 15),
        vec![1, 3, 5, 13]
    );
    assert_eq!(
        pins(&snarl),
        [(0, 0), (0, 1), (1, 2), (2, 3)]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );

    assert!(scroll.reveal(CanvasNode::SourceBlock(0), 15, 6));
    sync_endpoint_wires_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &scroll,
        &mut snarl,
        policy(),
    );
    assert_eq!(
        pins(&snarl),
        [(0, 0), (0, 1), (2, 2), (3, 3)]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(
        semantic_canvas_wires(&graph, &root, &sources, &targets, &snarl, policy()),
        canonical_before
    );
    assert!(scroll.clear_reveal(CanvasNode::SourceBlock(0), 15));
    sync_endpoint_wires_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &scroll,
        &mut snarl,
        policy(),
    );
    assert_eq!(
        pins(&snarl),
        [(0, 0), (0, 1), (1, 2), (2, 3)]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );

    scroll.set_connected_only(CanvasNode::SourceBlock(0), false);
    scroll.set_connected_only(CanvasNode::TargetBlock(0), false);
    sync_endpoint_wires_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &scroll,
        &mut snarl,
        policy(),
    );
    assert_eq!(
        pins(&snarl),
        [(2, 1), (2, 3), (8, 5), (12, 12)]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(
        serde_json::to_value((&graph, &root)).unwrap(),
        mapping_before
    );
}

#[test]
fn canonical_membership_respects_owned_item_and_unavailable_source_policy() {
    let source_schema =
        SchemaNode::group("source", vec![SchemaNode::scalar("s", ScalarType::String)]);
    let target_schema =
        SchemaNode::group("target", vec![SchemaNode::scalar("t", ScalarType::String)]);
    let sources = source_blocks(&source_schema);
    let targets = target_blocks(&target_schema);
    let mut graph = mapping::Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["s".into()],
            frame: None,
        },
    );
    let root = Scope {
        bindings: vec![Binding {
            target_field: "t".into(),
            node: 0,
        }],
        ..Default::default()
    };
    let mut snarl = Snarl::new();
    snarl.insert_node(egui::Pos2::ZERO, CanvasNode::SourceBlock(0));
    snarl.insert_node(egui::pos2(400.0, 0.0), CanvasNode::TargetBlock(0));
    let mut scroll = EndpointScrollState::default();
    let empty_owned = Default::default();
    let empty_unavailable = Default::default();
    update_endpoint_connections_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &mut scroll,
        &snarl,
        SourcePinPolicy {
            owned_items: &empty_owned,
            unavailable_blocks: &empty_unavailable,
        },
    );
    assert_eq!(scroll.connected_count(CanvasNode::SourceBlock(0), 1), 1);
    assert_eq!(scroll.connected_count(CanvasNode::TargetBlock(0), 1), 1);
    let unavailable = [0].into_iter().collect();
    update_endpoint_connections_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &mut scroll,
        &snarl,
        SourcePinPolicy {
            owned_items: &empty_owned,
            unavailable_blocks: &unavailable,
        },
    );
    assert_eq!(scroll.connected_count(CanvasNode::SourceBlock(0), 1), 0);
    assert_eq!(scroll.connected_count(CanvasNode::TargetBlock(0), 1), 0);
    let owned = [0].into_iter().collect();
    update_endpoint_connections_with_policy(
        &graph,
        &root,
        &sources,
        &targets,
        &mut scroll,
        &snarl,
        SourcePinPolicy {
            owned_items: &owned,
            unavailable_blocks: &empty_unavailable,
        },
    );
    assert_eq!(scroll.connected_count(CanvasNode::SourceBlock(0), 1), 0);
    assert_eq!(scroll.connected_count(CanvasNode::TargetBlock(0), 1), 0);
}
