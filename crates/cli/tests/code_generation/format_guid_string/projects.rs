use ir::{Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};
use std::collections::BTreeMap;

pub(super) fn project() -> Project {
    let source_types = ScalarTypeSet::new([
        ScalarType::String,
        ScalarType::Bool,
        ScalarType::Int,
        ScalarType::Float,
    ])
    .expect("distinct authored types");
    Project {
        source: SchemaNode::group("Input", vec![SchemaNode::scalar_union("Hex", source_types)]),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Formatted", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Hex".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "format_guid_string".into(),
                        args: vec![1],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Formatted".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    }
}

pub(super) fn argument(row: &[&str]) -> Value {
    match row[1] {
        "S" => Value::String(row[2].into()),
        "N" => Value::Null,
        "J" => Value::json_null(),
        "X" => Value::xml_nil(),
        "B" => Value::Bool(true),
        "I" => Value::Int(7),
        "F" => Value::Float(0.5),
        _ => panic!("unknown authored scalar kind"),
    }
}

pub(super) fn expected(text: &str) -> Result<Instance, engine::EngineError> {
    use codegen_runtime::FunctionError;
    let error = match text {
        "invalid" => Some(FunctionError::InvalidArgument {
            function: "format_guid_string",
            message: "requires exactly 32 ASCII hexadecimal characters",
        }),
        "type:null" | "type:json null" | "type:xml nil" | "type:bool" | "type:int"
        | "type:float" => Some(FunctionError::TypeMismatch {
            function: "format_guid_string",
            got: match text {
                "type:null" => "null",
                "type:json null" => "json null",
                "type:xml nil" => "xml nil",
                "type:bool" => "bool",
                "type:int" => "int",
                _ => "float",
            },
        }),
        _ => None,
    };
    match error {
        Some(error) => Err(engine::EngineError::Function(error)),
        None => Ok(Instance::Group(
            vec![(
                "Formatted".into(),
                Instance::Scalar(Value::String(text.into())),
            )]
            .into(),
        )),
    }
}
