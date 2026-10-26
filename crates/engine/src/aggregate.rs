use ir::Value;

use crate::EngineError;

/// Decimal folds advance before another projected operand is evaluated. Other
/// reductions retain their existing eager value collection.
pub(crate) fn collect_value(
    function: mapping::AggregateOp,
    values: &mut Vec<Value>,
    value: Value,
) -> Result<(), EngineError> {
    if function == mapping::AggregateOp::DecimalSum {
        let total = aggregate(function, values.len(), values, None)?;
        let total = aggregate(function, 2, &[total, value], None)?;
        values.clear();
        values.push(total);
    } else {
        values.push(value);
    }
    Ok(())
}

/// Applies one aggregate over the per-item values of a collection.
/// `item_count` counts items, not non-null values.
pub(crate) fn aggregate(
    function: mapping::AggregateOp,
    item_count: usize,
    values: &[Value],
    arg: Option<Value>,
) -> Result<Value, EngineError> {
    use mapping::AggregateOp;
    match function {
        AggregateOp::LastPresentText | AggregateOp::LastDistinctText => {
            if arg.is_some() {
                return Err(functions::FunctionError::InvalidArgument {
                    function: if function == AggregateOp::LastPresentText {
                        "last_present_text"
                    } else {
                        "last_distinct_text"
                    },
                    message: "does not accept an argument",
                }
                .into());
            }
            let distinct = function == AggregateOp::LastDistinctText;
            let mut seen = std::collections::HashSet::new();
            let mut selected = None;
            for value in values {
                if matches!(value, Value::Null) {
                    continue;
                }
                let Value::String(text) = value else {
                    return Err(EngineError::AggregateUnsupportedScalar {
                        function,
                        found: value.type_name(),
                    });
                };
                if !distinct || seen.insert(text.as_str()) {
                    selected = Some(text.clone());
                }
            }
            Ok(selected.map_or(Value::Null, Value::String))
        }
        AggregateOp::DecimalSum => {
            if arg.is_some() {
                return Err(functions::FunctionError::InvalidArgument {
                    function: "decimal_sum",
                    message: "does not accept an argument",
                }
                .into());
            }
            let mut total = ir::Decimal96::ZERO;
            for value in values {
                let Value::Decimal(decimal) = value else {
                    return Err(EngineError::AggregateUnsupportedScalar {
                        function,
                        found: value.type_name(),
                    });
                };
                total = total
                    .add_rounded(**decimal)
                    .map_err(|source| EngineError::AggregateDecimal { function, source })?;
            }
            Ok(Value::decimal(total))
        }
        AggregateOp::Count => Ok(Value::Int(item_count as i64)),
        AggregateOp::Sum => {
            let numbers = numeric_values(function, values)?;
            if numbers.iter().all(|number| number.is_int()) {
                numbers
                    .iter()
                    .try_fold(0_i64, |sum, number| {
                        let NumericValue::Int(value) = number else {
                            return Ok(sum);
                        };
                        sum.checked_add(*value)
                            .ok_or(EngineError::AggregateIntegerOverflow { function })
                    })
                    .map(Value::Int)
            } else {
                finite_float(function, compensated_sum(&numbers)?).map(Value::Float)
            }
        }
        AggregateOp::Avg => {
            let numbers = numeric_values(function, values)?;
            if numbers.is_empty() {
                return Ok(Value::Null);
            }
            finite_float(function, compensated_average(&numbers)?).map(Value::Float)
        }
        AggregateOp::Min | AggregateOp::Max => {
            let numbers = numeric_values(function, values)?;
            let want = if function == AggregateOp::Min {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
            let mut best: Option<NumericValue> = None;
            for value in numbers {
                match best {
                    None => best = Some(value),
                    Some(current) => {
                        if value.cmp(current) == want {
                            best = Some(value);
                        }
                    }
                }
            }
            Ok(best.map_or(Value::Null, NumericValue::into_value))
        }
        AggregateOp::Join => {
            let separator = arg.map(|value| value_text(&value)).unwrap_or_default();
            Ok(Value::String(
                values
                    .iter()
                    .filter(|value| !matches!(value, Value::Null | Value::JsonNull(_)))
                    .map(value_text)
                    .collect::<Vec<_>>()
                    .join(&separator),
            ))
        }
        AggregateOp::ItemAt => {
            if matches!(arg, Some(Value::Decimal(_))) {
                return Err(EngineError::AggregateUnsupportedScalar {
                    function,
                    found: "decimal",
                });
            }
            // 1-based, XPath style; anything out of range is Null.
            let index = arg.as_ref().and_then(|value| match value {
                Value::Int(value) => Some(*value),
                Value::Float(value) => Some(value.round() as i64),
                Value::String(value) => value.trim().parse().ok(),
                _ => None,
            });
            Ok(match index {
                Some(index) if index >= 1 => values
                    .get(index as usize - 1)
                    .cloned()
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            })
        }
    }
}

#[derive(Clone, Copy)]
enum NumericValue {
    Int(i64),
    Float(f64),
}

impl NumericValue {
    fn is_int(self) -> bool {
        matches!(self, Self::Int(_))
    }

    fn as_float(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Int(value) => Value::Int(value),
            Self::Float(value) => Value::Float(value),
        }
    }

    fn cmp(self, other: Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Int(left), Self::Int(right)) => left.cmp(&right),
            (Self::Float(left), Self::Float(right)) => left
                .partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Self::Int(left), Self::Float(right)) => compare_int_float(left, right),
            (Self::Float(left), Self::Int(right)) => compare_int_float(right, left).reverse(),
        }
    }
}

/// Parses numeric values without routing integers through `f64`. Strings from
/// untyped sources parse; everything else is omitted from numeric reductions.
fn numeric_value(value: &Value) -> Result<Option<NumericValue>, ()> {
    match value {
        Value::Int(value) => Ok(Some(NumericValue::Int(*value))),
        Value::Float(value) if value.is_finite() => Ok(Some(NumericValue::Float(*value))),
        Value::Float(_) => Err(()),
        Value::String(value) => {
            let value = value.trim();
            if let Ok(value) = value.parse::<i64>() {
                return Ok(Some(NumericValue::Int(value)));
            }
            match value.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(Some(NumericValue::Float(value))),
                Ok(_) => Err(()),
                Err(_) => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

fn numeric_values(
    function: mapping::AggregateOp,
    values: &[Value],
) -> Result<Vec<NumericValue>, EngineError> {
    let mut numbers = Vec::new();
    for value in values {
        if matches!(value, Value::Decimal(_)) {
            return Err(EngineError::AggregateUnsupportedScalar {
                function,
                found: "decimal",
            });
        }
        if let Some(number) =
            numeric_value(value).map_err(|()| EngineError::AggregateNonFinite { function })?
        {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

fn finite_float(function: mapping::AggregateOp, value: f64) -> Result<f64, EngineError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(EngineError::AggregateNonFinite { function })
    }
}

fn compensated_sum(values: &[NumericValue]) -> Result<f64, EngineError> {
    let scale = values
        .iter()
        .map(|value| value.as_float().abs())
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Ok(0.0);
    }

    let mut sum = 0.0;
    let mut correction = 0.0;
    for value in values {
        let value = value.as_float() / scale;
        let next = finite_float(mapping::AggregateOp::Sum, sum + value)?;
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction = finite_float(mapping::AggregateOp::Sum, correction)?;
        sum = next;
    }
    let normalized = finite_float(mapping::AggregateOp::Sum, sum + correction)?;
    finite_float(mapping::AggregateOp::Sum, normalized * scale)
}

fn compensated_average(values: &[NumericValue]) -> Result<f64, EngineError> {
    let mut sum = 0.0;
    let mut correction = 0.0;
    let mut unscaled = true;
    for value in values {
        let value = value.as_float();
        let next = sum + value;
        if !next.is_finite() {
            unscaled = false;
            break;
        }
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        if !correction.is_finite() {
            unscaled = false;
            break;
        }
        sum = next;
    }
    if unscaled {
        let total = sum + correction;
        let mean = total / values.len() as f64;
        if mean.is_finite() {
            return Ok(mean);
        }
    }

    // Scaling keeps averages such as (MAX + MAX) / 2 finite when an
    // ordinary compensated sum would overflow before division.
    let scale = values
        .iter()
        .map(|value| value.as_float().abs())
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Ok(0.0);
    }

    let mut sum = 0.0;
    let mut correction = 0.0;
    for value in values {
        let value = value.as_float() / scale;
        let next = finite_float(mapping::AggregateOp::Avg, sum + value)?;
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction = finite_float(mapping::AggregateOp::Avg, correction)?;
        sum = next;
    }
    let normalized = finite_float(mapping::AggregateOp::Avg, sum + correction)?;
    let mean = finite_float(mapping::AggregateOp::Avg, normalized / values.len() as f64)?;
    finite_float(mapping::AggregateOp::Avg, mean * scale)
}

/// Compares an integer and a finite float without rounding the integer first.
fn compare_int_float(integer: i64, float: f64) -> std::cmp::Ordering {
    if float >= i64::MAX as f64 {
        return std::cmp::Ordering::Less;
    }
    if float < i64::MIN as f64 {
        return std::cmp::Ordering::Greater;
    }

    let truncated = float.trunc() as i64;
    match integer.cmp(&truncated) {
        std::cmp::Ordering::Equal if float.fract().is_sign_positive() && float.fract() != 0.0 => {
            std::cmp::Ordering::Less
        }
        std::cmp::Ordering::Equal if float.fract().is_sign_negative() && float.fract() != 0.0 => {
            std::cmp::Ordering::Greater
        }
        ordering => ordering,
    }
}

/// Total scalar order used by scope sorting.
///
/// Mapping keys are normally schema-homogeneous, but open or dynamically
/// computed data can mix scalar domains. Treating those pairs as equal while
/// ordering same-domain pairs violates transitivity, so sorting uses an
/// explicit domain order: missing, boolean, numeric, string, XML nil. NaN is
/// ordered after every other numeric value.
pub(crate) fn sort_value_ordering(left: &Value, right: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (left, right) {
        (Value::Null | Value::JsonNull(_), Value::Null | Value::JsonNull(_)) => Ordering::Equal,
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::Int(left), Value::Int(right)) => left.cmp(right),
        (Value::Float(left), Value::Float(right)) => match (left.is_nan(), right.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => left.partial_cmp(right).unwrap_or(Ordering::Equal),
        },
        (Value::Int(_), Value::Float(right)) if right.is_nan() => Ordering::Less,
        (Value::Float(left), Value::Int(_)) if left.is_nan() => Ordering::Greater,
        (Value::Int(left), Value::Float(right)) => compare_int_float(*left, *right),
        (Value::Float(left), Value::Int(right)) => compare_int_float(*right, *left).reverse(),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Decimal(left), Value::Decimal(right)) => left.cmp(right),
        (Value::XmlNil(_), Value::XmlNil(_)) => Ordering::Equal,
        _ => sort_value_rank(left).cmp(&sort_value_rank(right)),
    }
}

fn sort_value_rank(value: &Value) -> u8 {
    match value {
        Value::Null | Value::JsonNull(_) => 0,
        Value::Bool(_) => 1,
        Value::Int(_) | Value::Float(_) => 2,
        Value::String(_) => 3,
        Value::XmlNil(_) => 4,
        Value::Decimal(_) => 5,
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Decimal(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}

#[cfg(test)]
mod text_selection_tests {
    use super::*;
    use mapping::AggregateOp;

    fn text(value: &str) -> Value {
        Value::String(value.into())
    }

    #[test]
    fn ordered_text_selection_keeps_distinct_order_and_presence() {
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
        ];
        for (values, last, distinct) in cases {
            assert_eq!(
                aggregate(AggregateOp::LastPresentText, values.len(), &values, None).unwrap(),
                last
            );
            assert_eq!(
                aggregate(AggregateOp::LastDistinctText, values.len(), &values, None).unwrap(),
                distinct
            );
        }
    }

    #[test]
    fn ordered_text_selection_refuses_other_kinds_without_retaining_failed_state() {
        for function in [AggregateOp::LastPresentText, AggregateOp::LastDistinctText] {
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
                        matches!(aggregate(function, values.len(), &values, None), Err(EngineError::AggregateUnsupportedScalar { function: actual, found }) if actual == function && found == expected_kind)
                    );
                    let expected = if function == AggregateOp::LastPresentText {
                        text("A")
                    } else {
                        text("B")
                    };
                    assert_eq!(
                        aggregate(function, 3, &[text("A"), text("B"), text("A")], None).unwrap(),
                        expected
                    );
                }
            }
            let expected_function = if function == AggregateOp::LastPresentText {
                "last_present_text"
            } else {
                "last_distinct_text"
            };
            assert!(matches!(
                aggregate(function, 1, &[text("A")], Some(Value::Null)),
                Err(EngineError::Function(
                    functions::FunctionError::InvalidArgument { function, .. }
                )) if function == expected_function
            ));
        }
    }
}

#[cfg(test)]
mod decimal_tests {
    use super::*;

    #[test]
    fn decimal_numeric_aggregates_fail_without_dropping_any_position() {
        for function in [
            mapping::AggregateOp::Sum,
            mapping::AggregateOp::Avg,
            mapping::AggregateOp::Min,
            mapping::AggregateOp::Max,
        ] {
            for index in 0..3 {
                let mut values = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
                values[index] = Value::decimal(ir::Decimal96::parse_exact("0.1").unwrap());
                assert!(matches!(
                    aggregate(function, 3, &values, None),
                    Err(EngineError::AggregateUnsupportedScalar {
                        found: "decimal",
                        ..
                    })
                ));
            }
        }
        let decimal = Value::decimal(ir::Decimal96::parse_exact("9007199254740993").unwrap());
        assert_eq!(
            aggregate(
                mapping::AggregateOp::Count,
                1,
                std::slice::from_ref(&decimal),
                None
            )
            .unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            aggregate(
                mapping::AggregateOp::ItemAt,
                1,
                std::slice::from_ref(&decimal),
                Some(Value::Int(1))
            )
            .unwrap(),
            decimal
        );
        assert_eq!(
            aggregate(
                mapping::AggregateOp::Join,
                1,
                std::slice::from_ref(&decimal),
                None
            )
            .unwrap(),
            Value::String("9007199254740993".into())
        );
        assert!(matches!(
            aggregate(
                mapping::AggregateOp::ItemAt,
                1,
                &[Value::Int(1)],
                Some(decimal)
            ),
            Err(EngineError::AggregateUnsupportedScalar { .. })
        ));
    }

    #[test]
    fn decimal_sorting_keeps_a_separate_exact_domain() {
        let a = Value::decimal(ir::Decimal96::parse_exact("9007199254740992").unwrap());
        let b = Value::decimal(ir::Decimal96::parse_exact("9007199254740993").unwrap());
        assert_eq!(sort_value_ordering(&a, &b), std::cmp::Ordering::Less);
        assert_eq!(
            sort_value_ordering(&a, &Value::Int(9007199254740992)),
            std::cmp::Ordering::Greater
        );
        assert_eq!(sort_value_ordering(&a, &a), std::cmp::Ordering::Equal);
    }
}
