use crate::{RuntimeError, Value, iteration::compare_int_float};

/// A scalar reduction over one source collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Join,
    ItemAt,
    DecimalSum,
    LastPresentText,
    LastDistinctText,
    FirstNonemptyText,
}

/// Append an ordinary aggregate operand, or advance a decimal fold immediately.
/// Decimal projection failures and arithmetic failures therefore keep input
/// order. A failed append leaves the previously collected values unchanged.
pub fn collect_aggregate_value(
    function: AggregateFunction,
    values: &mut Vec<Value>,
    value: Value,
) -> Result<(), RuntimeError> {
    if function == AggregateFunction::DecimalSum {
        let total = aggregate(function, values, None)?;
        let total = aggregate(function, &[total, value], None)?;
        values.clear();
        values.push(total);
    } else {
        values.push(value);
    }
    Ok(())
}

/// Applies one aggregate after generated code has evaluated every item value.
pub fn aggregate(
    function: AggregateFunction,
    values: &[Value],
    arg: Option<Value>,
) -> Result<Value, RuntimeError> {
    match function {
        AggregateFunction::LastPresentText
        | AggregateFunction::LastDistinctText
        | AggregateFunction::FirstNonemptyText => {
            if arg.is_some() {
                return Err(crate::FunctionError::InvalidArgument {
                    function: if function == AggregateFunction::FirstNonemptyText {
                        "first_nonempty_text"
                    } else if function == AggregateFunction::LastPresentText {
                        "last_present_text"
                    } else {
                        "last_distinct_text"
                    },
                    message: "does not accept an argument",
                }
                .into());
            }
            let distinct = function == AggregateFunction::LastDistinctText;
            let mut seen = std::collections::HashSet::new();
            let mut selected = None;
            for value in values {
                if matches!(value, Value::Null) {
                    continue;
                }
                let Value::String(text) = value else {
                    return Err(RuntimeError::AggregateUnsupportedScalar {
                        function,
                        found: value.type_name(),
                    });
                };
                if function == AggregateFunction::FirstNonemptyText {
                    if selected.is_none() && !text.is_empty() {
                        selected = Some(text.clone());
                    }
                } else if !distinct || seen.insert(text.as_str()) {
                    selected = Some(text.clone());
                }
            }
            Ok(selected.map_or(Value::Null, Value::String))
        }
        AggregateFunction::DecimalSum => {
            if arg.is_some() {
                return Err(crate::FunctionError::InvalidArgument {
                    function: "decimal_sum",
                    message: "does not accept an argument",
                }
                .into());
            }
            let mut total = ir::Decimal96::ZERO;
            for value in values {
                let Value::Decimal(decimal) = value else {
                    return Err(RuntimeError::AggregateUnsupportedScalar {
                        function,
                        found: value.type_name(),
                    });
                };
                total = total
                    .add_rounded(**decimal)
                    .map_err(|source| RuntimeError::AggregateDecimal { function, source })?;
            }
            Ok(Value::decimal(total))
        }
        AggregateFunction::Count => Ok(Value::Int(values.len() as i64)),
        AggregateFunction::Sum => {
            let numbers = numeric_values(function, values)?;
            if numbers.iter().all(|number| number.is_int()) {
                numbers
                    .iter()
                    .try_fold(0_i64, |sum, number| {
                        let NumericValue::Int(value) = number else {
                            return Ok(sum);
                        };
                        sum.checked_add(*value)
                            .ok_or(RuntimeError::AggregateIntegerOverflow { function })
                    })
                    .map(Value::Int)
            } else {
                finite_float(function, compensated_sum(&numbers)?).map(Value::Float)
            }
        }
        AggregateFunction::Avg => {
            let numbers = numeric_values(function, values)?;
            if numbers.is_empty() {
                return Ok(Value::Null);
            }
            finite_float(function, compensated_average(&numbers)?).map(Value::Float)
        }
        AggregateFunction::Min | AggregateFunction::Max => {
            let numbers = numeric_values(function, values)?;
            let want = if function == AggregateFunction::Min {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
            let mut best = None;
            for value in numbers {
                match best {
                    None => best = Some(value),
                    Some(current) if value.compare(current) == want => best = Some(value),
                    Some(_) => {}
                }
            }
            Ok(best.map_or(Value::Null, NumericValue::into_value))
        }
        AggregateFunction::Join => {
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
        AggregateFunction::ItemAt => {
            if matches!(arg, Some(Value::Decimal(_))) {
                return Err(RuntimeError::AggregateUnsupportedScalar {
                    function,
                    found: "decimal",
                });
            }
            let index = arg.as_ref().and_then(|value| match value {
                Value::Int(value) => Some(*value),
                Value::Float(value) => Some(value.round() as i64),
                Value::String(value) => value.trim().parse().ok(),
                Value::Null
                | Value::JsonNull(_)
                | Value::XmlNil(_)
                | Value::Bool(_)
                | Value::Decimal(_) => None,
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
    const fn is_int(self) -> bool {
        matches!(self, Self::Int(_))
    }

    fn as_float(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    const fn into_value(self) -> Value {
        match self {
            Self::Int(value) => Value::Int(value),
            Self::Float(value) => Value::Float(value),
        }
    }

    fn compare(self, other: Self) -> std::cmp::Ordering {
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

fn numeric_value(value: &Value) -> Result<Option<NumericValue>, ()> {
    match value {
        Value::Int(value) => Ok(Some(NumericValue::Int(*value))),
        Value::Float(value) if value.is_finite() => Ok(Some(NumericValue::Float(*value))),
        Value::Float(_) | Value::Decimal(_) => Err(()),
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
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) | Value::Bool(_) => Ok(None),
    }
}

fn numeric_values(
    function: AggregateFunction,
    values: &[Value],
) -> Result<Vec<NumericValue>, RuntimeError> {
    let mut numbers = Vec::new();
    for value in values {
        if matches!(value, Value::Decimal(_)) {
            return Err(RuntimeError::AggregateUnsupportedScalar {
                function,
                found: "decimal",
            });
        }
        if let Some(number) =
            numeric_value(value).map_err(|()| RuntimeError::AggregateNonFinite { function })?
        {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

fn finite_float(function: AggregateFunction, value: f64) -> Result<f64, RuntimeError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(RuntimeError::AggregateNonFinite { function })
    }
}

fn compensated_sum(values: &[NumericValue]) -> Result<f64, RuntimeError> {
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
        let next = finite_float(AggregateFunction::Sum, sum + value)?;
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction = finite_float(AggregateFunction::Sum, correction)?;
        sum = next;
    }
    let normalized = finite_float(AggregateFunction::Sum, sum + correction)?;
    finite_float(AggregateFunction::Sum, normalized * scale)
}

fn compensated_average(values: &[NumericValue]) -> Result<f64, RuntimeError> {
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
        let mean = (sum + correction) / values.len() as f64;
        if mean.is_finite() {
            return Ok(mean);
        }
    }

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
        let next = finite_float(AggregateFunction::Avg, sum + value)?;
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction = finite_float(AggregateFunction::Avg, correction)?;
        sum = next;
    }
    let normalized = finite_float(AggregateFunction::Avg, sum + correction)?;
    let mean = finite_float(AggregateFunction::Avg, normalized / values.len() as f64)?;
    finite_float(AggregateFunction::Avg, mean * scale)
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
mod tests {
    use super::*;

    #[test]
    fn decimal_values_are_never_skipped_by_legacy_numeric_aggregates() {
        for function in [
            AggregateFunction::Sum,
            AggregateFunction::Avg,
            AggregateFunction::Min,
            AggregateFunction::Max,
        ] {
            for index in 0..3 {
                let mut values = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
                values[index] = Value::decimal(ir::Decimal96::parse_exact("0.1").unwrap());
                assert_eq!(
                    aggregate(function, &values, None),
                    Err(RuntimeError::AggregateUnsupportedScalar {
                        function,
                        found: "decimal"
                    })
                );
            }
        }
        let decimal = Value::decimal(ir::Decimal96::parse_exact("9007199254740993").unwrap());
        assert_eq!(
            aggregate(
                AggregateFunction::Count,
                std::slice::from_ref(&decimal),
                None
            ),
            Ok(Value::Int(1))
        );
        assert_eq!(
            aggregate(
                AggregateFunction::ItemAt,
                std::slice::from_ref(&decimal),
                Some(Value::Int(1))
            ),
            Ok(decimal.clone())
        );
        assert_eq!(
            aggregate(
                AggregateFunction::Join,
                std::slice::from_ref(&decimal),
                None
            ),
            Ok(Value::String("9007199254740993".into()))
        );
        assert_eq!(
            aggregate(AggregateFunction::ItemAt, &[Value::Int(1)], Some(decimal)),
            Err(RuntimeError::AggregateUnsupportedScalar {
                function: AggregateFunction::ItemAt,
                found: "decimal"
            })
        );
    }

    #[test]
    fn exact_integer_and_mixed_numeric_reductions_match_engine_contracts() {
        assert_eq!(
            aggregate(
                AggregateFunction::Sum,
                &[Value::Int(9_007_199_254_740_992), Value::Int(1)],
                None,
            ),
            Ok(Value::Int(9_007_199_254_740_993))
        );
        assert_eq!(
            aggregate(
                AggregateFunction::Sum,
                &[Value::Int(i64::MAX), Value::Int(1)],
                None,
            ),
            Err(RuntimeError::AggregateIntegerOverflow {
                function: AggregateFunction::Sum,
            })
        );
        let precise = [
            Value::Int(9_007_199_254_740_993),
            Value::Float(9_007_199_254_740_992.0),
        ];
        assert_eq!(
            aggregate(AggregateFunction::Min, &precise, None),
            Ok(Value::Float(9_007_199_254_740_992.0))
        );
        assert_eq!(
            aggregate(AggregateFunction::Max, &precise, None),
            Ok(Value::Int(9_007_199_254_740_993))
        );
    }

    #[test]
    fn finite_compensation_empty_values_and_text_operations_match_engine_contracts() {
        assert_eq!(
            aggregate(
                AggregateFunction::Avg,
                &[Value::Float(f64::MAX), Value::Float(f64::MAX)],
                None,
            ),
            Ok(Value::Float(f64::MAX))
        );
        assert_eq!(
            aggregate(AggregateFunction::Count, &[], None),
            Ok(Value::Int(0))
        );
        assert_eq!(
            aggregate(AggregateFunction::Sum, &[], None),
            Ok(Value::Int(0))
        );
        assert_eq!(
            aggregate(AggregateFunction::Avg, &[], None),
            Ok(Value::Null)
        );
        assert_eq!(
            aggregate(
                AggregateFunction::Join,
                &[
                    Value::String("a".into()),
                    Value::Null,
                    Value::xml_nil(),
                    Value::String("b".into()),
                ],
                Some(Value::String("|".into())),
            ),
            Ok(Value::String("a||b".into()))
        );
        assert_eq!(
            aggregate(
                AggregateFunction::ItemAt,
                &[Value::String("a".into()), Value::String("b".into())],
                Some(Value::Float(1.5)),
            ),
            Ok(Value::String("b".into()))
        );
    }

    #[test]
    fn non_finite_values_and_results_remain_typed() {
        for function in [
            AggregateFunction::Sum,
            AggregateFunction::Avg,
            AggregateFunction::Min,
            AggregateFunction::Max,
        ] {
            assert_eq!(
                aggregate(function, &[Value::String("inf".into())], None),
                Err(RuntimeError::AggregateNonFinite { function })
            );
        }
        assert_eq!(
            aggregate(
                AggregateFunction::Sum,
                &[Value::Float(f64::MAX), Value::Float(f64::MAX)],
                None,
            ),
            Err(RuntimeError::AggregateNonFinite {
                function: AggregateFunction::Sum,
            })
        );
    }

    #[test]
    fn equal_signed_zero_preserves_the_first_numeric_value() {
        for values in [
            [Value::Float(-0.0), Value::Float(0.0), Value::Int(0)],
            [Value::Float(0.0), Value::Float(-0.0), Value::Int(0)],
        ] {
            let Value::Float(first) = values[0] else {
                panic!("first value is a float");
            };
            for function in [AggregateFunction::Min, AggregateFunction::Max] {
                let Ok(Value::Float(value)) = aggregate(function, &values, None) else {
                    panic!("aggregate returns the first equal float");
                };
                assert_eq!(value.to_bits(), first.to_bits());
            }
        }
    }
}
