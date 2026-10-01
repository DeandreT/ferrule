use std::collections::BTreeMap;

use mapping::{
    DelimitedDialect, DelimitedRecordField, FlexCommand, FlexLineEnding, FlexTextLayout,
    FunctionId, FunctionParameter, FunctionParameterId, Graph, NamedTarget, UserFunction,
};

use super::*;

fn layout(fields: &[(&str, ScalarType)]) -> String {
    let layout = FlexTextLayout::new(
        "Root",
        FlexCommand::DelimitedRecords {
            name: "Row".into(),
            dialect: DelimitedDialect::new(',', "\n", '"', '"').unwrap(),
            fields: fields
                .iter()
                .map(|(name, ty)| DelimitedRecordField::new(*name, *ty).unwrap())
                .collect(),
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    serde_json::to_string(&layout).unwrap()
}

fn nested_graph() -> Graph {
    Graph {
        nodes: BTreeMap::from([
            (
                1,
                Node::SourceField {
                    path: vec!["Raw".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String(layout(&[
                        ("Name", ScalarType::String),
                        ("Payload", ScalarType::String),
                    ])),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String(r#"["Row","Payload"]"#.into()),
                },
            ),
            (
                4,
                Node::Call {
                    function: "flextext_parse_field".into(),
                    args: vec![1, 2, 3],
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::String(layout(&[("Number", ScalarType::Int)])),
                },
            ),
            (
                6,
                Node::Const {
                    value: Value::String(r#"["Row","Number"]"#.into()),
                },
            ),
            (
                7,
                Node::Call {
                    function: "flextext_parse_field".into(),
                    args: vec![4, 5, 6],
                },
            ),
        ]),
    }
}

fn project() -> Project {
    let mut project = supported_project();
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Raw", ScalarType::String)],
    );
    project.target = SchemaNode::group("Target", vec![SchemaNode::scalar("Out", ScalarType::Int)]);
    project.graph = nested_graph();
    project.root = Scope {
        bindings: vec![MappingBinding {
            target_field: "Out".into(),
            node: 7,
        }],
        ..Scope::default()
    };
    project
}

fn ids(program: &crate::Program) -> Vec<u32> {
    program.expressions.iter().map(|node| node.id).collect()
}

#[test]
fn nested_projections_keep_only_runtime_input_edges() {
    let project = project();
    let program = lower(&project).unwrap();
    assert_eq!(ids(&program), [1, 4, 7]);
    for id in [4, 7] {
        let expression = program
            .expressions
            .iter()
            .find(|node| node.id == id)
            .unwrap();
        let Expression::DelimitedTextField { parser, .. } = &expression.expression else {
            panic!("projection retains its validated descriptor");
        };
        assert!(!parser.layout_descriptor().is_empty());
        assert!(!parser.path_descriptor().is_empty());
    }
    let input = ir::Instance::Group(vec![(
        "Raw".into(),
        ir::Instance::Scalar(Value::String("label,7".into())),
    )]);
    let output = engine::run(&project, &input).unwrap();
    assert_eq!(
        output.field("Out").unwrap().as_scalar(),
        Some(&Value::Int(7))
    );
}

#[test]
fn metadata_shared_with_a_binding_remains_a_runtime_expression() {
    let mut project = project();
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("Path", ScalarType::String));
    project.root.bindings.push(MappingBinding {
        target_field: "Path".into(),
        node: 3,
    });
    assert_eq!(ids(&lower(&project).unwrap()), [1, 3, 4, 7]);
}

#[test]
fn metadata_shared_with_an_ordinary_call_remains_reachable() {
    let mut project = project();
    project.graph.nodes.insert(
        8,
        Node::Call {
            function: "length".into(),
            args: vec![3],
        },
    );
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("Length", ScalarType::Int));
    project.root.bindings.push(MappingBinding {
        target_field: "Length".into(),
        node: 8,
    });
    assert_eq!(ids(&lower(&project).unwrap()), [1, 3, 4, 7, 8]);
}

#[test]
fn named_targets_keep_their_independently_used_metadata() {
    let mut project = project();
    project.extra_targets.push(NamedTarget {
        name: "Metadata".into(),
        schema: SchemaNode::group(
            "Metadata",
            vec![SchemaNode::scalar("Layout", ScalarType::String)],
        ),
        path: None,
        options: Default::default(),
        root: Scope {
            bindings: vec![MappingBinding {
                target_field: "Layout".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    });
    assert_eq!(ids(&lower(&project).unwrap()), [1, 2, 4, 7]);
}

#[test]
fn user_function_projections_use_their_own_runtime_reachability() {
    let mut project = project();
    let function = FunctionId::new(1);
    let parameter = FunctionParameterId::new(1);
    let mut body = nested_graph();
    body.nodes.insert(1, Node::FunctionParameter { parameter });
    project.user_functions.insert(
        function,
        UserFunction {
            library: "tests".into(),
            name: "parse_nested_record".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: parameter,
                name: "raw".into(),
                ty: ScalarType::String,
            }],
            output_name: "number".into(),
            output_type: ScalarType::Int,
            body,
            output: 7,
        },
    );
    project.graph.nodes.retain(|id, _| *id == 1);
    project.graph.nodes.insert(
        8,
        Node::UserFunctionCall {
            function,
            args: vec![1],
        },
    );
    project.root.bindings[0].node = 8;
    let program = lower(&project).unwrap();
    assert_eq!(ids(&program), [1, 8]);
    assert_eq!(
        program.user_functions[0]
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [1, 4, 7]
    );
}

#[test]
fn invalid_and_nonliteral_descriptors_still_reject() {
    for replacement in [
        Node::Const {
            value: Value::String("invalid descriptor".into()),
        },
        Node::SourceField {
            frame: None,
            path: vec!["Raw".into()],
        },
    ] {
        let mut project = project();
        project.graph.nodes.insert(2, replacement);
        let error = lower(&project).unwrap_err();
        assert!(error.diagnostics().iter().any(|diagnostic|
            matches!(diagnostic, Diagnostic::UnsupportedFunction { node: 4, function } if function == "flextext_parse_field")));
    }
}

#[test]
fn ordinary_json_projection_retains_every_runtime_argument() {
    let mut project = project();
    project.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: vec!["Raw".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(
                    serde_json::to_string(&SchemaNode::scalar("Number", ScalarType::Int)).unwrap(),
                ),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("[]".into()),
            },
        ),
        (
            4,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![1, 2, 3],
            },
        ),
    ]);
    project.root.bindings[0].node = 4;
    assert_eq!(ids(&lower(&project).unwrap()), [1, 2, 3, 4]);
}
