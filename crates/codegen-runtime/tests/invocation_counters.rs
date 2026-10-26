use codegen_runtime::{InvocationCounterError, InvocationCounterLimits, InvocationCounters};

#[test]
fn invocation_scopes_share_state_but_reused_host_settings_do_not() {
    use codegen_runtime::{
        ExecutionContext, GroupedItems, Instance, ScopeContext, Value, field, group, repeated,
    };
    use std::path::Path;
    let source = group([field("Rows", repeated([group([]), group([])]))]);
    let execution = ExecutionContext::new(Path::new("mapping.ferrule"))
        .with_invocation_counter_limits(InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 4,
        });
    let root = ScopeContext::with_execution_context(&source, &execution);
    assert_eq!(
        root.next_invocation_counter(1, "line", 1, 1, None),
        Ok(Value::Int(1))
    );
    let children = root.walk_source(&["Rows"]);
    assert_eq!(children.len(), 2);
    assert_eq!(
        children[0].next_invocation_counter(1, "line", 999, 1, None),
        Ok(Value::Int(2))
    );
    assert_eq!(
        children[1].next_invocation_counter(1, "line", 999, 1, None),
        Ok(Value::Int(3))
    );
    let grouped = GroupedItems::by(
        children
            .into_iter()
            .enumerate()
            .map(|(index, child)| (child, Value::Int(index as i64)))
            .collect(),
        None,
    );
    let values: Vec<_> = grouped
        .contexts()
        .iter()
        .map(|context| {
            context
                .next_invocation_counter(1, "line", 999, 1, None)
                .unwrap()
        })
        .collect();
    assert_eq!(values, [Value::Int(4), Value::Int(5)]);
    let fresh = ScopeContext::with_execution_context(&source, &execution);
    assert_eq!(
        fresh.next_invocation_counter(1, "line", 1, 1, None),
        Ok(Value::Int(1))
    );
    assert_eq!(
        root.next_invocation_counter(1, "line", 999, 1, None),
        Ok(Value::Int(6))
    );
    let empty = Instance::Group(Vec::new().into());
    assert!(
        ScopeContext::new(&empty)
            .require_invocation_counters(7)
            .is_err()
    );
}

#[test]
fn invocation_operand_helpers_keep_strict_types_and_restart_presence() {
    use codegen_runtime::{
        RuntimeError, Value, invocation_counter_integer, invocation_counter_restart,
    };
    assert_eq!(invocation_counter_integer(9, "start", Value::Int(3)), Ok(3));
    for value in [
        Value::Null,
        Value::String("3".into()),
        Value::Float(3.0),
        Value::decimal(ir::Decimal96::parse_exact("3").unwrap()),
        Value::Bool(true),
        Value::json_null(),
        Value::xml_nil(),
    ] {
        let found = value.type_name();
        assert_eq!(
            invocation_counter_integer(9, "start", value),
            Err(RuntimeError::InvocationCounterOperand {
                node: 9,
                operand: "start",
                expected: "integer",
                found
            })
        );
    }
    assert_eq!(invocation_counter_restart(9, Value::Null), Ok(None));
    assert_eq!(
        invocation_counter_restart(9, Value::String("".into())),
        Ok(Some("".into()))
    );
    assert!(invocation_counter_restart(9, Value::json_null()).is_err());
    assert!(invocation_counter_restart(9, Value::xml_nil()).is_err());
    assert!(
        invocation_counter_restart(9, Value::decimal(ir::Decimal96::parse_exact("3").unwrap()))
            .is_err()
    );
}

#[test]
fn generated_consumer_can_own_counter_state_without_runtime_parameters() {
    let mut counters = InvocationCounters::new(InvocationCounterLimits {
        max_counters: 1,
        max_text_bytes: 4,
    });
    assert_eq!(counters.next("line", 1, 1, None), Ok(1));
    assert_eq!(counters.next("line", 999, 2, None), Ok(3));
    assert_eq!(
        counters.next("other", 1, 1, None),
        Err(InvocationCounterError::CountLimit { maximum: 1 })
    );
    assert_eq!(counters.next("line", 999, 2, None), Ok(5));
    assert!(counters.reset("line"));
    assert_eq!(counters.next("line", 10, 1, None), Ok(10));
}
