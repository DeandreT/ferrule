use codegen_runtime::{AggregateFunction, Value, aggregate};

#[test]
fn first_nonempty_scans_and_checks_the_complete_sequence() {
    let operation = AggregateFunction::FirstNonemptyText;
    assert_eq!(aggregate(operation, &[], None).unwrap(), Value::Null);
    assert_eq!(
        aggregate(
            operation,
            &[Value::Null, Value::String(String::new())],
            None
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        aggregate(
            operation,
            &[
                Value::String(String::new()),
                Value::String(" ".into()),
                Value::String("later".into())
            ],
            None
        )
        .unwrap(),
        Value::String(" ".into())
    );
    assert!(
        aggregate(
            operation,
            &[Value::String("first".into()), Value::Int(2)],
            None
        )
        .is_err()
    );
    assert!(
        aggregate(
            operation,
            &[Value::String("first".into())],
            Some(Value::Int(1))
        )
        .is_err()
    );
}
