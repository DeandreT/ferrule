use super::*;
use ir::{Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeConstruction};

fn execute(types: [ScalarType; 2], value: Value) -> Result<Value, EngineError> {
    let Some(types) = ScalarTypeSet::new(types) else {
        panic!("test scalar union must contain two distinct types");
    };
    let project = Project {
        source: SchemaNode::group("source", Vec::new()),
        target: SchemaNode::group("target", vec![SchemaNode::scalar_union("value", types)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(1, Node::Const { value })].into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    run(&project, &Instance::Group(Vec::new()))?
        .field("value")
        .and_then(Instance::as_scalar)
        .cloned()
        .ok_or_else(|| EngineError::MissingSourceField("value".into()))
}

fn execute_scalar_scope(types: [ScalarType; 2], value: Value) -> Result<Value, EngineError> {
    let Some(types) = ScalarTypeSet::new(types) else {
        panic!("test scalar union must contain two distinct types");
    };
    let project = Project {
        source: SchemaNode::group("source", Vec::new()),
        target: SchemaNode::scalar_union("target", types),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(1, Node::Const { value })].into(),
        },
        root: Scope {
            construction: ScopeConstruction::Scalar { value: 1 },
            ..Scope::default()
        },
    };
    run(&project, &Instance::Group(Vec::new()))?
        .as_scalar()
        .cloned()
        .ok_or_else(|| EngineError::MissingSourceField("target".into()))
}

#[test]
fn admitted_union_tags_are_preserved() -> Result<(), EngineError> {
    assert_eq!(
        execute(
            [ScalarType::String, ScalarType::Int],
            Value::String("42".into())
        )?,
        Value::String("42".into())
    );
    assert_eq!(
        execute([ScalarType::String, ScalarType::Int], Value::Int(42))?,
        Value::Int(42)
    );
    Ok(())
}

#[test]
fn union_target_widens_exact_integer_only_when_float_is_required() -> Result<(), EngineError> {
    assert_eq!(
        execute([ScalarType::String, ScalarType::Float], Value::Int(42))?,
        Value::Float(42.0)
    );
    assert_eq!(
        execute_scalar_scope([ScalarType::String, ScalarType::Float], Value::Int(42))?,
        Value::Float(42.0)
    );
    let outside_exact_range = (1_i64 << f64::MANTISSA_DIGITS) + 1;
    assert_eq!(
        execute(
            [ScalarType::Float, ScalarType::Bool],
            Value::Int(outside_exact_range)
        )?,
        Value::Int(outside_exact_range)
    );
    Ok(())
}

#[test]
fn union_target_does_not_invent_string_or_boolean_coercions() -> Result<(), EngineError> {
    assert_eq!(
        execute([ScalarType::String, ScalarType::Bool], Value::Int(7))?,
        Value::Int(7)
    );
    assert_eq!(
        execute(
            [ScalarType::Int, ScalarType::Bool],
            Value::String("7".into())
        )?,
        Value::String("7".into())
    );
    Ok(())
}
