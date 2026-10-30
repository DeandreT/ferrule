use super::*;
use std::path::Path;

fn root(tag: &str) -> Instance {
    Instance::Group(
        vec![
            ("Tag".into(), Instance::Scalar(Value::String(tag.into()))),
            (
                "Rows".into(),
                Instance::Repeated(vec![Instance::Group(vec![].into())]),
            ),
        ]
        .into(),
    )
}

#[test]
fn new_primary_keeps_counter_state_without_reusing_old_frames() {
    let a = root("A");
    let b = root("B");
    let execution = ExecutionContext::new(Path::new("original.mfd"))
        .with_invocation_counter_limits(crate::InvocationCounterLimits {
            max_counters: 2,
            max_text_bytes: 128,
        });
    let clock = || Ok("2030-01-02T03:04:05".to_string());
    let first =
        ScopeContext::with_execution_context(&a, &execution).with_document_body_clock(Some(&clock));
    assert_eq!(
        first
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(1)
    );
    let second = first.next_primary_source(&b).unwrap();
    assert_eq!(
        second
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(2)
    );
    assert_eq!(
        first
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        first.source_root_field(2, &["Tag"]).unwrap(),
        Value::String("A".into())
    );
    assert_eq!(
        second.source_root_field(2, &["Tag"]).unwrap(),
        Value::String("B".into())
    );
    assert_eq!(
        second.runtime_value(RuntimeValue::CurrentDateTime).unwrap(),
        Value::String("2030-01-02T03:04:05".into())
    );
    assert!(
        first.walk_source(&["Rows"])[0]
            .next_primary_source(&b)
            .is_none()
    );
    let inputs = [NamedInput {
        name: "Other",
        instance: &b,
    }];
    assert!(
        ScopeContext::with_named_inputs(&a, &inputs)
            .next_primary_source(&b)
            .is_none()
    );
}

#[test]
fn explicit_primary_transition_retains_the_same_named_inputs() {
    let a = root("A");
    let b = root("B");
    let parameter = root("parameter");
    let inputs = [NamedInput {
        name: "Parameters",
        instance: &parameter,
    }];
    let execution = ExecutionContext::new(Path::new("original.mfd"))
        .with_invocation_counter_limits(crate::InvocationCounterLimits {
            max_counters: 2,
            max_text_bytes: 128,
        });
    let clock = || Ok("2030-01-02T03:04:05".to_string());
    let first = ScopeContext::with_named_inputs_and_execution_context(&a, &inputs, &execution)
        .with_document_body_clock(Some(&clock));
    assert!(first.next_primary_source(&b).is_none());
    assert_eq!(
        first
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(1)
    );
    let second = first.next_primary_source_with_same_inputs(&b).unwrap();
    assert_eq!(
        second
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(2)
    );
    assert_eq!(
        second.resolve_source_field(&["Parameters", "Tag"]).unwrap(),
        Value::String("parameter".into())
    );
    assert_eq!(
        second.source_root_field(2, &["Tag"]).unwrap(),
        Value::String("B".into())
    );
    assert_eq!(
        first.source_root_field(2, &["Tag"]).unwrap(),
        Value::String("A".into())
    );
    assert_eq!(
        second.runtime_value(RuntimeValue::CurrentDateTime).unwrap(),
        Value::String("2030-01-02T03:04:05".into())
    );
    let third = second.next_primary_source_with_same_inputs(&a).unwrap();
    assert_eq!(
        third
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        third.resolve_source_field(&["Parameters", "Tag"]).unwrap(),
        Value::String("parameter".into())
    );
    assert!(
        first.walk_source(&["Rows"])[0]
            .next_primary_source_with_same_inputs(&b)
            .is_none()
    );
    assert!(
        ScopeContext::with_basic_named_inputs(&a, &inputs)
            .next_primary_source_with_same_inputs(&b)
            .is_none()
    );
    let fresh = ScopeContext::with_named_inputs_and_execution_context(&a, &inputs, &execution);
    assert_eq!(
        fresh
            .next_invocation_counter(1, "lines", 1, 1, None)
            .unwrap(),
        Value::Int(1)
    );
}
