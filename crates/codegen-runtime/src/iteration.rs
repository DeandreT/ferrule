use std::cmp::Ordering;

use crate::{RuntimeError, Value};

/// Direction applied to one generated sequence sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// One parent-context sequence window after its graph bounds have been
/// evaluated and converted to non-negative item counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceWindow {
    SkipFirst(usize),
    First(usize),
    From(usize),
    FromTo { first: usize, last: usize },
    Last(usize),
}

/// Stably orders candidates by already-evaluated scalar keys.
///
/// Generated code evaluates each key exactly once before calling this helper.
/// The const key count keeps candidate keys and directions aligned. Equal or
/// incomparable key tuples retain their input order.
pub fn sort_candidates<T, const N: usize>(
    mut candidates: Vec<(T, [Value; N])>,
    directions: [SortDirection; N],
) -> Vec<T> {
    candidates.sort_by(|(_, left), (_, right)| {
        left.iter()
            .zip(right)
            .zip(directions)
            .find_map(|((left, right), direction)| {
                let ordering = sort_value_ordering(left, right);
                let ordering = match direction {
                    SortDirection::Ascending => ordering,
                    SortDirection::Descending => ordering.reverse(),
                };
                (ordering != Ordering::Equal).then_some(ordering)
            })
            .unwrap_or(Ordering::Equal)
    });
    candidates
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect()
}

/// Converts a scalar window bound using the engine's non-negative item-count
/// rules. Floats truncate toward zero and strings must contain an `i64`.
pub fn item_count(node: u32, value: Value) -> Result<usize, RuntimeError> {
    let count = match &value {
        Value::Int(value) => Some(*value),
        Value::Float(value) if value.is_finite() => Some(value.trunc() as i64),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        Value::Null | Value::XmlNil(_) | Value::Bool(_) | Value::Float(_) => None,
    };
    count
        .map(|count| count.max(0) as usize)
        .ok_or(RuntimeError::NotAnItemCount {
            node,
            found: value.type_name(),
        })
}

/// Applies sequence windows in declaration order.
pub fn apply_sequence_windows<T>(mut items: Vec<T>, windows: &[SequenceWindow]) -> Vec<T> {
    for window in windows {
        items = match *window {
            SequenceWindow::SkipFirst(count) => items.into_iter().skip(count).collect(),
            SequenceWindow::First(count) => items.into_iter().take(count).collect(),
            SequenceWindow::From(position) => {
                items.into_iter().skip(position.saturating_sub(1)).collect()
            }
            SequenceWindow::FromTo { first, last } => {
                let skip = first.saturating_sub(1);
                let count = last.saturating_sub(skip);
                items.into_iter().skip(skip).take(count).collect()
            }
            SequenceWindow::Last(count) => {
                let skip = items.len().saturating_sub(count);
                items.into_iter().skip(skip).collect()
            }
        };
    }
    items
}

fn sort_value_ordering(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Int(left), Value::Int(right)) => left.cmp(right),
        (Value::Float(left), Value::Float(right)) => {
            left.partial_cmp(right).unwrap_or(Ordering::Equal)
        }
        (Value::Int(left), Value::Float(right)) if right.is_finite() => {
            compare_int_float(*left, *right)
        }
        (Value::Float(left), Value::Int(right)) if left.is_finite() => {
            compare_int_float(*right, *left).reverse()
        }
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

/// Compares an integer and a finite float without rounding the integer first.
pub(crate) fn compare_int_float(integer: i64, float: f64) -> Ordering {
    if float >= i64::MAX as f64 {
        return Ordering::Less;
    }
    if float < i64::MIN as f64 {
        return Ordering::Greater;
    }

    let truncated = float.trunc() as i64;
    match integer.cmp(&truncated) {
        Ordering::Equal if float.fract().is_sign_positive() && float.fract() != 0.0 => {
            Ordering::Less
        }
        Ordering::Equal if float.fract().is_sign_negative() && float.fract() != 0.0 => {
            Ordering::Greater
        }
        ordering => ordering,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_multi_key_sort_preserves_ties_and_mixed_directions() {
        let candidates = vec![
            ("first-a", [Value::Int(2), Value::String("a".into())]),
            ("b", [Value::Int(2), Value::String("b".into())]),
            ("second-a", [Value::Int(2), Value::String("a".into())]),
            ("low", [Value::Int(1), Value::String("z".into())]),
        ];

        assert_eq!(
            sort_candidates(
                candidates,
                [SortDirection::Descending, SortDirection::Ascending],
            ),
            ["first-a", "second-a", "b", "low"]
        );
    }

    #[test]
    fn sort_matches_exact_mixed_numeric_null_and_incomparable_ordering() {
        let precise = sort_candidates(
            vec![
                ("integer", [Value::Int(9_007_199_254_740_993)]),
                ("float", [Value::Float(9_007_199_254_740_992.0)]),
            ],
            [SortDirection::Ascending],
        );
        assert_eq!(precise, ["float", "integer"]);

        let null_first = sort_candidates(
            vec![
                ("value", [Value::String("value".into())]),
                ("null", [Value::Null]),
            ],
            [SortDirection::Ascending],
        );
        assert_eq!(null_first, ["null", "value"]);

        let incomparable = sort_candidates(
            vec![
                ("xml-nil", [Value::xml_nil()]),
                ("string", [Value::String("value".into())]),
                ("nan", [Value::Float(f64::NAN)]),
            ],
            [SortDirection::Ascending],
        );
        assert_eq!(incomparable, ["xml-nil", "string", "nan"]);
    }

    #[test]
    fn item_counts_truncate_clamp_parse_and_retain_typed_errors() {
        assert_eq!(item_count(1, Value::Int(-3)), Ok(0));
        assert_eq!(item_count(2, Value::Float(3.9)), Ok(3));
        assert_eq!(item_count(3, Value::String(" 4 ".into())), Ok(4));
        assert_eq!(item_count(4, Value::Float(f64::MAX)), Ok(i64::MAX as usize));
        assert_eq!(
            item_count(5, Value::Bool(true)),
            Err(RuntimeError::NotAnItemCount {
                node: 5,
                found: "bool",
            })
        );
        assert_eq!(
            item_count(6, Value::Float(f64::INFINITY)),
            Err(RuntimeError::NotAnItemCount {
                node: 6,
                found: "float",
            })
        );
    }

    #[test]
    fn every_window_is_one_based_bounded_and_ordered() {
        let values = || (1..=8).collect::<Vec<_>>();
        assert_eq!(
            apply_sequence_windows(values(), &[SequenceWindow::SkipFirst(2)]),
            [3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            apply_sequence_windows(values(), &[SequenceWindow::First(2)]),
            [1, 2]
        );
        assert_eq!(
            apply_sequence_windows(values(), &[SequenceWindow::From(3)]),
            [3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            apply_sequence_windows(values(), &[SequenceWindow::FromTo { first: 3, last: 5 }],),
            [3, 4, 5]
        );
        assert_eq!(
            apply_sequence_windows(values(), &[SequenceWindow::Last(2)]),
            [7, 8]
        );
        assert_eq!(
            apply_sequence_windows(
                values(),
                &[SequenceWindow::SkipFirst(2), SequenceWindow::First(3)],
            ),
            [3, 4, 5]
        );
        assert_eq!(
            apply_sequence_windows(
                values(),
                &[SequenceWindow::First(3), SequenceWindow::SkipFirst(2)],
            ),
            [3]
        );
    }

    #[test]
    fn zero_and_out_of_range_windows_match_engine_boundaries() {
        assert_eq!(
            apply_sequence_windows(vec![1, 2], &[SequenceWindow::First(0)]),
            Vec::<i32>::new()
        );
        assert_eq!(
            apply_sequence_windows(vec![1, 2], &[SequenceWindow::Last(0)]),
            Vec::<i32>::new()
        );
        assert_eq!(
            apply_sequence_windows(vec![1, 2], &[SequenceWindow::From(0)]),
            [1, 2]
        );
        assert_eq!(
            apply_sequence_windows(vec![1, 2], &[SequenceWindow::FromTo { first: 4, last: 2 }],),
            Vec::<i32>::new()
        );
        assert_eq!(
            apply_sequence_windows(vec![1, 2], &[SequenceWindow::SkipFirst(9)]),
            Vec::<i32>::new()
        );
    }
}
