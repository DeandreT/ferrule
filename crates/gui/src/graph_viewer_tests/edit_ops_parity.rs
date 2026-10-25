//! Compare headless operations with the actual canvas mutation paths, not a
//! second hand-written expected graph. IDs allocated by the canvas are passed
//! explicitly to the headless operation, as a serialized caller would do.
use super::*;
use edit_ops::{BindingRef, EditOperation, EditSession, Expression, Index, ScopeRef, Target};

fn project(fx: &Fixture) -> mapping::Project {
    mapping::Project {
        source: SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        ),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("out", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: fx.graph.clone(),
        root: fx.root_scope.clone(),
    }
}
fn root() -> ScopeRef {
    ScopeRef {
        target: Target::Primary {},
        children: vec![],
    }
}
fn target_binding() -> BindingRef {
    BindingRef {
        scope: root(),
        field: "out".into(),
    }
}
fn compare(before: mapping::Project, fx: &Fixture, op: EditOperation) {
    let key = |p: &mapping::Project| mapping::project_file::encode_pretty(p).unwrap();
    let mut session = EditSession::new(before.clone());
    session.apply(op).unwrap();
    assert_eq!(key(session.project()), key(&project(fx)));
    session.undo().unwrap();
    assert_eq!(key(session.project()), key(&before));
}

#[test]
fn source_binding_matches_canvas() {
    let mut fx = fixture();
    let before = project(&fx);
    let mut snarl = std::mem::take(&mut fx.snarl);
    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.target,
        input: 0,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    let id = fx.root_scope.bindings[0].node;
    compare(
        before,
        &fx,
        EditOperation::Bind {
            binding: target_binding(),
            expression: Expression::Source {
                id: edit_ops::NodeId(id),
                path: vec!["name".into()],
                frame: None,
            },
        },
    );
}

#[test]
fn node_connection_and_disconnect_match_canvas_hidden_inputs() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let old = fx.viewer().fresh_unconnected().unwrap();
    let Node::Call { args, .. } = fx.graph.nodes.get_mut(&0).unwrap() else {
        panic!()
    };
    args.push(old);
    let before = project(&fx);
    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    let id = fx.graph.nodes[&0].evaluated_inputs()[0];
    compare(
        before,
        &fx,
        EditOperation::Connect {
            node: edit_ops::NodeId(0),
            input: Index(0),
            expression: Expression::Source {
                id: edit_ops::NodeId(id),
                path: vec!["name".into()],
                frame: None,
            },
        },
    );
    let before = project(&fx);
    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().disconnect(&from, &to, &mut snarl);
    let replacement = fx.graph.nodes[&0].evaluated_inputs()[0];
    compare(
        before,
        &fx,
        EditOperation::Disconnect {
            node: edit_ops::NodeId(0),
            input: Index(0),
            replacement: Some(edit_ops::NodeId(replacement)),
        },
    );
}

#[test]
fn node_removal_matches_canvas_orphan_cleanup() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let (id, visual) = fx
        .viewer()
        .insert_with_unconnected_inputs(&mut snarl, egui::pos2(600.0, 300.0), 3, |inputs| {
            Node::If {
                condition: inputs[0],
                then: inputs[1],
                else_: inputs[2],
            }
        })
        .unwrap();
    let before = project(&fx);
    assert!(fx.viewer().remove_graph_node(id, visual, &mut snarl));
    compare(
        before,
        &fx,
        EditOperation::RemoveNodes {
            scope: root(),
            nodes: vec![edit_ops::NodeId(id)],
            replacements: vec![],
        },
    );
}

#[test]
fn property_edits_match_the_canvas_staged_commit() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Float(-0.0),
        },
    );
    let before = project(&fx);
    fx.viewer()
        .commit_node_property_edit(
            1,
            Node::Const {
                value: ir::Value::Int(9007199254740993),
            },
            false,
            false,
        )
        .unwrap();
    compare(
        before,
        &fx,
        EditOperation::EditConstant {
            node: edit_ops::NodeId(1),
            value: edit_ops::Scalar::Int {
                text: "9007199254740993".into(),
            },
        },
    );
    let before = project(&fx);
    fx.viewer()
        .commit_node_property_edit(
            0,
            Node::Call {
                function: "upper".into(),
                args: vec![1],
            },
            true,
            false,
        )
        .unwrap();
    compare(
        before,
        &fx,
        EditOperation::EditCall {
            node: edit_ops::NodeId(0),
            function: "upper".into(),
            args: vec![edit_ops::NodeId(1)],
        },
    );
    fx.graph.nodes.insert(
        2,
        Node::ValueMap {
            input: 1,
            input_type: None,
            table: vec![],
            default: None,
        },
    );
    let before = project(&fx);
    fx.viewer()
        .commit_node_property_edit(
            2,
            Node::ValueMap {
                input: 1,
                input_type: Some(ScalarType::Int),
                table: vec![(ir::Value::Int(1), ir::Value::Float(-0.0))],
                default: Some(ir::Value::Null),
            },
            false,
            false,
        )
        .unwrap();
    compare(
        before,
        &fx,
        EditOperation::EditValueMap {
            node: edit_ops::NodeId(2),
            value: edit_ops::ValueMap {
                input_type: Some(edit_ops::ScalarType::Int {}),
                table: vec![edit_ops::ValueMapEntry {
                    from: edit_ops::Scalar::Int { text: "1".into() },
                    to: edit_ops::Scalar::Float {
                        text: "-0.0".into(),
                    },
                }],
                default: Some(edit_ops::Scalar::Null {}),
            },
        },
    );
}
