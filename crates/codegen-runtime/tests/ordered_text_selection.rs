use codegen_runtime::{AggregateFunction, FunctionError, RuntimeError, Value, aggregate};

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn ordered_text_selection_preserves_presence_and_first_seen_identity() {
    let cases = [
        (vec![], Value::Null, Value::Null),
        (vec![Value::Null], Value::Null, Value::Null),
        (vec![text("A"), text("B"), text("A")], text("A"), text("B")),
        (
            vec![text("A"), Value::Null, text("B"), Value::Null],
            text("B"),
            text("B"),
        ),
        (vec![text("A"), text(""), text("A")], text("A"), text("")),
        (
            vec![text(" X "), text("X"), text(" X ")],
            text(" X "),
            text("X"),
        ),
        (
            vec![text("é"), text("e\u{301}"), text("é")],
            text("é"),
            text("e\u{301}"),
        ),
        (
            vec![text("A"), text("B"), text("A"), text("C"), text("B")],
            text("B"),
            text("C"),
        ),
    ];
    for (values, last, distinct) in cases {
        assert_eq!(
            aggregate(AggregateFunction::LastPresentText, &values, None).unwrap(),
            last
        );
        assert_eq!(
            aggregate(AggregateFunction::LastDistinctText, &values, None).unwrap(),
            distinct
        );
    }
}

#[test]
fn ordered_text_selection_refuses_non_text_at_every_position_and_has_no_retry_state() {
    for function in [
        AggregateFunction::LastPresentText,
        AggregateFunction::LastDistinctText,
    ] {
        for invalid in [
            Value::json_null(),
            Value::xml_nil(),
            Value::Int(1),
            Value::Float(0.25),
            Value::Bool(true),
            Value::decimal(ir::Decimal96::ZERO),
        ] {
            let expected_kind = invalid.type_name();
            for position in 0..3 {
                let mut values = vec![text("A"), text("B"), text("A")];
                values[position] = invalid.clone();
                assert!(
                    matches!(aggregate(function, &values, None), Err(RuntimeError::AggregateUnsupportedScalar { function: actual, found }) if actual == function && found == expected_kind)
                );
                let expected = if function == AggregateFunction::LastPresentText {
                    text("A")
                } else {
                    text("B")
                };
                assert_eq!(
                    aggregate(function, &[text("A"), text("B"), text("A")], None).unwrap(),
                    expected
                );
            }
        }
        let expected_function = if function == AggregateFunction::LastPresentText {
            "last_present_text"
        } else {
            "last_distinct_text"
        };
        assert!(matches!(
            aggregate(function, &[text("A")], Some(Value::Null)),
            Err(RuntimeError::Function(
                FunctionError::InvalidArgument { function, .. }
            )) if function == expected_function
        ));
    }
}
