/// Converts `value` to `f64` only when the conversion is exact: the integer's
/// binary significand must fit in the `f64` mantissa. Powers of two beyond
/// the contiguous integer range, including `i64::MIN`, are therefore exact.
pub fn i64_to_exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}

#[cfg(test)]
mod tests {
    use super::i64_to_exact_f64;

    #[test]
    fn accepts_only_exactly_representable_integers() {
        let contiguous = 1_i64 << f64::MANTISSA_DIGITS;
        assert_eq!(i64_to_exact_f64(0), Some(0.0));
        assert_eq!(i64_to_exact_f64(contiguous), Some(contiguous as f64));
        assert_eq!(i64_to_exact_f64(-contiguous), Some(-contiguous as f64));
        assert_eq!(i64_to_exact_f64(contiguous + 1), None);
        assert_eq!(
            i64_to_exact_f64(contiguous + 2),
            Some((contiguous + 2) as f64)
        );
        assert_eq!(i64_to_exact_f64(i64::MIN), Some(i64::MIN as f64));
        assert_eq!(i64_to_exact_f64(i64::MAX), None);
    }
}
