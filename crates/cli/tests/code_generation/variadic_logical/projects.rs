use std::collections::BTreeMap;

use ir::{Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

// One generated library exercises every admitted arity without changing its graph.
pub(super) fn project() -> Project {
    let mut nodes = BTreeMap::new();
    let types = ScalarTypeSet::new([ScalarType::Bool, ScalarType::String, ScalarType::Int])
        .expect("distinct authored scalar union");
    let mut fields = Vec::new();
    for index in 1..=5 {
        fields.push(SchemaNode::scalar_union(format!("Arg{index}"), types));
        fields.push(SchemaNode::scalar(
            format!("Raise{index}"),
            ScalarType::Bool,
        ));
        nodes.insert(
            index,
            Node::SourceField {
                path: vec![format!("Arg{index}")],
                frame: None,
            },
        );
        nodes.insert(
            10 + index,
            Node::SourceField {
                path: vec![format!("Raise{index}")],
                frame: None,
            },
        );
        nodes.insert(20 + index, Node::Raise { message: None });
        nodes.insert(
            30 + index,
            Node::If {
                condition: 10 + index,
                then: 20 + index,
                else_: index,
            },
        );
    }
    fields.push(SchemaNode::scalar("Arity", ScalarType::Int));
    fields.push(SchemaNode::scalar("Or", ScalarType::Bool));
    nodes.insert(
        40,
        Node::SourceField {
            path: vec!["Arity".into()],
            frame: None,
        },
    );
    nodes.insert(
        41,
        Node::SourceField {
            path: vec!["Or".into()],
            frame: None,
        },
    );
    for count in 2..=5 {
        nodes.insert(
            50 + count,
            Node::Const {
                value: Value::Int(i64::from(count)),
            },
        );
        nodes.insert(
            60 + count,
            Node::Call {
                function: "equal".into(),
                args: vec![40, 50 + count],
            },
        );
        for (function, offset) in [("and", 100), ("or", 200)] {
            nodes.insert(
                offset + count,
                Node::Call {
                    function: function.into(),
                    args: (31..31 + count).collect(),
                },
            );
        }
    }
    for offset in [100, 200] {
        // Unselected arity branches remain lazy, while a selected Call is eager.
        for count in (2..=4).rev() {
            nodes.insert(
                offset + 10 + count,
                Node::If {
                    condition: 60 + count,
                    then: offset + count,
                    else_: if count == 4 {
                        offset + 5
                    } else {
                        offset + 11 + count
                    },
                },
            );
        }
    }
    nodes.insert(
        300,
        Node::If {
            condition: 41,
            then: 212,
            else_: 112,
        },
    );
    Project {
        source: SchemaNode::group("Source", fields),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Result", ScalarType::Bool)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 300,
            }],
            ..Scope::default()
        },
    }
}

pub(super) fn scalar(token: &str) -> Value {
    match token {
        "T" => Value::Bool(true),
        "F" => Value::Bool(false),
        "N" => Value::Null,
        "S" => Value::String("authored-string".into()),
        "I" => Value::Int(7),
        _ => panic!("unknown frozen scalar token"),
    }
}

pub(super) fn source(columns: &[&str]) -> Instance {
    let mut fields = Vec::new();
    for index in 0..5 {
        fields.push((
            format!("Arg{}", index + 1),
            Instance::Scalar(scalar(columns[3 + index])),
        ));
        fields.push((
            format!("Raise{}", index + 1),
            Instance::Scalar(Value::Bool(columns[8].as_bytes()[index] == b'1')),
        ));
    }
    fields.push((
        "Arity".into(),
        Instance::Scalar(Value::Int(columns[1].parse().expect("frozen integer"))),
    ));
    fields.push((
        "Or".into(),
        Instance::Scalar(Value::Bool(columns[2] == "or")),
    ));
    Instance::Group(fields.into())
}

// InstanceGroup Debug and ordinary serialization omit runtime XML provenance.
pub(super) fn origins(value: &Instance) -> Vec<(String, String)> {
    fn visit(value: &Instance, path: String, output: &mut Vec<(String, String)>) {
        match value {
            Instance::Group(fields) => {
                output.push((path.clone(), format!("{:?}", fields.xml_type_origin())));
                for (index, (name, child)) in fields.iter().enumerate() {
                    visit(child, format!("{path}/{index}:{name}"), output);
                }
            }
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                for (index, child) in items.iter().enumerate() {
                    visit(child, format!("{path}/{index}"), output);
                }
            }
            Instance::DocumentSet(documents) => {
                for (index, document) in documents.iter().enumerate() {
                    visit(document.value(), format!("{path}/document{index}"), output);
                }
            }
            Instance::Scalar(_) => {}
        }
    }
    let mut output = Vec::new();
    visit(value, "root".into(), &mut output);
    output
}
