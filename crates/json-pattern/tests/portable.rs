use json_pattern::{
    CompileErrorKind, DEFAULT_MATCH_WORK_LIMIT, MAX_AST_NODES, MAX_INSTRUCTIONS, MAX_PARSE_DEPTH,
    MAX_SOURCE_BYTES, MatchError, PortableJsonPattern,
};

fn matches(pattern: &str, value: &str) -> bool {
    match PortableJsonPattern::compile(pattern) {
        Ok(pattern) => pattern.is_match(value) == Ok(true),
        Err(_) => false,
    }
}

fn compiled(pattern: &str) -> PortableJsonPattern {
    match PortableJsonPattern::compile(pattern) {
        Ok(pattern) => pattern,
        Err(error) => panic!("pattern {pattern:?} did not compile: {error}"),
    }
}

fn compile_error(pattern: &str) -> json_pattern::CompileError {
    match PortableJsonPattern::compile(pattern) {
        Ok(_) => panic!("pattern {pattern:?} unexpectedly compiled"),
        Err(error) => error,
    }
}

#[test]
fn default_matching_is_unanchored_and_empty_patterns_match() {
    assert!(matches("", ""));
    assert!(matches("", "anything"));
    assert!(matches("needle", "a needle in text"));
    assert!(!matches("needle", "need"));
}

#[test]
fn anchors_use_absolute_scalar_positions() {
    assert!(matches("^abc$", "abc"));
    assert!(!matches("^abc$", "xabc"));
    assert!(!matches("^abc$", "abcx"));
    assert!(matches("^$", ""));
    assert!(!matches("^$", "x"));
    assert!(matches("^😀$", "😀"));
}

#[test]
fn end_anchor_accepts_one_final_line_terminator_sequence() {
    for value in ["abc\n", "abc\r", "abc\u{2028}", "abc\u{2029}", "abc\r\n"] {
        assert!(matches("abc$", value), "{value:?}");
    }
    assert!(!matches("abc$", "abc\n\n"));
    assert!(!matches(r"\r$", "\r\n"));
    assert!(matches(r"\r\n$", "\r\n"));
}

#[test]
fn dot_excludes_the_portable_line_terminators() {
    assert!(matches("^.$", "x"));
    assert!(matches("^.$", "😀"));
    for value in ["\n", "\r", "\u{2028}", "\u{2029}"] {
        assert!(!matches("^.$", value), "{value:?}");
    }
}

#[test]
fn alternation_and_both_group_forms_work() {
    assert!(matches("^(red|green|blue)$", "green"));
    assert!(matches("^(?:red|green|blue)$", "blue"));
    assert!(matches("^(a|)$", ""));
    assert!(matches("^()$", ""));
    assert!(matches("^(?:)$", ""));
}

#[test]
fn repetition_forms_and_lazy_suffixes_have_boolean_semantics() {
    for pattern in ["^a*$", "^a*?$", "^(?:a){0,}$"] {
        assert!(matches(pattern, ""));
        assert!(matches(pattern, "aaaa"));
    }
    for pattern in ["^a+$", "^a+?$", "^(?:a){1,}$"] {
        assert!(!matches(pattern, ""));
        assert!(matches(pattern, "aaaa"));
    }
    for pattern in ["^a?$", "^a??$", "^(?:a){0,1}$"] {
        assert!(matches(pattern, ""));
        assert!(matches(pattern, "a"));
        assert!(!matches(pattern, "aa"));
    }
    assert!(matches("^a{3}$", "aaa"));
    assert!(matches("^a{2,4}$", "aa"));
    assert!(matches("^a{2,4}?$", "aaaa"));
    assert!(!matches("^a{2,4}$", "aaaaa"));
}

#[test]
fn character_classes_are_scalar_based() {
    assert!(matches("^[a-c😀]$", "b"));
    assert!(matches("^[a-c😀]$", "😀"));
    assert!(!matches("^[a-c😀]$", "d"));
    assert!(matches("^[^a-c]$", "δ"));
    assert!(matches("^[-a]$", "-"));
    assert!(matches("^[a-]$", "-"));
    assert!(!matches("^[]$", "a"));
    assert!(matches("^[^]$", "a"));
}

#[test]
fn escaped_hyphens_are_literals_or_explicit_range_endpoints() {
    for pattern in [r"^[a\x2dz]$", r"^[a\u002dz]$", r"^[a\u{2d}z]$"] {
        for value in ["a", "-", "z"] {
            assert!(
                matches(pattern, value),
                "{pattern:?} should match {value:?}"
            );
        }
        assert!(!matches(pattern, "m"), "{pattern:?} should not match m");
    }

    assert!(matches(r"^[\x20-\x2d]$", "!"));
    assert!(!matches(r"^[\x20-\x2d]$", "a"));
    assert!(matches(r"^[\x2d-a]$", "0"));
    assert!(!matches(r"^[\x2d-a]$", "z"));
}

#[test]
fn approved_escapes_decode_exactly() {
    assert!(matches(
        r"^\^\$\\\.\*\+\?\(\)\[\]\{\}\|\/$",
        r"^$\.*+?()[]{}|/"
    ));
    assert!(matches(r"^\n\r\t\f\v\0$", "\n\r\t\u{c}\u{b}\0"));
    assert!(matches(r"^\x41\u03b4\u{1f600}$", "Aδ😀"));
    assert!(matches(r"^\uD83D\uDE00$", "😀"));
}

#[test]
fn unsupported_constructs_are_rejected() {
    let invalid = [
        r"\1",
        r"(a)\1",
        r"(?=a)",
        r"(?!a)",
        r"(?<=a)",
        r"(?<!a)",
        r"(?<name>a)",
        r"(?i:a)",
        r"\p{Letter}",
        r"\P{Letter}",
        r"\d",
        r"\D",
        r"\s",
        r"\S",
        r"\w",
        r"\W",
        r"\a",
        r"\01",
        r"\cA",
        r"[\-]",
        r"[a&&b]",
        r"[a--b]",
        r"[a~~b]",
        r"[a||b]",
        r"[a[b]",
        "[a-b-c]",
    ];
    for pattern in invalid {
        assert!(
            PortableJsonPattern::compile(pattern).is_err(),
            "accepted {pattern:?}"
        );
    }
}

#[test]
fn malformed_escapes_classes_ranges_and_quantifiers_are_rejected() {
    let invalid = [
        "\\",
        r"\x0",
        r"\xGG",
        r"\u123",
        r"\u{}",
        r"\u{110000}",
        r"\u{d800}",
        r"\uD800",
        r"\uDC00",
        r"\uD800\u0041",
        "[",
        "[z-a]",
        "*a",
        "a{",
        "a{}",
        "a{,2}",
        "a{2,1}",
        "a{2",
        "a{2,x}",
        "a**",
        "a???",
        "^*",
        "$+",
        "){",
    ];
    for pattern in invalid {
        assert!(
            PortableJsonPattern::compile(pattern).is_err(),
            "accepted {pattern:?}"
        );
    }
}

#[test]
fn compile_errors_report_stable_kinds_and_byte_offsets() {
    let error = compile_error("é[");
    assert_eq!(error.kind(), CompileErrorKind::InvalidCharacterClass);
    assert_eq!(error.byte_offset(), "é".len());

    let error = compile_error("^*");
    assert_eq!(error.kind(), CompileErrorKind::QuantifiedAssertion);
    assert_eq!(error.byte_offset(), 0);
}

#[test]
fn compile_limits_are_enforced_before_unbounded_work() {
    let too_long = "a".repeat(MAX_SOURCE_BYTES + 1);
    assert_eq!(
        compile_error(&too_long).kind(),
        CompileErrorKind::SourceTooLong
    );

    let too_deep = format!(
        "{}a{}",
        "(".repeat(MAX_PARSE_DEPTH + 1),
        ")".repeat(MAX_PARSE_DEPTH + 1)
    );
    assert_eq!(
        compile_error(&too_deep).kind(),
        CompileErrorKind::ParseDepthExceeded
    );

    let too_many_nodes = "a".repeat(MAX_AST_NODES);
    assert_eq!(
        compile_error(&too_many_nodes).kind(),
        CompileErrorKind::AstNodeLimitExceeded
    );

    let instruction_limit = format!("a{{{MAX_INSTRUCTIONS}}}");
    assert_eq!(
        compile_error(&instruction_limit).kind(),
        CompileErrorKind::InstructionLimitExceeded
    );
}

#[test]
fn instruction_counts_are_deterministic() {
    for (source, expected) in [("", 2), ("a", 2), ("a?", 3), ("a*", 3), ("a+", 4)] {
        assert_eq!(
            PortableJsonPattern::validate(source).map(|v| v.instruction_count()),
            Ok(expected)
        );
        assert_eq!(
            PortableJsonPattern::compile(source).map(|p| p.instruction_count()),
            Ok(expected)
        );
    }
}

#[test]
fn count_only_validation_handles_expansion_without_materializing_it() {
    assert_eq!(
        PortableJsonPattern::validate("a{16000}").map(|v| v.instruction_count()),
        Ok(16001)
    );
    assert_eq!(
        PortableJsonPattern::validate("(a{16000}){16000}").map_err(|error| error.kind()),
        Err(CompileErrorKind::InstructionLimitExceeded)
    );
}

#[test]
fn count_only_validation_matches_materialized_complex_programs() {
    for source in [
        "(?:)",
        "(a|b|)",
        "^(ab|c?){0}$",
        "^(ab|c?){2,5}$",
        "(?:[a-z]+|[^]?)",
        "(x{3}|y*)+",
    ] {
        let validation =
            PortableJsonPattern::validate(source).unwrap_or_else(|error| panic!("{error}"));
        let compiled =
            PortableJsonPattern::compile(source).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            validation.instruction_count(),
            compiled.instruction_count(),
            "{source:?}"
        );
    }
}

#[test]
fn budget_charge_counts_unicode_scalars_and_is_shared() {
    let pattern = compiled(".");
    assert_eq!(pattern.work_estimate(""), 2);
    assert_eq!(pattern.work_estimate("😀"), 2);
    assert_eq!(pattern.work_estimate("😀x"), 4);

    let mut remaining = 6;
    assert_eq!(
        pattern.is_match_with_budget("😀x", &mut remaining),
        Ok(true)
    );
    assert_eq!(remaining, 2);
    assert_eq!(
        pattern.is_match_with_budget("abc", &mut remaining),
        Err(MatchError::WorkLimitExceeded {
            required: 6,
            limit: 2
        })
    );
    assert_eq!(remaining, 2);
    assert!(DEFAULT_MATCH_WORK_LIMIT > remaining);
}

#[test]
fn large_epsilon_closures_terminate() {
    let pattern = compiled("^(a?)*$");
    assert_eq!(pattern.is_match(""), Ok(true));
    assert_eq!(pattern.is_match("aaaa"), Ok(true));

    let grouped_assertion = compiled("(^)*");
    assert_eq!(grouped_assertion.is_match("anything"), Ok(true));
}
