//! Scalar key ordering shared by scope sorting in the interpreter and Rust runtime.

use std::cmp::Ordering;

use ir::Value;

/// Compares scalar keys using the scope-sort policy, not expression comparison
/// or grouping equality.
///
/// The ascending domain order is absence and JSON null (tied), Boolean,
/// Int/Float, string, XML nil, then Decimal. Booleans and strings use their
/// natural order; decimals compare exactly within their separate domain and
/// are never coerced to Int/Float. All XML nil keys tie.
///
/// Int/Float keys compare numerically without first rounding integers to floats.
/// Signed zeroes and integer zero tie. All NaN signs and payloads tie after
/// every other numeric key, including positive infinity. Comparator ties do not
/// imply that the underlying values have the same identity.
pub fn compare_scope_sort_values(left: &Value, right: &Value) -> Ordering {
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

/// Compares an integer and a non-NaN float without rounding the integer first.
fn compare_int_float(integer: i64, float: f64) -> Ordering {
    // i64::MAX rounds up to the exclusive upper bound 2^63 as f64.
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

    const NAN_BITS: [u64; 4] = [
        0x7ff8_0000_0000_0001,
        0xfff8_0000_0000_0042,
        0x7ff0_0000_0000_0001,
        0xfff0_0000_0000_0001,
    ];

    fn decimal(text: &str) -> Value {
        Value::decimal(ir::Decimal96::parse_exact(text).unwrap())
    }

    #[test]
    fn scope_sort_domains_have_explicit_pairwise_precedence() {
        let domains = [
            vec![Value::Null, Value::json_null()],
            vec![Value::Bool(false), Value::Bool(true)],
            vec![
                Value::Float(f64::NEG_INFINITY),
                Value::Int(i64::MIN),
                Value::Int(0),
                Value::Int(i64::MAX),
                Value::Float(f64::INFINITY),
                Value::Float(f64::NAN),
            ],
            vec![Value::String(String::new()), Value::String("0".into())],
            vec![Value::xml_nil()],
            vec![decimal("-79228162514264337593543950335"), decimal("0")],
        ];
        for (index, earlier) in domains.iter().enumerate() {
            for later in &domains[index + 1..] {
                for left in earlier {
                    for right in later {
                        assert_eq!(compare_scope_sort_values(left, right), Ordering::Less);
                        assert_eq!(compare_scope_sort_values(right, left), Ordering::Greater);
                    }
                }
            }
        }
    }

    #[test]
    fn mixed_numeric_pairs_keep_exact_precision_and_i64_boundaries() {
        let cases = [
            (i64::MIN, f64::NEG_INFINITY, Ordering::Greater),
            (i64::MIN, -9_223_372_036_854_777_856.0, Ordering::Greater),
            (i64::MIN, -9_223_372_036_854_775_808.0, Ordering::Equal),
            (
                i64::MIN + 1,
                -9_223_372_036_854_775_808.0,
                Ordering::Greater,
            ),
            (i64::MIN, -9_223_372_036_854_774_784.0, Ordering::Less),
            (
                -9_007_199_254_740_993,
                -9_007_199_254_740_992.0,
                Ordering::Less,
            ),
            (
                -9_007_199_254_740_992,
                -9_007_199_254_740_992.0,
                Ordering::Equal,
            ),
            (-1, -1.5, Ordering::Greater),
            (-1, -0.5, Ordering::Less),
            (0, -f64::from_bits(1), Ordering::Greater),
            (0, -0.0, Ordering::Equal),
            (0, 0.0, Ordering::Equal),
            (0, f64::from_bits(1), Ordering::Less),
            (1, 0.5, Ordering::Greater),
            (1, 1.5, Ordering::Less),
            (
                9_007_199_254_740_992,
                9_007_199_254_740_992.0,
                Ordering::Equal,
            ),
            (
                9_007_199_254_740_993,
                9_007_199_254_740_992.0,
                Ordering::Greater,
            ),
            (
                9_007_199_254_740_993,
                9_007_199_254_740_994.0,
                Ordering::Less,
            ),
            (i64::MAX, 9_223_372_036_854_774_784.0, Ordering::Greater),
            (i64::MAX, 9_223_372_036_854_775_808.0, Ordering::Less),
            (i64::MAX, 9_223_372_036_854_777_856.0, Ordering::Less),
            (i64::MAX, f64::INFINITY, Ordering::Less),
        ];
        for (integer, float, expected) in cases {
            let left = Value::Int(integer);
            let right = Value::Float(float);
            assert_eq!(compare_scope_sort_values(&left, &right), expected);
            assert_eq!(compare_scope_sort_values(&right, &left), expected.reverse());
        }
    }

    #[test]
    fn same_domain_pairs_keep_exact_and_lexical_order_and_null_ties() {
        let cases = [
            (Value::Null, Value::json_null(), Ordering::Equal),
            (Value::xml_nil(), Value::xml_nil(), Ordering::Equal),
            (Value::Bool(false), Value::Bool(true), Ordering::Less),
            (Value::Int(i64::MIN), Value::Int(i64::MAX), Ordering::Less),
            (Value::Float(-0.0), Value::Float(0.0), Ordering::Equal),
            (
                Value::Float(f64::NEG_INFINITY),
                Value::Float(-f64::MAX),
                Ordering::Less,
            ),
            (
                Value::Float(f64::MAX),
                Value::Float(f64::INFINITY),
                Ordering::Less,
            ),
            (
                Value::String("10".into()),
                Value::String("2".into()),
                Ordering::Less,
            ),
            (
                Value::String("é".into()),
                Value::String("e\u{301}".into()),
                Ordering::Greater,
            ),
            (decimal("-10"), decimal("-2.5"), Ordering::Less),
            (decimal("1.00"), decimal("1"), Ordering::Equal),
            (
                decimal("9007199254740992"),
                decimal("9007199254740993"),
                Ordering::Less,
            ),
        ];
        for (left, right, expected) in cases {
            assert_eq!(compare_scope_sort_values(&left, &right), expected);
            assert_eq!(compare_scope_sort_values(&right, &left), expected.reverse());
        }
    }

    #[test]
    fn nan_signs_and_payloads_tie_after_all_other_numeric_keys() {
        let numbers = [
            Value::Float(f64::NEG_INFINITY),
            Value::Int(i64::MIN),
            Value::Float(-0.0),
            Value::Int(0),
            Value::Float(0.0),
            Value::Int(i64::MAX),
            Value::Float(f64::INFINITY),
        ];
        for bits in NAN_BITS {
            let nan = Value::Float(f64::from_bits(bits));
            for number in &numbers {
                assert_eq!(compare_scope_sort_values(&nan, number), Ordering::Greater);
                assert_eq!(compare_scope_sort_values(number, &nan), Ordering::Less);
            }
            for other in NAN_BITS {
                assert_eq!(
                    compare_scope_sort_values(&nan, &Value::Float(f64::from_bits(other))),
                    Ordering::Equal,
                );
            }
        }
    }

    #[test]
    fn scope_sort_order_is_reflexive_antisymmetric_and_transitive() {
        let mut values = vec![
            Value::Null,
            Value::json_null(),
            Value::Bool(false),
            Value::Bool(true),
            Value::Float(f64::NEG_INFINITY),
            Value::Int(i64::MIN),
            Value::Float(-9_223_372_036_854_775_808.0),
            Value::Int(i64::MIN + 1),
            Value::Int(-9_007_199_254_740_993),
            Value::Float(-9_007_199_254_740_992.0),
            Value::Float(-1.5),
            Value::Int(-1),
            Value::Float(-f64::from_bits(1)),
            Value::Float(-0.0),
            Value::Int(0),
            Value::Float(0.0),
            Value::Float(f64::from_bits(1)),
            Value::Float(1.5),
            Value::Int(2),
            Value::Float(9_007_199_254_740_992.0),
            Value::Int(9_007_199_254_740_993),
            Value::Int(i64::MAX),
            Value::Float(9_223_372_036_854_775_808.0),
            Value::Float(f64::INFINITY),
            Value::String(String::new()),
            Value::String("value".into()),
            Value::xml_nil(),
            decimal("-9007199254740993"),
            decimal("0"),
            decimal("1"),
            decimal("1.00"),
            decimal("9007199254740993"),
        ];
        values.extend(NAN_BITS.map(|bits| Value::Float(f64::from_bits(bits))));
        for left in &values {
            assert_eq!(compare_scope_sort_values(left, left), Ordering::Equal);
            for right in &values {
                let ordering = compare_scope_sort_values(left, right);
                assert_eq!(ordering, compare_scope_sort_values(right, left).reverse());
                for last in &values {
                    if ordering != Ordering::Greater
                        && compare_scope_sort_values(right, last) != Ordering::Greater
                    {
                        assert_ne!(
                            compare_scope_sort_values(left, last),
                            Ordering::Greater,
                            "{left:?} <= {right:?} <= {last:?}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn stable_sort_keeps_null_zero_nan_xml_nil_and_decimal_ties() {
        let mut keys = [
            ("nan-negative", Value::Float(f64::from_bits(NAN_BITS[1]))),
            ("zero-negative", Value::Float(-0.0)),
            ("json-null", Value::json_null()),
            ("decimal-first", decimal("1.0")),
            ("zero-integer", Value::Int(0)),
            ("absence", Value::Null),
            ("nan-positive", Value::Float(f64::from_bits(NAN_BITS[0]))),
            ("xml-nil-first", Value::xml_nil()),
            ("zero-positive", Value::Float(0.0)),
            ("decimal-second", decimal("1.00")),
            ("nan-signaling", Value::Float(f64::from_bits(NAN_BITS[2]))),
            ("xml-nil-second", Value::xml_nil()),
        ];
        keys.sort_by(|(_, left), (_, right)| compare_scope_sort_values(left, right));
        assert_eq!(
            keys.map(|(label, _)| label),
            [
                "json-null",
                "absence",
                "zero-negative",
                "zero-integer",
                "zero-positive",
                "nan-negative",
                "nan-positive",
                "nan-signaling",
                "xml-nil-first",
                "xml-nil-second",
                "decimal-first",
                "decimal-second",
            ],
        );
    }
}
