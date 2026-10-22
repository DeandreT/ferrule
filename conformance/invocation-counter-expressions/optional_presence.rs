use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Node, Project, Scope, ScopeIteration};

/// Extend the shared counter fixture without changing its named audit output.
pub fn project(mut project: Project) -> Project {
    for (id, name) in [(6, "count_condition"), (7, "keep")] {
        project.graph.nodes.insert(
            id,
            Node::RuntimeParameter {
                name: name.into(),
                ty: ScalarType::Bool,
                preview: None,
            },
        );
    }
    project.graph.nodes.insert(
        8,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 2],
        },
    );
    project.graph.nodes.insert(
        4,
        Node::If {
            condition: 6,
            then: 8,
            else_: 7,
        },
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Maybe", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    project.root = Scope {
        children: vec![Scope {
            target_field: "Maybe".into(),
            iteration: ScopeIteration::OptionalPresence { condition: 4 },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    project
}
