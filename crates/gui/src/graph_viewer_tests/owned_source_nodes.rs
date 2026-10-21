use mapping::{Graph, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

use super::{GraphViewer, fixture, graph_node_presentation, graph_references};

#[test]
fn owned_nodes_preserve_identity_zero_inputs_and_full_unavailable_details() {
    let nodes = [
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: " Ref 雪 ".into(),
                },
                path: vec!["Rows".into(), "Value".into()],
            },
            frame: Some(vec![]),
        }),
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        }),
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(vec![]),
        }),
    ];
    let mut graph = Graph {
        nodes: nodes
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, node)| (index as u32 + 9, node))
            .collect(),
    };
    graph.nodes.insert(
        99,
        Node::Call {
            function: "identity".into(),
            args: vec![9],
        },
    );
    let encoded = serde_json::to_string(&graph).unwrap();
    let reopened: Graph = serde_json::from_str(&encoded).unwrap();
    assert_eq!(serde_json::to_string(&reopened).unwrap(), encoded);
    assert_eq!(reopened.nodes[&99].dependencies(), vec![9]);
    assert!(reopened.nodes.contains_key(&9));
    let mut fx = fixture();
    fx.graph = reopened;
    for (index, node) in nodes.iter().enumerate() {
        assert_eq!(GraphViewer::input_count(node), 0);
        assert_eq!(fx.viewer().input_at(index as u32 + 9, 0), None);
        assert!(graph_references::node_inputs(node).is_empty());
        assert!(graph_node_presentation::header(node, "owned", false).is_some());
        let full = graph_node_presentation::complete_title(Some(node), "short".into());
        assert!(full.contains("Read-only; source ownership execution is unavailable"));
    }
    let scalar = graph_node_presentation::owned_source_title(&nodes[0]).unwrap();
    assert!(scalar.contains(" Ref 雪 "));
    assert!(scalar.contains(r#"["Rows", "Value"]"#));
    assert!(scalar.contains("Frame: explicit []"));
    assert!(
        graph_node_presentation::owned_source_title(&nodes[1])
            .unwrap()
            .contains("automatic (unresolved)")
    );
    assert!(
        graph_node_presentation::owned_source_title(&nodes[2])
            .unwrap()
            .contains("explicit []")
    );
}

#[test]
fn owned_join_reads_display_their_identity_read_only_with_zero_inputs() {
    let rows = OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "Lookup".into(),
        },
        path: vec!["Rows".into()],
    };
    let nodes = [
        Node::OwnedJoinField(mapping::OwnedJoinField {
            join: mapping::JoinId::new(7),
            collection: rows.clone(),
            path: vec!["code".into()],
        }),
        Node::OwnedJoinSourcePosition(mapping::OwnedJoinSourcePosition {
            join: mapping::JoinId::new(7),
            collection: rows,
        }),
    ];
    let mut fx = fixture();
    fx.graph = Graph {
        nodes: nodes
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, node)| (index as u32 + 9, node))
            .collect(),
    };
    for (index, node) in nodes.iter().enumerate() {
        assert_eq!(GraphViewer::input_count(node), 0);
        assert_eq!(fx.viewer().input_at(index as u32 + 9, 0), None);
        assert!(graph_references::node_inputs(node).is_empty());
        let title = graph_node_presentation::owned_source_title(node).unwrap();
        assert!(title.contains("Owned join 7"), "{title}");
        assert!(title.contains(r#""Lookup""#), "{title}");
        assert!(
            title.contains("Read-only; owned join authoring is unavailable"),
            "{title}"
        );
    }
    assert!(
        graph_node_presentation::owned_source_title(&nodes[0])
            .unwrap()
            .contains(r#"Path: ["code"]"#)
    );
}
