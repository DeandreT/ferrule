use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, Project, Scope,
    UserFunction,
};

use crate::{EngineError, run, validate};

fn parameter(id: u64, name: &str, ty: ScalarType) -> FunctionParameter {
    FunctionParameter {
        id: FunctionParameterId::new(id),
        name: name.into(),
        ty,
    }
}

fn function(
    name: &str,
    parameters: Vec<FunctionParameter>,
    output_type: ScalarType,
    nodes: impl IntoIterator<Item = (u32, Node)>,
    output: u32,
) -> UserFunction {
    UserFunction {
        library: "tests".into(),
        name: name.into(),
        description: None,
        parameters,
        output_name: "result".into(),
        output_type,
        body: Graph {
            nodes: nodes.into_iter().collect(),
        },
        output,
    }
}

fn project(
    graph: Graph,
    user_functions: BTreeMap<FunctionId, UserFunction>,
    output: u32,
) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("result", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions,
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "result".into(),
                node: output,
            }],
            ..Scope::default()
        },
    }
}

fn source(value: &str) -> Instance {
    Instance::Group(vec![(
        "value".into(),
        Instance::Scalar(Value::String(value.into())),
    )])
}

fn output_value(output: &Instance) -> Option<&Value> {
    output.field("result").and_then(Instance::as_scalar)
}

#[test]
fn evaluates_nested_functions_with_isolated_parameters_and_coercion() {
    let increment = FunctionId::new(2);
    let parse_and_increment = FunctionId::new(1);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        increment,
        function(
            "increment",
            vec![parameter(1, "number", ScalarType::Int)],
            ScalarType::Int,
            [
                (
                    0,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "add".into(),
                        args: vec![0, 1],
                    },
                ),
            ],
            2,
        ),
    );
    user_functions.insert(
        parse_and_increment,
        function(
            "parse_and_increment",
            vec![parameter(8, "text", ScalarType::Int)],
            ScalarType::String,
            [
                (
                    10,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(8),
                    },
                ),
                (
                    11,
                    Node::UserFunctionCall {
                        function: increment,
                        args: vec![10],
                    },
                ),
            ],
            11,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::SourceField {
                    path: vec!["value".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: parse_and_increment,
                    args: vec![0],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let project = project(graph, user_functions, 1);

    assert!(validate(&project).is_empty());
    let output = run(&project, &source("41")).unwrap();
    assert_eq!(output_value(&output), Some(&Value::String("42".into())));
}

#[test]
fn evaluates_only_the_selected_function_branch() {
    let choose = FunctionId::new(1);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        choose,
        function(
            "choose",
            vec![parameter(1, "condition", ScalarType::Bool)],
            ScalarType::String,
            [
                (
                    0,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("selected".into()),
                    },
                ),
                (
                    2,
                    Node::UserFunctionCall {
                        function: FunctionId::new(999),
                        args: Vec::new(),
                    },
                ),
                (
                    3,
                    Node::If {
                        condition: 0,
                        then: 1,
                        else_: 2,
                    },
                ),
            ],
            3,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: choose,
                    args: vec![0],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let output = run(&project(graph, user_functions, 1), &source("unused")).unwrap();
    assert_eq!(
        output_value(&output),
        Some(&Value::String("selected".into()))
    );
}

#[test]
fn reports_the_first_parameter_that_cannot_be_adapted() {
    let function_id = FunctionId::new(1);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        function_id,
        function(
            "typed",
            vec![
                parameter(11, "first", ScalarType::Int),
                parameter(12, "second", ScalarType::Int),
            ],
            ScalarType::Int,
            [(
                0,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(11),
                },
            )],
            0,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::String("not-an-int".into()),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("also-not-an-int".into()),
                },
            ),
            (
                2,
                Node::UserFunctionCall {
                    function: function_id,
                    args: vec![0, 1],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    assert_eq!(
        run(&project(graph, user_functions, 2), &source("unused")),
        Err(EngineError::UserFunctionParameterType {
            function: function_id,
            parameter: FunctionParameterId::new(11),
            expected: ScalarType::Int,
            found: "string",
        })
    );
}

#[test]
fn guards_recursive_calls_even_without_prevalidation() {
    let recursive = FunctionId::new(7);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        recursive,
        function(
            "recursive",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::UserFunctionCall {
                    function: recursive,
                    args: Vec::new(),
                },
            )],
            0,
        ),
    );
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: recursive,
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };

    assert_eq!(
        run(&project(graph, user_functions, 0), &source("unused")),
        Err(EngineError::UserFunctionCycle {
            function: recursive
        })
    );
}

#[test]
fn validates_function_boundaries_bodies_and_call_cycles() {
    let first = FunctionId::new(1);
    let second = FunctionId::new(2);
    let duplicated = parameter(3, "value", ScalarType::String);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        first,
        function(
            "first",
            vec![duplicated.clone(), duplicated],
            ScalarType::String,
            [
                (
                    0,
                    Node::SourceField {
                        path: vec!["value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::UserFunctionCall {
                        function: second,
                        args: Vec::new(),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![2],
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![77],
                    },
                ),
                (
                    4,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(999),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "not-a-function".into(),
                        args: Vec::new(),
                    },
                ),
            ],
            99,
        ),
    );
    user_functions.insert(
        second,
        function(
            "second",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::UserFunctionCall {
                    function: first,
                    args: Vec::new(),
                },
            )],
            0,
        ),
    );
    user_functions.insert(
        FunctionId::new(3),
        function(
            "first",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::Const {
                    value: Value::String("duplicate name".into()),
                },
            )],
            0,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(3),
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: first,
                    args: Vec::new(),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let issues = validate(&project(graph, user_functions, 1));
    let messages = issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(messages.contains("valid only inside a user-defined function"));
    assert!(messages.contains("expects 2 argument(s), got 0"));
    assert!(messages.contains("parameter id 3 is duplicated"));
    assert!(messages.contains("parameter name `value` is duplicated"));
    assert!(messages.contains("function library and name duplicate function 1"));
    assert!(messages.contains("output references missing body node 99"));
    assert!(messages.contains("references missing body node 77"));
    assert!(messages.contains("references undeclared parameter 999"));
    assert!(messages.contains("unknown function `not-a-function`"));
    assert!(messages.contains("cycle reaches body node 2"));
    assert!(messages.contains("node kind is not supported"));
    assert!(messages.contains("recursive user-defined function calls are not supported"));
}

#[test]
fn validates_the_function_call_depth_limit() {
    let mut user_functions = BTreeMap::new();
    for index in 0..=64_u64 {
        let id = FunctionId::new(index);
        let node = if index == 64 {
            Node::Const {
                value: Value::String("end".into()),
            }
        } else {
            Node::UserFunctionCall {
                function: FunctionId::new(index + 1),
                args: Vec::new(),
            }
        };
        user_functions.insert(
            id,
            function(
                &format!("function_{index}"),
                Vec::new(),
                ScalarType::String,
                [(0, node)],
                0,
            ),
        );
    }
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: FunctionId::new(0),
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };

    assert!(
        validate(&project(graph, user_functions, 0))
            .iter()
            .any(|issue| {
                issue
                    .message
                    .contains("call nesting exceeds the limit of 64 functions")
            })
    );
}
