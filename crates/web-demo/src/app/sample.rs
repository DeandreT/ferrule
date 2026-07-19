use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration,
};

pub(super) const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Orders>
  <Order>
    <Id>A-1</Id>
    <Item><Price>1.5</Price></Item>
    <Item><Price>2.5</Price></Item>
  </Order>
  <Order>
    <Id>B-2</Id>
    <Item><Price>10.0</Price></Item>
  </Order>
</Orders>
"#;

/// The built-in demo mapping: per-order aggregates plus a joined id list.
pub(super) fn demo_project() -> Project {
    let source = SchemaNode::group(
        "Orders",
        vec![
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group("Item", vec![SchemaNode::scalar("Price", ScalarType::Float)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Summary",
        vec![
            SchemaNode::scalar("AllIds", ScalarType::String),
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::scalar("ItemCount", ScalarType::Int),
                    SchemaNode::scalar("Total", ScalarType::Float),
                ],
            )
            .repeating(),
        ],
    );

    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Id".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String(", ".into()),
        },
    );
    graph.nodes.insert(
        2,
        Node::Aggregate {
            function: AggregateOp::Join,
            collection: vec!["Order".into()],
            value: vec!["Id".into()],
            expression: None,
            arg: Some(1),
        },
    );
    graph.nodes.insert(
        3,
        Node::Aggregate {
            function: AggregateOp::Count,
            collection: vec!["Item".into()],
            value: vec![],
            expression: None,
            arg: None,
        },
    );
    graph.nodes.insert(
        4,
        Node::Aggregate {
            function: AggregateOp::Sum,
            collection: vec!["Item".into()],
            value: vec!["Price".into()],
            expression: None,
            arg: None,
        },
    );

    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            group_by: None,
            group_starting_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "AllIds".into(),
                node: 2,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![Scope {
                target_field: "Order".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                construction: ScopeConstruction::Constructed,
                filter: None,
                group_by: None,
                group_starting_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                sort_then_by: Vec::new(),
                sort_filter_order: Default::default(),
                windows: Vec::new(),
                iteration_output: Default::default(),
                bindings: vec![
                    Binding {
                        target_field: "Id".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "ItemCount".into(),
                        node: 3,
                    },
                    Binding {
                        target_field: "Total".into(),
                        node: 4,
                    },
                ],
                dynamic_bindings: Vec::new(),
                children: vec![],
                dynamic_children: Vec::new(),
                merge_dynamic_fields: false,
            }],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    }
}
