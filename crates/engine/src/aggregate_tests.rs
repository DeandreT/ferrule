use super::*;
use mapping::AggregateOp;

#[test]
fn integer_sum_is_exact_above_f64_integer_precision() {
    let result = aggregate(
        AggregateOp::Sum,
        2,
        &[Value::Int(9_007_199_254_740_992), Value::Int(1)],
        None,
    );

    assert_eq!(result, Ok(Value::Int(9_007_199_254_740_993)));
}

#[test]
fn integer_sum_reports_overflow() {
    let result = aggregate(
        AggregateOp::Sum,
        2,
        &[Value::Int(i64::MAX), Value::Int(1)],
        None,
    );

    assert_eq!(
        result,
        Err(EngineError::AggregateIntegerOverflow {
            function: AggregateOp::Sum,
        })
    );
}

#[test]
fn numeric_aggregates_reject_non_finite_values() {
    for function in [
        AggregateOp::Sum,
        AggregateOp::Avg,
        AggregateOp::Min,
        AggregateOp::Max,
    ] {
        assert_eq!(
            aggregate(function, 1, &[Value::String("NaN".into())], None),
            Err(EngineError::AggregateNonFinite { function })
        );
        assert_eq!(
            aggregate(function, 1, &[Value::Float(f64::INFINITY)], None),
            Err(EngineError::AggregateNonFinite { function })
        );
    }

    assert_eq!(
        aggregate(
            AggregateOp::Sum,
            2,
            &[Value::Float(f64::MAX), Value::Float(f64::MAX)],
            None,
        ),
        Err(EngineError::AggregateNonFinite {
            function: AggregateOp::Sum,
        })
    );
}

#[test]
fn average_of_large_finite_values_stays_finite() {
    assert_eq!(
        aggregate(
            AggregateOp::Avg,
            2,
            &[Value::Float(f64::MAX), Value::Float(f64::MAX)],
            None,
        ),
        Ok(Value::Float(f64::MAX))
    );
}

#[test]
fn average_uses_compensated_arithmetic() {
    let values = [13.6, 15.6, 16.2, 10.0, 7.3, 7.6, 13.6, 7.1].map(Value::Float);

    assert_eq!(
        aggregate(AggregateOp::Avg, values.len(), &values, None),
        Ok(Value::Float(11.375))
    );

    let values = [
        -3.2, -0.3, 6.5, 10.6, 19.0, 20.3, 22.3, 20.7, 19.2, 12.9, 8.1, 1.9,
    ]
    .map(Value::Float);
    assert_eq!(
        aggregate(AggregateOp::Avg, values.len(), &values, None),
        Ok(Value::Float(11.5))
    );
}

#[test]
fn mixed_sum_returns_a_finite_float() {
    assert_eq!(
        aggregate(
            AggregateOp::Sum,
            2,
            &[Value::Int(2), Value::Float(0.5)],
            None,
        ),
        Ok(Value::Float(2.5))
    );
}

#[test]
fn mixed_sum_does_not_overflow_before_finite_cancellation() {
    assert_eq!(
        aggregate(
            AggregateOp::Sum,
            3,
            &[
                Value::Float(f64::MAX),
                Value::Float(f64::MAX),
                Value::Float(-f64::MAX),
            ],
            None,
        ),
        Ok(Value::Float(f64::MAX))
    );
}

#[test]
fn min_and_max_compare_mixed_numbers_without_rounding_integers() {
    let values = [
        Value::Int(9_007_199_254_740_993),
        Value::Float(9_007_199_254_740_992.0),
    ];
    assert_eq!(
        aggregate(AggregateOp::Min, values.len(), &values, None),
        Ok(Value::Float(9_007_199_254_740_992.0))
    );
    assert_eq!(
        aggregate(AggregateOp::Max, values.len(), &values, None),
        Ok(Value::Int(9_007_199_254_740_993))
    );
}

#[test]
fn string_min_and_max_return_parsed_numbers() {
    let values = [Value::String("10".into()), Value::String("2.5".into())];
    assert_eq!(
        aggregate(AggregateOp::Min, values.len(), &values, None),
        Ok(Value::Float(2.5))
    );
    assert_eq!(
        aggregate(AggregateOp::Max, values.len(), &values, None),
        Ok(Value::Int(10))
    );
}

#[test]
fn min_and_max_treat_both_signed_zero_orders_as_equal() {
    for values in [
        [Value::Float(-0.0), Value::Float(0.0), Value::Int(0)],
        [Value::Float(0.0), Value::Float(-0.0), Value::Int(0)],
    ] {
        let expected_bits = match values[0] {
            Value::Float(value) => value.to_bits(),
            _ => unreachable!("the first regression value is a float"),
        };
        for function in [AggregateOp::Min, AggregateOp::Max] {
            let result = aggregate(function, values.len(), &values, None);
            let Ok(Value::Float(value)) = result else {
                panic!("{function:?} should preserve the first equal numeric value");
            };
            assert_eq!(value.to_bits(), expected_bits, "{function:?}");
        }
    }
}
