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

#[test]
fn owned_mixed_content_keeps_replacement_wires_read_only_with_contexts() {
    use mapping::{
        OwnedXmlMixedContent, OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement,
    };
    let node = Node::OwnedXmlMixedContent(OwnedXmlMixedContent {
        source: OwnedSourceRef {
            owner: SourceOwner::Named {
                name: "notes".into(),
            },
            path: vec!["Doc".into(), "Para".into()],
        },
        frame: Some(vec!["Doc".into()]),
        replacements: vec![
            OwnedXmlMixedContentReplacement {
                element: "b".into(),
                context: OwnedXmlMixedContentContext::Occurrence,
                expression: 3,
            },
            OwnedXmlMixedContentReplacement {
                element: "i".into(),
                context: OwnedXmlMixedContentContext::Parent,
                expression: 4,
            },
        ],
    });
    let mut graph = Graph::default();
    graph.nodes.insert(9, node.clone());
    for id in [3, 4] {
        graph.nodes.insert(
            id,
            Node::Const {
                value: ir::Value::String(id.to_string()),
            },
        );
    }
    let encoded = serde_json::to_string(&graph).unwrap();
    let reopened: Graph = serde_json::from_str(&encoded).unwrap();
    assert_eq!(serde_json::to_string(&reopened).unwrap(), encoded);
    let mut fx = fixture();
    fx.graph = reopened;
    assert_eq!(GraphViewer::input_count(&node), 2);
    assert_eq!(fx.viewer().input_at(9, 0), Some(3));
    assert_eq!(fx.viewer().input_at(9, 1), Some(4));
    assert_eq!(graph_references::node_inputs(&node), vec![3, 4]);
    // Saved wires cannot be rewired or cleared.
    assert!(!fx.viewer().set_input(9, 0, 4));
    assert_eq!(serde_json::to_string(&fx.graph).unwrap(), encoded);
    assert!(graph_node_presentation::header(&node, "owned", false).is_some());
    let title = graph_node_presentation::owned_source_title(&node).unwrap();
    assert!(title.contains("Owned XML mixed content: named \"notes\""));
    assert!(title.contains("Frame: explicit [\"Doc\"]"));
    assert!(title.contains("Replacements: b (occurrence), i (parent)"));
    assert!(title.contains("Read-only; owned mixed-content authoring is unavailable"));
    let occurrence = graph_node_presentation::input_pin_hint(Some(&node), 0).unwrap();
    assert!(occurrence.contains("Context: owned occurrence Doc/Para/b"));
    let parent = graph_node_presentation::input_pin_hint(Some(&node), 1).unwrap();
    assert!(parent.contains("Context: parent context"));
}

#[test]
fn owned_joined_aggregates_display_their_plan_read_only_and_keep_inputs() {
    let rows = |owner| OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    };
    let named = || SourceOwner::Named {
        name: "Lookup".into(),
    };
    let plan = mapping::OwnedJoinPlan::new(
        mapping::OwnedJoinSource::new(rows(SourceOwner::Primary)),
        mapping::OwnedJoinSource::new(rows(named())),
        mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
            rows(SourceOwner::Primary),
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    let node = Node::OwnedJoinAggregate(mapping::OwnedJoinAggregate {
        function: mapping::AggregateOp::Join,
        join: mapping::JoinId::new(8),
        plan,
        expression: Some(10),
        arg: Some(11),
    });
    let mut fx = fixture();
    fx.graph = Graph {
        nodes: [(9, node.clone())].into_iter().collect(),
    };
    // The tuple expression and the parent argument stay wired in order.
    assert_eq!(GraphViewer::input_count(&node), 2);
    assert_eq!(fx.viewer().input_at(9, 0), Some(10));
    assert_eq!(fx.viewer().input_at(9, 1), Some(11));
    assert_eq!(graph_references::node_inputs(&node), vec![10, 11]);
    let title = graph_node_presentation::owned_source_title(&node).unwrap();
    for expected in [
        "Owned join 8 aggregate Join",
        r#"primary ["Rows"], named "Lookup" ["Rows"]"#,
        "Value: computed per tuple",
        "Read-only; owned join authoring is unavailable",
    ] {
        assert!(title.contains(expected), "{title}");
    }
}
