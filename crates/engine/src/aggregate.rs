use ir::Value;

use crate::EngineError;

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
                    .filter(|value| !matches!(value, Value::Null))
                    .map(value_text)
                    .collect::<Vec<_>>()
                    .join(&separator),
            ))
        }
        AggregateOp::ItemAt => {
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
    values
        .iter()
        .filter_map(|value| numeric_value(value).transpose())
        .collect::<Result<_, _>>()
        .map_err(|()| EngineError::AggregateNonFinite { function })
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

pub(crate) fn value_ordering(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Null, _) => Some(std::cmp::Ordering::Less),
        (_, Value::Null) => Some(std::cmp::Ordering::Greater),
        (Value::Int(left), Value::Int(right)) => Some(left.cmp(right)),
        (Value::Float(left), Value::Float(right)) => left.partial_cmp(right),
        (Value::Int(left), Value::Float(right)) if right.is_finite() => {
            Some(compare_int_float(*left, *right))
        }
        (Value::Float(left), Value::Int(right)) if left.is_finite() => {
            Some(compare_int_float(*right, *left).reverse())
        }
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}
