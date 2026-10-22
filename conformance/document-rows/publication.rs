pub fn project(mut project: mapping::Project) -> mapping::Project {
    project.graph.nodes.insert(
        10,
        mapping::Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.graph.nodes.insert(
        11,
        mapping::Node::Call {
            function: "concat".into(),
            args: vec![10, 5, 6],
        },
    );
    assert!(project.root.set_output_path(Some(11)));
    project
}
