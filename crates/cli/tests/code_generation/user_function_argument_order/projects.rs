use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, Project, Scope,
    UserFunction,
};

pub(super) const CONTROLS: [&str; 6] = [
    "FirstRaise",
    "SecondRaise",
    "FirstBad",
    "SecondBad",
    "FirstNull",
    "Skip",
];

pub(super) fn first_function() -> UserFunction {
    UserFunction {
        library: "argument-order".into(),
        name: "first".into(),
        description: None,
        parameters: vec![
            FunctionParameter {
                id: FunctionParameterId::new(11),
                name: "first".into(),
                ty: ScalarType::Int,
            },
            FunctionParameter {
                id: FunctionParameterId::new(12),
                name: "second".into(),
                ty: ScalarType::Int,
            },
        ],
        output_name: "result".into(),
        output_type: ScalarType::Int,
        body: Graph {
            nodes: BTreeMap::from([(
                1,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(11),
                },
            )]),
        },
        output: 1,
    }
}

fn base(source: SchemaNode, nodes: BTreeMap<u32, Node>) -> Project {
    Project {
        source,
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Result", ScalarType::Int)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::from([(FunctionId::new(7), first_function())]),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 3,
            }],
            ..Scope::default()
        },
    }
}

pub(super) fn literal_project(nested: bool) -> Project {
    let nodes = BTreeMap::from([
        (
            1,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (2, Node::Raise { message: None }),
        (
            3,
            Node::UserFunctionCall {
                function: FunctionId::new(7),
                args: vec![1, 2],
            },
        ),
    ]);
    let mut project = base(
        SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Unused", ScalarType::String)],
        ),
        nodes,
    );
    if nested {
        let body = std::mem::take(&mut project.graph);
        project.user_functions.insert(
            FunctionId::new(8),
            UserFunction {
                library: "argument-order".into(),
                name: "outer-literal".into(),
                description: None,
                parameters: Vec::new(),
                output_name: "result".into(),
                output_type: ScalarType::Int,
                body,
                output: 3,
            },
        );
        project.graph.nodes.insert(
            3,
            Node::UserFunctionCall {
                function: FunctionId::new(8),
                args: Vec::new(),
            },
        );
    }
    project
}

pub(super) fn controlled_project(nested: bool) -> Project {
    let mut nodes = BTreeMap::from([
        (
            1,
            Node::If {
                condition: 11,
                then: 21,
                else_: 2,
            },
        ),
        (
            2,
            Node::If {
                condition: 13,
                then: 31,
                else_: 3,
            },
        ),
        (
            3,
            Node::If {
                condition: 15,
                then: 33,
                else_: 32,
            },
        ),
        (
            4,
            Node::If {
                condition: 12,
                then: 22,
                else_: 5,
            },
        ),
        (
            5,
            Node::If {
                condition: 14,
                then: 31,
                else_: 34,
            },
        ),
        (
            6,
            Node::UserFunctionCall {
                function: FunctionId::new(7),
                args: vec![1, 4],
            },
        ),
        (
            7,
            Node::If {
                condition: 16,
                then: 35,
                else_: 6,
            },
        ),
        (21, Node::Raise { message: Some(36) }),
        (22, Node::Raise { message: None }),
        (
            31,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            32,
            Node::Const {
                value: Value::String(" 42 ".into()),
            },
        ),
        (33, Node::Const { value: Value::Null }),
        (
            34,
            Node::Const {
                value: Value::String("7".into()),
            },
        ),
        (
            35,
            Node::Const {
                value: Value::Int(99),
            },
        ),
        (
            36,
            Node::Const {
                value: Value::String("first-expression".into()),
            },
        ),
    ]);
    for (index, name) in CONTROLS.iter().enumerate() {
        nodes.insert(
            11 + index as u32,
            Node::SourceField {
                path: vec![(*name).into()],
                frame: None,
            },
        );
    }
    let mut project = base(
        SchemaNode::group(
            "Source",
            CONTROLS
                .iter()
                .map(|name| SchemaNode::scalar(*name, ScalarType::Bool))
                .collect(),
        ),
        nodes,
    );
    project.root.bindings[0].node = 7;
    if nested {
        let mut body = std::mem::take(&mut project.graph);
        let mut parameters = Vec::new();
        for (index, name) in CONTROLS.iter().enumerate() {
            let id = FunctionParameterId::new(101 + index as u64);
            parameters.push(FunctionParameter {
                id,
                name: (*name).into(),
                ty: ScalarType::Bool,
            });
            body.nodes
                .insert(11 + index as u32, Node::FunctionParameter { parameter: id });
            project.graph.nodes.insert(
                11 + index as u32,
                Node::SourceField {
                    path: vec![(*name).into()],
                    frame: None,
                },
            );
        }
        project.user_functions.insert(
            FunctionId::new(8),
            UserFunction {
                library: "argument-order".into(),
                name: "outer-controls".into(),
                description: None,
                parameters,
                output_name: "result".into(),
                output_type: ScalarType::Int,
                body,
                output: 7,
            },
        );
        project.graph.nodes.insert(
            7,
            Node::UserFunctionCall {
                function: FunctionId::new(8),
                args: (11..=16).collect(),
            },
        );
    }
    project
}

pub(super) fn source(flags: [bool; 6]) -> Instance {
    Instance::Group(
        CONTROLS
            .into_iter()
            .zip(flags)
            .map(|(name, value)| (name.into(), Instance::Scalar(Value::Bool(value))))
            .collect::<Vec<_>>()
            .into(),
    )
}

pub(super) fn output(value: Value) -> Instance {
    Instance::Group(vec![("Result".into(), Instance::Scalar(value))].into())
}

pub(super) fn cases() -> Vec<(
    &'static str,
    [bool; 6],
    Result<Instance, engine::EngineError>,
)> {
    use engine::EngineError;
    let parameter = |id| EngineError::UserFunctionParameterType {
        function: FunctionId::new(7),
        parameter: FunctionParameterId::new(id),
        expected: ScalarType::Int,
        found: "bool",
    };
    vec![
        (
            "later-expression-before-adaptation",
            [false, true, true, false, false, false],
            Err(EngineError::MappingException {
                node: 22,
                message: None,
            }),
        ),
        (
            "two-expression-failures",
            [true, true, false, false, false, false],
            Err(EngineError::MappingException {
                node: 21,
                message: Some("first-expression".into()),
            }),
        ),
        (
            "two-adaptation-failures",
            [false, false, true, true, false, false],
            Err(parameter(11)),
        ),
        (
            "second-adaptation-failure",
            [false, false, false, true, false, false],
            Err(parameter(12)),
        ),
        (
            "successful-coercion",
            [false; 6],
            Ok(output(Value::Int(42))),
        ),
        (
            "null-preserved",
            [false, false, false, false, true, false],
            Ok(output(Value::Null)),
        ),
        (
            "unselected-lazy-call",
            [true, true, true, true, false, true],
            Ok(output(Value::Int(99))),
        ),
        (
            "first-expression-before-second-adaptation",
            [true, false, false, true, false, false],
            Err(EngineError::MappingException {
                node: 21,
                message: Some("first-expression".into()),
            }),
        ),
    ]
}
