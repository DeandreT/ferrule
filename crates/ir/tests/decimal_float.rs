use ir::{Decimal96, DecimalError};

#[test]
fn explicit_binary64_conversion_has_canonical_fifteen_digit_results() {
    for (input, expected) in [
        (0.0, "0"),
        (-0.0, "0"),
        (0.1, "0.1"),
        (-0.1, "-0.1"),
        (1.0 / 3.0, "0.333333333333333"),
        (1.2345678901234567, "1.23456789012346"),
        (9007199254740991.0, "9007199254740990"),
        (1e28, "10000000000000000000000000000"),
        (1e-28, "0.0000000000000000000000000001"),
        (5e-29, "0"),
        (1.5e-28, "0.0000000000000000000000000002"),
        (2.5e-28, "0.0000000000000000000000000002"),
    ] {
        let actual = Decimal96::from_f64(input).unwrap();
        assert_eq!(actual.to_string(), expected, "{input:?}");
        assert_eq!(actual, Decimal96::parse_exact(expected).unwrap());
        assert_eq!(Decimal96::from_f64(-input), Ok(actual.negated()));
    }
}

#[test]
fn underflow_overflow_and_nonfinite_values_have_explicit_boundaries() {
    for bits in [
        0,
        1,
        0x000f_ffff_ffff_ffff,
        (5e-29_f64).to_bits() - 1,
        (5e-29_f64).to_bits() + 1,
    ] {
        assert_eq!(
            Decimal96::from_f64(f64::from_bits(bits)),
            Ok(Decimal96::ZERO)
        );
        assert_eq!(
            Decimal96::from_f64(f64::from_bits(bits | (1 << 63))),
            Ok(Decimal96::ZERO)
        );
    }
    // The first adjacent value still scales to the exact binary64 half-unit.
    let above_half = f64::from_bits((5e-29_f64).to_bits() + 2);
    assert_eq!(
        Decimal96::from_f64(above_half).unwrap().to_string(),
        "0.0000000000000000000000000001"
    );
    assert_eq!(
        Decimal96::from_f64(f64::from_bits(0x45ef_ffff_ffff_ffff))
            .unwrap()
            .to_string(),
        "79228162514264300000000000000"
    );
    for input in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        -f64::MAX,
        f64::from_bits(0x45f0_0000_0000_0000),
        -f64::from_bits(0x45f0_0000_0000_0000),
    ] {
        assert_eq!(Decimal96::from_f64(input), Err(DecimalError::Overflow));
    }
}

#[test]
fn explicit_conversion_does_not_widen_plain_decimal_parsing() {
    for text in ["1e0", " 1", "1 ", "NaN", "Infinity"] {
        assert_eq!(
            Decimal96::parse_exact(text),
            Err(DecimalError::InvalidLexical)
        );
        assert_eq!(
            Decimal96::parse_rounded(text),
            Err(DecimalError::InvalidLexical)
        );
    }
}
