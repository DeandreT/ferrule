use std::collections::{BTreeSet, HashSet};

use ir::{Decimal96, DecimalError, MAX_DECIMAL_COEFFICIENT, MAX_DECIMAL_LEXICAL_BYTES};

fn parse(text: &str) -> Decimal96 {
    Decimal96::parse_exact(text).unwrap()
}

#[test]
fn exact_plain_decimal_values_and_canonical_text() {
    for (input, expected) in [
        ("0", "0"),
        ("-0", "0"),
        ("+0.000", "0"),
        ("0012.5000", "12.5"),
        (".25", "0.25"),
        ("3.", "3"),
        ("-0.0000010", "-0.000001"),
        ("9007199254740993", "9007199254740993"),
        (
            "0.1234567890123456789012345678",
            "0.1234567890123456789012345678",
        ),
        (
            "79228162514264337593543950335",
            "79228162514264337593543950335",
        ),
        (
            "-79228162514264337593543950335",
            "-79228162514264337593543950335",
        ),
    ] {
        let value = parse(input);
        assert_eq!(value.to_string(), expected, "{input}");
        assert_eq!(parse(&value.to_string()), value);
        assert_eq!(Decimal96::parse_rounded(input), Ok(value));
    }
    assert_eq!(Decimal96::from_parts(true, 0, 28), Ok(Decimal96::ZERO));
    assert_eq!(Decimal96::from_parts(false, 25_000, 4), Ok(parse("2.5")));
    assert_eq!(
        Decimal96::from_parts(false, 1, 29),
        Err(DecimalError::InvalidScale)
    );
    assert_eq!(
        Decimal96::from_parts(false, MAX_DECIMAL_COEFFICIENT + 1, 0),
        Err(DecimalError::Overflow)
    );
}

#[test]
fn nearest_even_rounding_is_explicit_and_symmetric() {
    for (input, expected) in [
        (
            "0.12345678901234567890123456785",
            "0.1234567890123456789012345678",
        ),
        (
            "0.12345678901234567890123456795",
            "0.123456789012345678901234568",
        ),
        ("0.00000000000000000000000000005", "0"),
        (
            "0.00000000000000000000000000015",
            "0.0000000000000000000000000002",
        ),
        (
            "0.00000000000000000000000000005001",
            "0.0000000000000000000000000001",
        ),
        ("0.00000000000000000000000000004999", "0"),
        (
            "7922816251426433759354395033.55",
            "7922816251426433759354395034",
        ),
    ] {
        assert_eq!(
            Decimal96::parse_exact(input),
            Err(DecimalError::PrecisionLoss),
            "{input}"
        );
        let positive = Decimal96::parse_rounded(input).unwrap();
        assert_eq!(positive.to_string(), expected, "{input}");
        assert_eq!(
            Decimal96::parse_rounded(&format!("-{input}")),
            Ok(positive.negated())
        );
    }
    assert_eq!(
        Decimal96::parse_exact("1.0000000000000000000000000000000"),
        Ok(parse("1"))
    );
}

#[test]
fn lexical_failures_and_overflow_are_not_silent_zeroes() {
    for text in [
        "", "+", "-", ".", "1..0", "1e0", "NaN", "Infinity", " 1", "1 ", "1,000", "1_000", "١",
        "1\0", "--1",
    ] {
        assert_eq!(
            Decimal96::parse_exact(text),
            Err(DecimalError::InvalidLexical),
            "{text:?}"
        );
        assert_eq!(
            Decimal96::parse_rounded(text),
            Err(DecimalError::InvalidLexical),
            "{text:?}"
        );
    }
    for text in [
        "79228162514264337593543950336",
        "99999999999999999999999999999",
        "-79228162514264337593543950336",
    ] {
        assert_eq!(Decimal96::parse_exact(text), Err(DecimalError::Overflow));
        assert_eq!(Decimal96::parse_rounded(text), Err(DecimalError::Overflow));
    }
    assert_eq!(
        Decimal96::parse_exact(&"0".repeat(MAX_DECIMAL_LEXICAL_BYTES)),
        Ok(Decimal96::ZERO)
    );
    assert_eq!(
        Decimal96::parse_rounded(&"0".repeat(MAX_DECIMAL_LEXICAL_BYTES + 1)),
        Err(DecimalError::InputTooLong)
    );
}

#[test]
fn exact_addition_and_wide_scale_alignment() {
    for (left, right, expected) in [
        ("0.1", "0.2", "0.3"),
        ("-0.1", "0.1", "0"),
        ("9007199254740993", "0.1", "9007199254740993.1"),
        (
            "7922816251426433759354395033.5",
            "-7922816251426433759354395033",
            "0.5",
        ),
        (
            "79228162514264337593543950335",
            "0.0000000000000000000000000001",
            "79228162514264337593543950335",
        ),
        (
            "79228162514264337593543950335",
            "0.4",
            "79228162514264337593543950335",
        ),
        (
            "79228162514264337593543950335",
            "-0.5",
            "79228162514264337593543950334",
        ),
        (
            "7922816251426433759354395033.5",
            "0.05",
            "7922816251426433759354395034",
        ),
        (
            "7922816251426433759354395033.4",
            "0.049",
            "7922816251426433759354395033.4",
        ),
        (
            "0.0000000000000000000000000001",
            "0.0000000000000000000000000001",
            "0.0000000000000000000000000002",
        ),
    ] {
        let a = parse(left);
        let b = parse(right);
        let result = parse(expected);
        assert_eq!(a.add_rounded(b), Ok(result), "{left} + {right}");
        assert_eq!(b.add_rounded(a), Ok(result));
        assert_eq!(a.negated().add_rounded(b.negated()), Ok(result.negated()));
        assert_eq!(a.subtract_rounded(b.negated()), Ok(result));
    }
    let maximum = Decimal96::from_parts(false, MAX_DECIMAL_COEFFICIENT, 0).unwrap();
    for value in [parse("1"), parse("0.5"), maximum] {
        assert_eq!(maximum.add_rounded(value), Err(DecimalError::Overflow));
        assert_eq!(
            maximum.negated().add_rounded(value.negated()),
            Err(DecimalError::Overflow)
        );
    }
    let half = Decimal96::parse_rounded("0.00000000000000000000000000005").unwrap();
    assert_eq!(half.add_rounded(half), Ok(Decimal96::ZERO));
}

#[test]
fn numeric_order_equality_hashes_and_algebraic_controls() {
    let mut values = Vec::new();
    for scale in 0..=28 {
        for coefficient in [
            0,
            1,
            9,
            10,
            123,
            65536,
            u128::from(u64::MAX),
            MAX_DECIMAL_COEFFICIENT,
        ] {
            for negative in [false, true] {
                let value = Decimal96::from_parts(negative, coefficient, scale).unwrap();
                assert_eq!(value.add_rounded(Decimal96::ZERO), Ok(value));
                assert_eq!(value.add_rounded(value.negated()), Ok(Decimal96::ZERO));
                assert_eq!(value.subtract_rounded(value), Ok(Decimal96::ZERO));
                assert_eq!(parse(&value.to_string()), value);
                values.push(value);
            }
        }
    }
    let ordered = values.iter().copied().collect::<BTreeSet<_>>();
    let hashed = values.iter().copied().collect::<HashSet<_>>();
    assert_eq!(ordered.len(), hashed.len());
    let sorted = ordered.into_iter().collect::<Vec<_>>();
    for pair in sorted.windows(2) {
        assert!(pair[0] < pair[1]);
        assert!(pair[0].negated() > pair[1].negated());
    }
    assert_eq!(parse("10.00"), parse("0010"));
    assert!(parse("-10") < parse("-0.01"));
    assert!(parse("0.0000000000000000000000000001") > parse("0"));
}
