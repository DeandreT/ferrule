use ir::{Decimal96, DecimalError, MAX_DECIMAL_LEXICAL_BYTES};

#[test]
fn numeric_text_keeps_plain_and_binary64_exponent_paths_distinct() {
    for (text, expected) in [
        (" 1 ", "1"),
        ("\t.1E+2\n", "10"),
        ("1.e2", "100"),
        ("0e9999", "0"),
        ("1e-9999", "0"),
        ("-1.20E+2", "-120"),
        (
            "0.1234567890123456789012345678",
            "0.1234567890123456789012345678",
        ),
        ("0.1234567890123456789012345678e0", "0.123456789012346"),
        (
            "79228162514264337593543950335",
            "79228162514264337593543950335",
        ),
    ] {
        assert_eq!(
            Decimal96::parse_number(text).unwrap().to_string(),
            expected,
            "{text:?}"
        );
    }
    for text in [
        "NaN",
        " INF ",
        "-INF",
        "1e9999",
        "-1e9999",
        "7.9228162514264337593543950335e28",
    ] {
        assert_eq!(
            Decimal96::parse_number(text),
            Err(DecimalError::Overflow),
            "{text:?}"
        );
    }
}

#[test]
fn numeric_text_rejects_invalid_syntax_without_weakening_plain_parsers() {
    for text in [
        "",
        "\t",
        "e0",
        "1e",
        "++1",
        "1e++1",
        "1,000",
        "1_0",
        "Infinity",
        "+INF",
        "nan",
        "٠",
        "1\u{a0}0",
        "\u{180e}1",
        "\u{200b}1",
        "\u{feff}1",
    ] {
        assert_eq!(
            Decimal96::parse_number(text),
            Err(DecimalError::InvalidNumberLexical),
            "{text:?}"
        );
    }
    for text in [" 1", "1e0"] {
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

#[test]
fn numeric_text_bounds_count_the_original_utf8_before_trimming() {
    let accepted = " ".repeat(MAX_DECIMAL_LEXICAL_BYTES - 1) + "1";
    assert_eq!(Decimal96::parse_number(&accepted).unwrap().to_string(), "1");
    assert_eq!(
        Decimal96::parse_number(&(" ".repeat(MAX_DECIMAL_LEXICAL_BYTES) + "1")),
        Err(DecimalError::InputTooLong)
    );
    assert_eq!(
        Decimal96::parse_number(&" ".repeat(MAX_DECIMAL_LEXICAL_BYTES)),
        Err(DecimalError::InvalidNumberLexical)
    );
    assert_eq!(
        Decimal96::parse_number(&("\u{a0}".repeat(MAX_DECIMAL_LEXICAL_BYTES / 2) + "1")),
        Err(DecimalError::InputTooLong)
    );
}
