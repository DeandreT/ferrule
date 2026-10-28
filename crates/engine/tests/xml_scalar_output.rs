use ir::{
    Instance, ScalarType, SchemaNode, Value, XmlDoubleFields, XmlScalarOutputPolicy,
    XmlScalarOutputUnresolved, XmlScalarOutputUnresolvedReason,
};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Amount", ScalarType::Float)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: FormatOptions {
            xml_document: true,
            xml_scalar_output: Some(XmlScalarOutputPolicy::SuppressCastInSubtreeV1 {
                declarations: XmlDoubleFields::new(vec!["Amount".into()]).unwrap(),
            }),
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                1,
                Node::Const {
                    value: Value::Int(17),
                },
            )]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Amount".into(),
                node: 1,
            }],
            ..Default::default()
        },
    }
}

#[test]
fn suppression_preserves_integer_tag_and_default_execution_still_adapts() {
    let mut project = project();
    let source = Instance::Group(vec![].into());
    assert!(engine::validate(&project).is_empty());
    let result = engine::run(&project, &source).unwrap();
    assert_eq!(
        result.field("Amount"),
        Some(&Instance::Scalar(Value::Int(17)))
    );
    project.target_options.xml_scalar_output = None;
    assert_eq!(
        engine::run(&project, &source).unwrap().field("Amount"),
        Some(&Instance::Scalar(Value::Float(17.0)))
    );
}

#[test]
fn unresolved_requests_refuse_before_a_failing_expression_for_both_dispatches() {
    let mut project = project();
    project.target_options.xml_scalar_output = Some(XmlScalarOutputPolicy::Unresolved {
        request: XmlScalarOutputUnresolved::for_reason(
            XmlScalarOutputUnresolvedReason::UnsupportedShape,
        ),
    });
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "divide".into(),
            args: vec![2, 3],
        },
    );
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(1),
        },
    );
    project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Int(0),
        },
    );
    let source = Instance::Scalar(Value::Null);
    assert!(matches!(
        engine::run(&project, &source),
        Err(engine::EngineError::XmlScalarOutput(_))
    ));
    assert!(matches!(
        engine::run_selected_target(&project, &source, engine::TargetSelection::Primary),
        Err(engine::EngineError::XmlScalarOutput(_))
    ));
}

#[test]
fn malformed_deep_scope_is_rejected_before_cloning_or_execution() {
    let mut project = project();
    let mut child = Scope::default();
    for _ in 0..6000 {
        child = Scope {
            children: vec![child],
            ..Default::default()
        };
    }
    project.root.children.push(child);
    assert!(matches!(
        engine::run(&project, &Instance::Scalar(Value::Null)),
        Err(engine::EngineError::XmlScalarOutput(_))
    ));
    std::mem::forget(project);
}
