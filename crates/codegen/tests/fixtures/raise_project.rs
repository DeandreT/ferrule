// Shared hand-authored project; backend hosts retain separate literal outcomes.
fn raise_project() -> mapping::Project {
    use std::collections::BTreeMap;
    use ir::{SchemaNode, ScalarType, Value};
    use mapping::{Binding, FailureIteration, FailureRule, FailureSelection, FunctionId,
        FunctionParameter, FunctionParameterId, Graph, Node, Project, RuntimeValue, Scope,
        ScopeIteration, UserFunction};
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()], frame: Some(vec!["Item".into()]),
    };
    let parameter = FunctionParameterId::new(11);
    let mut nodes = BTreeMap::from([
        (0, field("Value")), (1, field("Denominator")),
        (2, field("Blocked")), (3, field("Message")),
        (4, Node::Call { function: "divide".into(), args: vec![0, 1] }),
        (5, Node::SourceField { path: vec!["Mode".into()], frame: None }),
        (11, Node::Const { value: Value::Null }),
        (12, Node::Const { value: Value::Int(0) }),
        (13, Node::RuntimeValue { value: RuntimeValue::MappingFilePath }),
        (14, Node::Call { function: "divide".into(), args: vec![0, 12] }),
        (25, Node::If { condition: 20, then: 41, else_: 26 }),
        (26, Node::If { condition: 21, then: 42, else_: 27 }),
        (27, Node::If { condition: 22, then: 43, else_: 28 }),
        (28, Node::If { condition: 23, then: 44, else_: 29 }),
        (29, Node::If { condition: 24, then: 55, else_: 40 }),
        (40, Node::Raise { message: Some(3) }),
        (41, Node::Raise { message: None }),
        (42, Node::Raise { message: Some(11) }),
        (43, Node::Raise { message: Some(14) }),
        (44, Node::Raise { message: Some(13) }),
        (50, Node::If { condition: 2, then: 25, else_: 4 }),
        (55, Node::UserFunctionCall { function: FunctionId::new(1), args: vec![3] }),
        (60, Node::SourceField { path: vec!["Global".into()], frame: None }),
    ]);
    for mode in 1..=5 {
        nodes.insert(14 + mode, Node::Const { value: Value::Int(i64::from(mode)) });
        nodes.insert(19 + mode, Node::Call { function: "equal".into(), args: vec![5, 14 + mode] });
    }
    Project {
        source: SchemaNode::group("Input", vec![
            SchemaNode::scalar("Mode", ScalarType::Int),
            SchemaNode::scalar("Global", ScalarType::Bool),
            SchemaNode::group("Item", vec![
                SchemaNode::scalar("Value", ScalarType::Int),
                SchemaNode::scalar("Denominator", ScalarType::Int),
                SchemaNode::scalar("Blocked", ScalarType::Bool),
                SchemaNode::scalar("Message", ScalarType::String),
            ]).repeating(),
        ]),
        target: SchemaNode::group("Output", vec![SchemaNode::group("Row", vec![
            SchemaNode::scalar("Result", ScalarType::Float),
        ]).repeating()]),
        source_path: None, target_path: None,
        source_options: Default::default(), target_options: Default::default(),
        extra_sources: Vec::new(), extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source { collection: vec!["Item".into()] },
            selection: FailureSelection::WhenTrue { predicate: 60 }, message: Some(3),
        }],
        user_functions: BTreeMap::from([(FunctionId::new(1), UserFunction {
            library: "tests".into(), name: "raise-message".into(), description: None,
            parameters: vec![FunctionParameter { id: parameter, name: "message".into(), ty: ScalarType::String }],
            output_name: "result".into(), output_type: ScalarType::String,
            body: Graph { nodes: BTreeMap::from([
                (7, Node::FunctionParameter { parameter }),
                (8, Node::Raise { message: Some(7) }),
            ]) }, output: 8,
        })]),
        graph: Graph { nodes },
        root: Scope { children: vec![Scope {
            target_field: "Row".into(), iteration: ScopeIteration::Source(vec!["Item".into()]),
            bindings: vec![Binding { target_field: "Result".into(), node: 50 }],
            ..Scope::default()
        }], ..Scope::default() },
    }
}
