use super::*;

pub(super) const FIXTURES: [&str; 10] = [
    "I",
    "F",
    "S",
    "N",
    "U",
    "MessageError",
    "MessageConstant",
    "MessageNone",
    "MessageEmpty",
    "Context",
];

pub(super) fn project(fixture: &str) -> Project {
    let (ty, name) = match fixture {
        "F" | "N" => (ScalarType::Float, "n"),
        "S" => (ScalarType::String, "n"),
        "U" => (ScalarType::Int, "雪"),
        "Context" => (ScalarType::String, "Path"),
        _ => (ScalarType::Int, "n"),
    };
    let mut leaf = SchemaNode::scalar(name, ty);
    leaf.nullable = fixture == "N";
    let mut candidate = Project {
        source: SchemaNode::group("Input", vec![leaf.clone()]),
        target: SchemaNode::group("Output", vec![leaf]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                1,
                Node::SourceField {
                    path: vec![name.into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: name.into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    if !fixture.starts_with("Message") && fixture != "Context" {
        return candidate;
    }
    candidate.source = SchemaNode::group("Input", Vec::new());
    candidate.graph.nodes.clear();
    candidate.root.bindings[0].node = 3;
    if fixture == "Context" {
        candidate.graph.nodes.insert(
            3,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::MappingFilePath,
            },
        );
        return candidate;
    }
    candidate.graph.nodes.insert(
        3,
        Node::Raise {
            message: if fixture == "MessageNone" {
                None
            } else {
                Some(5)
            },
        },
    );
    if fixture == "MessageNone" {
        return candidate;
    }
    if fixture == "MessageEmpty" {
        candidate.graph.nodes.insert(
            5,
            Node::Const {
                value: Value::String(String::new()),
            },
        );
        return candidate;
    }
    candidate.source =
        SchemaNode::group("Input", vec![SchemaNode::scalar("Fail", ScalarType::Bool)]);
    candidate.root.bindings[0].node = 6;
    candidate.graph.nodes.extend([
        (
            1,
            Node::SourceField {
                path: vec!["Fail".into()],
                frame: None,
            },
        ),
        (
            6,
            Node::If {
                condition: 1,
                then: 3,
                else_: 7,
            },
        ),
        (
            7,
            Node::Const {
                value: Value::Int(7),
            },
        ),
    ]);
    if fixture == "MessageConstant" {
        candidate.graph.nodes.insert(
            5,
            Node::Const {
                value: Value::String("stop".into()),
            },
        );
    } else {
        candidate.graph.nodes.extend([
            (
                2,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                4,
                Node::Const {
                    value: Value::Int(0),
                },
            ),
            (
                5,
                Node::Call {
                    function: "divide".into(),
                    args: vec![2, 4],
                },
            ),
        ]);
    }
    candidate
}
