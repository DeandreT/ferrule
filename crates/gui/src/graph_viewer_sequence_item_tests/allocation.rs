use super::*;

fn missing_scope_item(project: &mut Project, named: bool, old: NodeId, new: NodeId) {
    let scope = if named {
        &mut project.extra_targets[0].root.children[0]
    } else {
        &mut project.root.children[0]
    };
    scope.set_sequence(Some(sequence(new)));
    scope.bindings[0].node = new;
    project.graph.nodes.remove(&old);
}

#[test]
fn physical_source_allocation_skips_consecutive_missing_owned_item_ids() {
    for named in [false, true] {
        for to_function in [false, true] {
            let mut project = source_reuse_project();
            missing_scope_item(&mut project, false, ITEM, 92);
            missing_scope_item(&mut project, true, NAMED_ITEM, 93);
            let mut expected = project.clone();
            let ownership = crate::graph_viewer::project_sequence_item_ids(&project);
            assert!(ownership.contains(&92) && ownership.contains(&93));
            let field = connect_physical_source(&mut project, named, to_function, false, false);
            assert_eq!(field, 94);
            assert!(!project.graph.nodes.contains_key(&92));
            assert!(!project.graph.nodes.contains_key(&93));
            assert!(matches!(project.graph.nodes.get(&field),
                Some(Node::SourceField { path, frame: None }) if path == &["Input"]));
            expected
                .graph
                .nodes
                .insert(94, physical_field(None, "Input"));
            if to_function {
                expected.graph.nodes.insert(
                    REUSE_CALL,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![94, 4],
                    },
                );
                expected.graph.nodes.remove(&REUSE_EMPTY);
            }
            reuse_root(&mut expected, named).bindings.push(Binding {
                target_field: "Output".into(),
                node: if to_function { REUSE_CALL } else { 94 },
            });
            assert_eq!(
                snapshot(&project),
                snapshot(&expected),
                "scope/reducer/failure ownership metadata is unchanged"
            );
            assert_eq!(
                crate::graph_viewer::project_sequence_item_ids(&project),
                ownership
            );
            repaired_physical_outputs(&project, named, to_function);
        }
    }
    let mut isolated = source_reuse_project();
    missing_scope_item(&mut isolated, false, ITEM, 92);
    let mut expected = isolated.clone();
    assert_eq!(
        connect_physical_source(&mut isolated, false, true, false, true),
        92,
        "isolated functions do not inherit project item IDs"
    );
    expected
        .graph
        .nodes
        .insert(92, physical_field(None, "Input"));
    expected.graph.nodes.insert(
        REUSE_CALL,
        Node::Call {
            function: "concat".into(),
            args: vec![92, 4],
        },
    );
    expected.graph.nodes.remove(&REUSE_EMPTY);
    assert_eq!(snapshot(&isolated), snapshot(&expected));
}

#[test]
fn exhausted_source_allocation_preserves_graph_binding_input_and_wires() {
    for named in [false, true] {
        for to_function in [false, true] {
            for owned_last in [false, true] {
                for reusable in [false, true] {
                    let mut project = source_reuse_project();
                    let maximum = if owned_last { u32::MAX - 1 } else { u32::MAX };
                    project.graph.nodes.insert(
                        maximum,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    );
                    if owned_last {
                        missing_scope_item(&mut project, false, ITEM, u32::MAX);
                    }
                    reuse_root(&mut project, named).bindings.push(Binding {
                        target_field: "Output".into(),
                        node: 21,
                    });
                    if to_function {
                        let Node::Call { args, .. } =
                            project.graph.nodes.get_mut(&REUSE_CALL).unwrap()
                        else {
                            unreachable!()
                        };
                        args[0] = 21;
                    }
                    if reusable {
                        project
                            .graph
                            .nodes
                            .insert(REUSE_FIELD, physical_field(None, "Input"));
                    }
                    let before = snapshot(&project);
                    let mut expected = project.clone();
                    let result = connect_physical_source_attempt(
                        &mut project,
                        named,
                        to_function,
                        false,
                        false,
                    );
                    if reusable {
                        assert_eq!(result, Ok(REUSE_FIELD));
                        expected.graph.nodes.remove(&21);
                        if to_function {
                            expected.graph.nodes.insert(
                                REUSE_CALL,
                                Node::Call {
                                    function: "concat".into(),
                                    args: vec![REUSE_FIELD, 4],
                                },
                            );
                        }
                        reuse_root(&mut expected, named).bindings[0].node =
                            if to_function { REUSE_CALL } else { REUSE_FIELD };
                        assert_eq!(
                            snapshot(&project),
                            snapshot(&expected),
                            "successful reuse preserves ownership and changes only displaced ordinary consumers"
                        );
                        repaired_physical_outputs(&project, named, to_function);
                    } else {
                        assert_eq!(result, Err("mapping node IDs are exhausted".into()));
                        assert_eq!(snapshot(&project), before);
                    }
                }
            }
        }
    }
}
