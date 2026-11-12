use super::{Collector, field_trace_project};
use crate::{ExecutionContext, TraceEvent, run_with_context};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DynamicBinding, DynamicChild, Graph, Node, Scope};
use std::error::Error;
use std::path::Path;

#[test]
fn target_field_trace_preserves_depth_first_write_order_and_scalar_states()
-> Result<(), Box<dyn Error>> {
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Missing", ScalarType::String),
            SchemaNode::scalar("Nil", ScalarType::String).nillable(),
            SchemaNode::group(
                "Child",
                vec![SchemaNode::scalar("Leaf", ScalarType::String)],
            ),
        ],
    );
    let project = field_trace_project(
        target,
        Graph {
            nodes: [
                (0, Node::Const { value: Value::Null }),
                (
                    1,
                    Node::Const {
                        value: Value::xml_nil(),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("leaf".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            bindings: vec![
                Binding {
                    target_field: "Missing".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Nil".into(),
                    node: 1,
                },
            ],
            children: vec![Scope {
                target_field: "Child".into(),
                bindings: vec![Binding {
                    target_field: "Leaf".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)?;

    let writes = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten {
                scope,
                field,
                binding,
                kind,
                value,
                ..
            } => Some((scope.target_path, field, binding, kind, value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 4);
    assert_eq!(
        writes
            .iter()
            .map(|(_, field, _, _, _)| field.as_str())
            .collect::<Vec<_>>(),
        ["Missing", "Nil", "Leaf", "Child"]
    );
    assert_eq!(
        writes[0].4.as_ref().map(|value| value.value_type),
        Some("null")
    );
    assert_eq!(
        writes[1].4.as_ref().map(|value| value.value_type),
        Some("xml nil")
    );
    assert_eq!(writes[2].0, ["Child"]);
    assert_eq!(
        writes[2].2,
        crate::TraceTargetFieldBinding::StaticBinding { value: 2 }
    );
    assert_eq!(writes[3].2, crate::TraceTargetFieldBinding::StaticChild);
    assert_eq!(writes[3].3, crate::TraceOutputKind::Group);
    assert!(writes[3].4.is_none());
    Ok(())
}

#[test]
fn target_field_trace_bounds_dynamic_keys_and_omits_rejected_writes() -> Result<(), Box<dyn Error>>
{
    let dynamic_field = SchemaNode::scalar("*", ScalarType::String);
    let target = SchemaNode::group("Output", Vec::new())
        .with_dynamic_fields(dynamic_field)
        .ok_or_else(|| std::io::Error::other("dynamic scalar target should be valid"))?;
    let key = "é".repeat(200);
    let project = field_trace_project(
        target,
        Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String(key),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("first".into()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("rejected".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            dynamic_bindings: vec![
                DynamicBinding { key: 0, value: 1 },
                DynamicBinding { key: 0, value: 2 },
            ],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    let error = run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)
        .expect_err("the duplicate dynamic field must fail");
    assert!(matches!(
        error,
        crate::EngineError::DuplicateDynamicProperty(_)
    ));

    let writes = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten {
                field,
                binding,
                value,
                ..
            } => Some((field, binding, value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0.chars().count(), 160);
    assert_eq!(
        writes[0].1,
        crate::TraceTargetFieldBinding::DynamicBinding { key: 0, value: 1 }
    );
    assert_eq!(
        writes[0]
            .2
            .as_ref()
            .map(|value| (value.value_type, value.preview.as_str())),
        Some(("string", "first"))
    );
    Ok(())
}

#[test]
fn target_field_trace_records_dynamic_children_after_their_content() -> Result<(), Box<dyn Error>> {
    let dynamic_group =
        SchemaNode::group("*", vec![SchemaNode::scalar("Leaf", ScalarType::String)]);
    let target = SchemaNode::group("Output", Vec::new())
        .with_dynamic_fields(dynamic_group)
        .ok_or_else(|| std::io::Error::other("dynamic group target should be valid"))?;
    let project = field_trace_project(
        target,
        Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("ComputedChild".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("value".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            dynamic_children: vec![DynamicChild {
                key: 0,
                scope: Scope {
                    bindings: vec![Binding {
                        target_field: "Leaf".into(),
                        node: 1,
                    }],
                    ..Scope::default()
                },
            }],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)?;

    let writes = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten {
                scope,
                field,
                binding,
                ..
            } => Some((scope.target_path, field, binding)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        writes,
        [
            (
                vec!["<dynamic>".into()],
                "Leaf".into(),
                crate::TraceTargetFieldBinding::StaticBinding { value: 1 },
            ),
            (
                Vec::new(),
                "ComputedChild".into(),
                crate::TraceTargetFieldBinding::DynamicChild { key: 0 },
            ),
        ]
    );
    Ok(())
}
