use ir::{
    FiniteF64, IntegerRange, NumberBound, NumberRange, NumericRange, ScalarType, SchemaKind,
    SchemaNode,
};

use crate::JsonFormatError;

const MAX_CONTIGUOUS_INTEGER_F64: f64 = 4_503_599_627_370_496.0;

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    type_was_absent: bool,
) -> Result<(), JsonFormatError> {
    if !has_range_keywords(schema) {
        return Ok(());
    }
    let lower = lower_bounds(name, schema)?;
    let upper = upper_bounds(name, schema)?;
    match node.kind {
        SchemaKind::Scalar {
            ty: ScalarType::Int,
        } => {
            let range = integer_range(name, &lower, &upper)?.map(NumericRange::Integer);
            node.numeric_range = intersect(name, node.numeric_range, range, ScalarType::Int)?;
        }
        SchemaKind::Scalar {
            ty: ScalarType::Float,
        } => {
            let range = number_range(name, &lower, &upper)?.map(NumericRange::Number);
            node.numeric_range = intersect(name, node.numeric_range, range, ScalarType::Float)?;
        }
        SchemaKind::Scalar {
            ty: ScalarType::String | ScalarType::Bool,
        }
        | SchemaKind::Group { .. }
            if !type_was_absent => {}
        SchemaKind::ScalarUnion { .. } => {
            return Err(unsupported(
                name,
                "numeric ranges on general scalar unions are not yet supported",
            ));
        }
        _ => {
            return Err(unsupported(
                name,
                "numeric range without a concrete numeric scalar type also admits unbounded non-numeric values",
            ));
        }
    }
    if !node.numeric_range_is_valid() {
        return Err(unsupported(
            name,
            "fixed numeric value falls outside the declared range",
        ));
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if !has_range_keywords(schema) {
        return Ok(());
    }
    lower_bounds(name, schema)?;
    upper_bounds(name, schema)?;
    Ok(())
}

pub(crate) fn validate_json(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(range) = schema.numeric_range else {
        return Ok(());
    };
    if value.is_null() && schema.nullable {
        return Ok(());
    }
    let matches = match (range, value) {
        (NumericRange::Integer(range), serde_json::Value::Number(value)) => {
            value.as_i64().is_some_and(|value| range.contains(value))
        }
        (NumericRange::Number(range), serde_json::Value::Number(value)) => {
            crate::exact_f64_from_json_number(value).is_some_and(|value| range.contains(value))
        }
        _ => false,
    };
    if matches {
        return Ok(());
    }
    Err(JsonFormatError::RangeMismatch {
        name: schema.name.clone(),
        range: display(range),
        got: value.to_string(),
    })
}

pub(crate) fn render(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let Some(range) = node.numeric_range else {
        return Ok(());
    };
    match range {
        NumericRange::Integer(range) => {
            if let Some(minimum) = range.minimum() {
                out.insert("minimum".into(), minimum.into());
            }
            if let Some(maximum) = range.maximum() {
                out.insert("maximum".into(), maximum.into());
            }
        }
        NumericRange::Number(range) => {
            render_number_bound(out, range.minimum(), "minimum", "exclusiveMinimum")?;
            render_number_bound(out, range.maximum(), "maximum", "exclusiveMaximum")?;
        }
    }
    Ok(())
}

pub(super) fn intersect(
    name: &str,
    left: Option<NumericRange>,
    right: Option<NumericRange>,
    target_type: ScalarType,
) -> Result<Option<NumericRange>, JsonFormatError> {
    let left = left
        .map(|range| convert(name, range, target_type))
        .transpose()?;
    let right = right
        .map(|range| convert(name, range, target_type))
        .transpose()?;
    match (left, right) {
        (None, range) | (range, None) => Ok(range),
        (Some(left), Some(right)) => left
            .intersection(right)
            .map(Some)
            .ok_or_else(|| unsupported(name, "allOf numeric ranges have an empty intersection")),
    }
}

pub(super) fn union(
    name: &str,
    ranges: impl IntoIterator<Item = Option<NumericRange>>,
) -> Result<Option<NumericRange>, JsonFormatError> {
    let ranges = ranges.into_iter().collect::<Vec<_>>();
    if ranges.iter().any(Option::is_none) {
        return Ok(None);
    }
    let ranges = ranges.into_iter().flatten().collect::<Vec<_>>();
    let Some(first) = ranges.first() else {
        return Ok(None);
    };
    match first {
        NumericRange::Integer(_) => union_integer_ranges(name, ranges),
        NumericRange::Number(_) => union_number_ranges(name, ranges),
    }
}

fn union_integer_ranges(
    name: &str,
    ranges: Vec<NumericRange>,
) -> Result<Option<NumericRange>, JsonFormatError> {
    let mut ranges = ranges
        .into_iter()
        .map(|range| match range {
            NumericRange::Integer(range) => Ok(range),
            NumericRange::Number(_) => Err(unsupported(
                name,
                "anyOf numeric ranges use incompatible integer and number domains",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    ranges.sort_by_key(|range| (range.minimum().is_some(), range.minimum()));
    let Some(first) = ranges.first().copied() else {
        return Ok(None);
    };
    let minimum = first.minimum();
    let mut maximum = first.maximum();
    for range in ranges.into_iter().skip(1) {
        let Some(current_maximum) = maximum else {
            return normalized_integer_union(name, minimum, None);
        };
        let Some(next_minimum) = range.minimum() else {
            return Err(unsupported(
                name,
                "anyOf integer ranges are not in canonical lower-bound order",
            ));
        };
        if next_minimum > current_maximum.saturating_add(1) {
            return Err(unsupported(
                name,
                "anyOf numeric ranges are disjoint and cannot be represented as one interval",
            ));
        }
        maximum = match (maximum, range.maximum()) {
            (_, None) => None,
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, _) => None,
        };
    }
    normalized_integer_union(name, minimum, maximum)
}

fn union_number_ranges(
    name: &str,
    ranges: Vec<NumericRange>,
) -> Result<Option<NumericRange>, JsonFormatError> {
    let mut ranges = ranges
        .into_iter()
        .map(|range| match range {
            NumericRange::Number(range) => Ok(range),
            NumericRange::Integer(_) => Err(unsupported(
                name,
                "anyOf numeric ranges use incompatible integer and number domains",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    ranges.sort_by(|left, right| number_first(*left).total_cmp(&number_first(*right)));
    let Some(first) = ranges.first().copied() else {
        return Ok(None);
    };
    let minimum = first.minimum();
    let mut maximum = first.maximum();
    for range in ranges.into_iter().skip(1) {
        let current_last = number_last(maximum);
        let next_first = number_first(range);
        let contiguous = current_last == f64::MAX
            || next_representable(current_last).is_some_and(|successor| next_first <= successor);
        if !contiguous {
            return Err(unsupported(
                name,
                "anyOf numeric ranges are disjoint and cannot be represented as one interval",
            ));
        }
        maximum = looser_maximum(maximum, range.maximum());
    }
    normalized_number_union(name, minimum, maximum)
}

fn number_first(range: NumberRange) -> f64 {
    match range.minimum() {
        Some(bound) if bound.is_exclusive() => {
            next_representable(bound.value().get()).unwrap_or(f64::MAX)
        }
        Some(bound) => bound.value().get(),
        None => -f64::MAX,
    }
}

fn number_last(maximum: Option<NumberBound>) -> f64 {
    match maximum {
        Some(bound) if bound.is_exclusive() => {
            previous_representable(bound.value().get()).unwrap_or(-f64::MAX)
        }
        Some(bound) => bound.value().get(),
        None => f64::MAX,
    }
}

fn looser_maximum(left: Option<NumberBound>, right: Option<NumberBound>) -> Option<NumberBound> {
    match (left, right) {
        (None, _) | (_, None) => None,
        (Some(left), Some(right)) => {
            let left_value = left.value().get();
            let right_value = right.value().get();
            if left_value < right_value {
                Some(right)
            } else if left_value > right_value || !left.is_exclusive() {
                Some(left)
            } else {
                Some(right)
            }
        }
    }
}

fn normalized_integer_union(
    name: &str,
    minimum: Option<i64>,
    maximum: Option<i64>,
) -> Result<Option<NumericRange>, JsonFormatError> {
    if minimum.is_none() && maximum.is_none() {
        return Ok(None);
    }
    IntegerRange::new(minimum, maximum)
        .map(NumericRange::Integer)
        .map(Some)
        .ok_or_else(|| unsupported(name, "anyOf integer range union failed to normalize"))
}

fn normalized_number_union(
    name: &str,
    minimum: Option<NumberBound>,
    maximum: Option<NumberBound>,
) -> Result<Option<NumericRange>, JsonFormatError> {
    if minimum.is_none() && maximum.is_none() {
        return Ok(None);
    }
    NumberRange::new(minimum, maximum)
        .map(NumericRange::Number)
        .map(Some)
        .ok_or_else(|| unsupported(name, "anyOf number range union failed to normalize"))
}

fn next_representable(value: f64) -> Option<f64> {
    if value == f64::MAX {
        return None;
    }
    if value == 0.0 {
        return Some(f64::from_bits(1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits + 1
    } else {
        bits - 1
    }))
}

fn previous_representable(value: f64) -> Option<f64> {
    if value == -f64::MAX {
        return None;
    }
    if value == 0.0 {
        return Some(f64::from_bits((1_u64 << 63) | 1));
    }
    let bits = value.to_bits();
    Some(f64::from_bits(if value > 0.0 {
        bits - 1
    } else {
        bits + 1
    }))
}

fn convert(
    name: &str,
    range: NumericRange,
    target_type: ScalarType,
) -> Result<NumericRange, JsonFormatError> {
    match (range, target_type) {
        (NumericRange::Integer(range), ScalarType::Int) => Ok(NumericRange::Integer(range)),
        (NumericRange::Number(range), ScalarType::Float) => Ok(NumericRange::Number(range)),
        (NumericRange::Number(range), ScalarType::Int) => {
            let lower = range
                .minimum()
                .map(|bound| integer_bound_from_f64(name, bound, true))
                .transpose()?;
            let upper = range
                .maximum()
                .map(|bound| integer_bound_from_f64(name, bound, false))
                .transpose()?;
            let range = IntegerRange::new(lower.flatten(), upper.flatten()).ok_or_else(|| {
                unsupported(
                    name,
                    "number range has no value in the signed 64-bit integer domain",
                )
            })?;
            Ok(NumericRange::Integer(range))
        }
        _ => Err(unsupported(
            name,
            "numeric range is incompatible with the intersected scalar type",
        )),
    }
}

pub(super) fn has_range_keywords(schema: &serde_json::Value) -> bool {
    ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"]
        .into_iter()
        .any(|keyword| schema.get(keyword).is_some())
}

#[derive(Clone, Copy)]
struct RawBound<'a> {
    value: &'a serde_json::Number,
    exclusive: bool,
}

fn lower_bounds<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Vec<RawBound<'a>>, JsonFormatError> {
    bounds(name, schema, "minimum", "exclusiveMinimum")
}

fn upper_bounds<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Vec<RawBound<'a>>, JsonFormatError> {
    bounds(name, schema, "maximum", "exclusiveMaximum")
}

fn bounds<'a>(
    name: &str,
    schema: &'a serde_json::Value,
    inclusive_name: &str,
    exclusive_name: &str,
) -> Result<Vec<RawBound<'a>>, JsonFormatError> {
    let inclusive = schema.get(inclusive_name);
    let exclusive = schema.get(exclusive_name);
    let inclusive_number = inclusive
        .map(|value| require_number(name, inclusive_name, value))
        .transpose()?;
    match exclusive {
        None => Ok(inclusive_number
            .map(|value| {
                vec![RawBound {
                    value,
                    exclusive: false,
                }]
            })
            .unwrap_or_default()),
        Some(serde_json::Value::Bool(true)) => inclusive_number
            .map(|value| {
                vec![RawBound {
                    value,
                    exclusive: true,
                }]
            })
            .ok_or_else(|| {
                unsupported(
                    name,
                    &format!("legacy `{exclusive_name}: true` requires `{inclusive_name}`"),
                )
            }),
        Some(serde_json::Value::Bool(false)) => Ok(inclusive_number
            .map(|value| {
                vec![RawBound {
                    value,
                    exclusive: false,
                }]
            })
            .unwrap_or_default()),
        Some(serde_json::Value::Number(value)) => {
            let mut bounds = inclusive_number
                .map(|value| {
                    vec![RawBound {
                        value,
                        exclusive: false,
                    }]
                })
                .unwrap_or_default();
            bounds.push(RawBound {
                value,
                exclusive: true,
            });
            Ok(bounds)
        }
        Some(_) => Err(unsupported(
            name,
            &format!("`{exclusive_name}` must be a number or legacy boolean"),
        )),
    }
}

fn require_number<'a>(
    name: &str,
    keyword: &str,
    value: &'a serde_json::Value,
) -> Result<&'a serde_json::Number, JsonFormatError> {
    value
        .as_number()
        .ok_or_else(|| unsupported(name, &format!("`{keyword}` must be a number")))
}

fn integer_range(
    name: &str,
    lower: &[RawBound<'_>],
    upper: &[RawBound<'_>],
) -> Result<Option<IntegerRange>, JsonFormatError> {
    let mut minimum = None;
    for bound in lower {
        let Some(candidate) = normalized_integer_bound(name, *bound, true)? else {
            continue;
        };
        minimum = Some(minimum.map_or(candidate, |minimum: i64| minimum.max(candidate)));
    }
    let mut maximum = None;
    for bound in upper {
        let Some(candidate) = normalized_integer_bound(name, *bound, false)? else {
            continue;
        };
        maximum = Some(maximum.map_or(candidate, |maximum: i64| maximum.min(candidate)));
    }
    match (minimum, maximum) {
        (None, None) => Ok(None),
        _ => IntegerRange::new(minimum, maximum)
            .map(Some)
            .ok_or_else(|| unsupported(name, "numeric range has no signed 64-bit integer values")),
    }
}

fn normalized_integer_bound(
    name: &str,
    bound: RawBound<'_>,
    lower: bool,
) -> Result<Option<i64>, JsonFormatError> {
    if let Some(value) = bound.value.as_i64() {
        return if bound.exclusive {
            if lower {
                value.checked_add(1).map(Some).ok_or_else(|| {
                    unsupported(name, "exclusive lower bound empties the i64 domain")
                })
            } else {
                value.checked_sub(1).map(Some).ok_or_else(|| {
                    unsupported(name, "exclusive upper bound empties the i64 domain")
                })
            }
        } else {
            Ok(Some(value))
        };
    }
    if bound.value.as_u64().is_some() {
        return if lower {
            Err(unsupported(
                name,
                "lower bound is above the signed 64-bit domain",
            ))
        } else {
            Ok(None)
        };
    }
    let value = crate::exact_f64_from_json_number(bound.value)
        .filter(|value| {
            value.abs() < MAX_CONTIGUOUS_INTEGER_F64 && value.fract() != 0.0
        })
        .ok_or_else(|| {
            unsupported(
                name,
                "floating-point integer bound is not exactly normalizable to i64; use an integer JSON token for integral endpoints",
            )
        })?;
    integer_bound_from_f64(
        name,
        if bound.exclusive {
            NumberBound::exclusive(
                FiniteF64::new(value)
                    .ok_or_else(|| unsupported(name, "numeric bound must be finite"))?,
            )
        } else {
            NumberBound::inclusive(
                FiniteF64::new(value)
                    .ok_or_else(|| unsupported(name, "numeric bound must be finite"))?,
            )
        },
        lower,
    )?
    .ok_or_else(|| unsupported(name, "numeric bound empties the signed 64-bit domain"))
    .map(Some)
}

fn integer_bound_from_f64(
    name: &str,
    bound: NumberBound,
    lower: bool,
) -> Result<Option<i64>, JsonFormatError> {
    let value = bound.value().get();
    if value.fract() != 0.0 && value.abs() >= MAX_CONTIGUOUS_INTEGER_F64 {
        return Err(unsupported(
            name,
            "number bound is too large to normalize exactly after integer intersection",
        ));
    }
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
    if value >= I64_UPPER_EXCLUSIVE {
        return if lower {
            Err(unsupported(
                name,
                "lower bound empties the signed 64-bit domain",
            ))
        } else {
            Ok(None)
        };
    }
    if value < i64::MIN as f64 {
        return if lower {
            Ok(None)
        } else {
            Err(unsupported(
                name,
                "upper bound empties the signed 64-bit domain",
            ))
        };
    }

    if value.fract() == 0.0 {
        let value = value as i64;
        if !bound.is_exclusive() {
            return Ok(Some(value));
        }
        return if lower {
            value
                .checked_add(1)
                .map(Some)
                .ok_or_else(|| unsupported(name, "exclusive lower bound empties the i64 domain"))
        } else {
            value
                .checked_sub(1)
                .map(Some)
                .ok_or_else(|| unsupported(name, "exclusive upper bound empties the i64 domain"))
        };
    }
    Ok(Some(if lower {
        value.ceil() as i64
    } else {
        value.floor() as i64
    }))
}

fn number_range(
    name: &str,
    lower: &[RawBound<'_>],
    upper: &[RawBound<'_>],
) -> Result<Option<NumberRange>, JsonFormatError> {
    let mut minimum = None;
    for bound in lower {
        let candidate = number_bound(name, *bound)?;
        minimum = stricter_minimum(minimum, Some(candidate));
    }
    let mut maximum = None;
    for bound in upper {
        let candidate = number_bound(name, *bound)?;
        maximum = stricter_maximum(maximum, Some(candidate));
    }
    match (minimum, maximum) {
        (None, None) => Ok(None),
        _ => NumberRange::new(minimum, maximum)
            .map(Some)
            .ok_or_else(|| unsupported(name, "numeric range is empty")),
    }
}

fn number_bound(name: &str, bound: RawBound<'_>) -> Result<NumberBound, JsonFormatError> {
    let value = finite_bound_value(name, bound.value)?;
    if bound.value.as_i64().is_none()
        && bound.value.as_u64().is_none()
        && value.get().fract() == 0.0
    {
        return Err(unsupported(
            name,
            "floating-point numeric bound rounded to an ambiguous integral value; use an integer JSON token for integral endpoints",
        ));
    }
    Ok(if bound.exclusive {
        NumberBound::exclusive(value)
    } else {
        NumberBound::inclusive(value)
    })
}

fn finite_bound_value(
    name: &str,
    value: &serde_json::Number,
) -> Result<FiniteF64, JsonFormatError> {
    crate::exact_f64_from_json_number(value)
        .and_then(FiniteF64::new)
        .ok_or_else(|| unsupported(name, "numeric bound must be a supported finite number"))
}

fn stricter_minimum(left: Option<NumberBound>, right: Option<NumberBound>) -> Option<NumberBound> {
    match (left, right) {
        (None, bound) | (bound, None) => bound,
        (Some(left), Some(right)) if left.value().get() > right.value().get() => Some(left),
        (Some(left), Some(right)) if right.value().get() > left.value().get() => Some(right),
        (Some(left), Some(right)) if left.is_exclusive() || right.is_exclusive() => {
            Some(NumberBound::exclusive(left.value()))
        }
        (Some(left), Some(_)) => Some(left),
    }
}

fn stricter_maximum(left: Option<NumberBound>, right: Option<NumberBound>) -> Option<NumberBound> {
    match (left, right) {
        (None, bound) | (bound, None) => bound,
        (Some(left), Some(right)) if left.value().get() < right.value().get() => Some(left),
        (Some(left), Some(right)) if right.value().get() < left.value().get() => Some(right),
        (Some(left), Some(right)) if left.is_exclusive() || right.is_exclusive() => {
            Some(NumberBound::exclusive(left.value()))
        }
        (Some(left), Some(_)) => Some(left),
    }
}

fn render_number_bound(
    out: &mut serde_json::Map<String, serde_json::Value>,
    bound: Option<NumberBound>,
    inclusive_name: &str,
    exclusive_name: &str,
) -> Result<(), JsonFormatError> {
    let Some(bound) = bound else {
        return Ok(());
    };
    out.insert(
        if bound.is_exclusive() {
            exclusive_name
        } else {
            inclusive_name
        }
        .into(),
        exact_json_number(bound.value().get())?,
    );
    Ok(())
}

fn exact_json_number(value: f64) -> Result<serde_json::Value, JsonFormatError> {
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
    const U64_UPPER_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

    if value == 0.0 {
        return Ok(0_i64.into());
    }
    if (i64::MIN as f64..I64_UPPER_EXCLUSIVE).contains(&value) {
        let integer = value as i64;
        if integer as f64 == value {
            return Ok(integer.into());
        }
    }
    if (0.0..U64_UPPER_EXCLUSIVE).contains(&value) {
        let integer = value as u64;
        if integer as f64 == value {
            return Ok(integer.into());
        }
    }
    if value.fract() == 0.0 {
        return Err(JsonFormatError::InvalidNumericRangeMetadata {
            reason: format!(
                "integral number bound `{value}` is outside the exact JSON integer token domain"
            ),
        });
    }
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or_else(|| JsonFormatError::InvalidNumericRangeMetadata {
            reason: format!("number bound `{value}` is not finite"),
        })
}

fn display(range: NumericRange) -> String {
    match range {
        NumericRange::Integer(range) => format!(
            "[{}, {}]",
            range
                .minimum()
                .map_or_else(|| "-inf".into(), |value| value.to_string()),
            range
                .maximum()
                .map_or_else(|| "inf".into(), |value| value.to_string())
        ),
        NumericRange::Number(range) => {
            let minimum = range.minimum();
            let maximum = range.maximum();
            format!(
                "{}{}, {}{}",
                minimum.map_or('(', |bound| if bound.is_exclusive() { '(' } else { '[' }),
                minimum.map_or_else(|| "-inf".into(), |bound| bound.value().get().to_string()),
                maximum.map_or_else(|| "inf".into(), |bound| bound.value().get().to_string()),
                maximum.map_or(')', |bound| if bound.is_exclusive() { ')' } else { ']' }),
            )
        }
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
