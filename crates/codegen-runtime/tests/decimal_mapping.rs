use codegen_runtime::{AggregateFunction, RuntimeError, Value, aggregate, collect_aggregate_value};
use ir::{Decimal96, DecimalError};

fn convert(text: &str) -> Value {
    functions::call("decimal_from_nullable_text", &[Value::String(text.into())]).unwrap()
}

#[test]
fn decimal_sum_is_ordered_exact_and_rounds_operands_before_reducing() {
    for (inputs, expected) in [
        (vec!["0.1", "0.2"], "0.3"),
        (vec!["9007199254740993", "1"], "9007199254740994"),
        (
            vec!["0.1234567890123456789012345678"],
            "0.1234567890123456789012345678",
        ),
        (
            vec![
                "0.00000000000000000000000000005",
                "0.00000000000000000000000000005",
            ],
            "0",
        ),
        (vec!["-0.1", "0.1"], "0"),
        (vec![], "0"),
    ] {
        let values: Vec<_> = inputs.iter().map(|text| convert(text)).collect();
        assert_eq!(
            aggregate(AggregateFunction::DecimalSum, &values, None),
            Ok(Value::decimal(Decimal96::parse_exact(expected).unwrap()))
        );
    }
    let values = [
        convert("79228162514264337593543950335"),
        convert("1"),
        convert("-1"),
    ];
    assert_eq!(
        aggregate(AggregateFunction::DecimalSum, &values, None),
        Err(RuntimeError::AggregateDecimal {
            function: AggregateFunction::DecimalSum,
            source: DecimalError::Overflow
        })
    );
}

#[test]
fn decimal_sum_refuses_mixed_kinds_at_every_position_and_remains_reusable() {
    for invalid in [
        Value::Null,
        Value::json_null(),
        Value::xml_nil(),
        Value::Int(1),
        Value::Float(0.1),
        Value::Bool(false),
        Value::String("0.1".into()),
    ] {
        for index in 0..3 {
            let mut values = vec![convert("0.1"), convert("0.2"), convert("0.3")];
            values[index] = invalid.clone();
            assert_eq!(
                aggregate(AggregateFunction::DecimalSum, &values, None),
                Err(RuntimeError::AggregateUnsupportedScalar {
                    function: AggregateFunction::DecimalSum,
                    found: invalid.type_name()
                })
            );
            assert_eq!(
                aggregate(
                    AggregateFunction::DecimalSum,
                    &[convert("0.1"), convert("0.2")],
                    None
                ),
                Ok(convert("0.3"))
            );
        }
    }
    assert!(matches!(
        aggregate(AggregateFunction::DecimalSum, &[], Some(Value::Int(1))),
        Err(RuntimeError::Function(_))
    ));
}

#[test]
fn decimal_collection_folds_immediately_and_preserves_state_on_failure() {
    let mut values = Vec::new();
    collect_aggregate_value(
        AggregateFunction::DecimalSum,
        &mut values,
        convert("79228162514264337593543950335"),
    )
    .unwrap();
    let previous = values.clone();
    assert!(matches!(
        collect_aggregate_value(AggregateFunction::DecimalSum, &mut values, convert("1")),
        Err(RuntimeError::AggregateDecimal {
            source: DecimalError::Overflow,
            ..
        })
    ));
    assert_eq!(values, previous);
    collect_aggregate_value(AggregateFunction::DecimalSum, &mut values, convert("-1")).unwrap();
    assert_eq!(
        aggregate(AggregateFunction::DecimalSum, &values, None),
        Ok(convert("79228162514264337593543950334"))
    );
    assert_eq!(values.len(), 1);
}
