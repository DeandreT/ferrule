use std::collections::BTreeMap;

use mapping::{FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, UserFunction};

use super::*;

#[test]
fn lowers_only_reachable_functions_with_isolated_node_ids() {
    let increment = FunctionId::new(1);
    let twice = FunctionId::new(2);
    let unused = FunctionId::new(3);
    let increment_input = FunctionParameterId::new(11);
    let twice_input = FunctionParameterId::new(21);
    let function = |name: &str, parameter, body, output| UserFunction {
        library: "tests".into(),
        name: name.into(),
        description: None,
        parameters: vec![FunctionParameter {
            id: parameter,
            name: "value".into(),
            ty: ScalarType::Int,
        }],
        output_name: "result".into(),
        output_type: ScalarType::Int,
        body: Graph { nodes: body },
        output,
    };

    let mut project = supported_project();
    project.user_functions = BTreeMap::from([
        (
            increment,
            function(
                "increment",
                increment_input,
                BTreeMap::from([
                    (
                        1,
                        Node::FunctionParameter {
                            parameter: increment_input,
                        },
                    ),
                    (
                        2,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        3,
                        Node::Call {
                            function: "add".into(),
                            args: vec![1, 2],
                        },
                    ),
                ]),
                3,
            ),
        ),
        (
            twice,
            function(
                "twice",
                twice_input,
                BTreeMap::from([
                    (
                        1,
                        Node::FunctionParameter {
                            parameter: twice_input,
                        },
                    ),
                    (
                        2,
                        Node::UserFunctionCall {
                            function: increment,
                            args: vec![1],
                        },
                    ),
                    (
                        3,
                        Node::UserFunctionCall {
                            function: increment,
                            args: vec![2],
                        },
                    ),
                ]),
                3,
            ),
        ),
        (
            unused,
            function(
                "unused",
                FunctionParameterId::new(31),
                BTreeMap::from([(
                    1,
                    Node::Const {
                        value: Value::Int(0),
                    },
                )]),
                1,
            ),
        ),
    ]);
    project.graph.nodes.insert(
        40,
        Node::UserFunctionCall {
            function: twice,
            args: vec![10],
        },
    );
    project.root.bindings[0].node = 40;

    let program = lower(&project).expect("reachable scalar user functions lower");

    assert_eq!(
        program
            .user_functions
            .iter()
            .map(|function| function.id)
            .collect::<Vec<_>>(),
        vec![increment, twice]
    );
    assert_eq!(
        program.user_functions[0]
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        program
            .expressions
            .last()
            .map(|expression| &expression.expression),
        Some(&Expression::UserFunctionCall {
            function: twice,
            args: vec![10],
        })
    );

    if let Some(Node::Call { function, .. }) = project
        .user_functions
        .get_mut(&increment)
        .and_then(|function| function.body.nodes.get_mut(&3))
    {
        *function = "not_codegen_portable".into();
    }
    assert_eq!(
        lower(&project).map_err(|error| error.into_diagnostics()),
        Err(vec![Diagnostic::Validation {
            location: "user function `tests:increment` (1) body node 3".into(),
            message: "unknown function `not_codegen_portable`".into(),
        }])
    );
}
